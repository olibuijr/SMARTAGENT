//! Minimal PNG decoder for Chrome screenshot output: 8-bit RGBA/RGB/grayscale,
//! non-interlaced, all five scanline filters. Rejects anything Chrome never
//! emits (palette, 16-bit, Adam7) loudly instead of half-supporting it.

use crate::inflate::zlib_decode;

pub struct Image {
    pub width: usize,
    pub height: usize,
    /// RGBA8, row-major, width*height*4 bytes.
    pub pixels: Vec<u8>,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

pub fn decode(data: &[u8]) -> Result<Image, String> {
    if data.len() < 8 || data[..8] != SIGNATURE {
        return Err("not a PNG (bad signature)".into());
    }
    let mut pos = 8;
    let (mut width, mut height, mut color_type) = (0usize, 0usize, 0u8);
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_ihdr = false;
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let body_start = pos + 8;
        if body_start + len + 4 > data.len() {
            return Err("truncated PNG chunk".into());
        }
        let body = &data[body_start..body_start + len];
        match ctype {
            b"IHDR" => {
                if len != 13 {
                    return Err("bad IHDR length".into());
                }
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                let bit_depth = body[8];
                color_type = body[9];
                let interlace = body[12];
                if bit_depth != 8 {
                    return Err(format!("unsupported bit depth {bit_depth} (only 8)"));
                }
                if !matches!(color_type, 0 | 2 | 6) {
                    return Err(format!("unsupported color type {color_type} (gray/RGB/RGBA only)"));
                }
                if interlace != 0 {
                    return Err("interlaced PNG unsupported".into());
                }
                if width == 0 || height == 0 || width > 65535 || height > 65535 {
                    return Err("unreasonable PNG dimensions".into());
                }
                seen_ihdr = true;
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {} // ancillary chunks ignored; CRCs skipped by design
        }
        pos = body_start + len + 4; // skip CRC
    }
    if !seen_ihdr {
        return Err("PNG missing IHDR".into());
    }
    if idat.is_empty() {
        return Err("PNG missing IDAT".into());
    }
    let raw = zlib_decode(&idat)?;
    let bpp: usize = match color_type {
        0 => 1,
        2 => 3,
        _ => 4,
    };
    let stride = width * bpp;
    if raw.len() != height * (stride + 1) {
        return Err(format!(
            "PNG data size mismatch: got {} want {}",
            raw.len(),
            height * (stride + 1)
        ));
    }
    // Unfilter scanlines in place into `scan`.
    let mut scan = vec![0u8; height * stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        for x in 0..stride {
            let a = if x >= bpp { scan[y * stride + x - bpp] } else { 0 }; // left
            let b = if y > 0 { scan[(y - 1) * stride + x] } else { 0 }; // up
            let c = if x >= bpp && y > 0 { scan[(y - 1) * stride + x - bpp] } else { 0 }; // up-left
            let v = src[x];
            scan[y * stride + x] = match filter {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                f => return Err(format!("bad PNG filter {f}")),
            };
        }
    }
    // Expand to RGBA8.
    let mut pixels = vec![0u8; width * height * 4];
    for i in 0..width * height {
        let (r, g, b, a) = match color_type {
            0 => (scan[i], scan[i], scan[i], 255),
            2 => (scan[i * 3], scan[i * 3 + 1], scan[i * 3 + 2], 255),
            _ => (scan[i * 4], scan[i * 4 + 1], scan[i * 4 + 2], scan[i * 4 + 3]),
        };
        pixels[i * 4] = r;
        pixels[i * 4 + 1] = g;
        pixels[i * 4 + 2] = b;
        pixels[i * 4 + 3] = a;
    }
    Ok(Image { width, height, pixels })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (pa, pb, pc) = (
        (b as i16 - c as i16).abs(),
        (a as i16 - c as i16).abs(),
        (a as i16 + b as i16 - 2 * c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PNG in-test: stored-block zlib, chosen filters.
    fn make_png(width: usize, height: usize, color_type: u8, scanlines: &[u8]) -> Vec<u8> {
        fn chunk(ctype: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&(body.len() as u32).to_be_bytes());
            v.extend_from_slice(ctype);
            v.extend_from_slice(body);
            v.extend_from_slice(&[0, 0, 0, 0]); // CRC unchecked by decoder
            v
        }
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(height as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
        let idat = crate::inflate::zlib_stored(scanlines);
        let mut png = SIGNATURE.to_vec();
        png.extend_from_slice(&chunk(b"IHDR", &ihdr));
        png.extend_from_slice(&chunk(b"IDAT", &idat));
        png.extend_from_slice(&chunk(b"IEND", &[]));
        png
    }

    #[test]
    fn decode_rgba_filter0() {
        // 2x1: red, blue — filter 0.
        let png = make_png(2, 1, 6, &[0, 255, 0, 0, 255, 0, 0, 255, 255]);
        let img = decode(&png).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.pixels, &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn decode_rgb_sub_filter() {
        // 2x1 RGB with filter 1 (Sub): raw (10,20,30), delta (5,5,5) → (15,25,35).
        let png = make_png(2, 1, 2, &[1, 10, 20, 30, 5, 5, 5]);
        let img = decode(&png).unwrap();
        assert_eq!(&img.pixels, &[10, 20, 30, 255, 15, 25, 35, 255]);
    }

    #[test]
    fn decode_gray_up_filter() {
        // 1x2 gray: row0 filter 0 value 100; row1 filter 2 (Up) delta 20 → 120.
        let png = make_png(1, 2, 0, &[0, 100, 2, 20]);
        let img = decode(&png).unwrap();
        assert_eq!(&img.pixels, &[100, 100, 100, 255, 120, 120, 120, 255]);
    }

    #[test]
    fn decode_average_and_paeth_filters() {
        // 2x3 gray exercising filter 3 (Average) and 4 (Paeth), computed by hand:
        // row0 (filter 0): [10, 20]
        // row1 (filter 3): x0 = 5 + (0+10)/2 = 10; x1 = 6 + (10+20)/2 = 21
        // row2 (filter 4): x0 = 7 + paeth(0,10,0)=10 → 17; x1 = 8 + paeth(17,21,10)=21 → 29
        let png = make_png(2, 3, 0, &[0, 10, 20, 3, 5, 6, 4, 7, 8]);
        let img = decode(&png).unwrap();
        let gray: Vec<u8> = img.pixels.chunks(4).map(|p| p[0]).collect();
        assert_eq!(gray, vec![10, 20, 10, 21, 17, 29]);
    }

    #[test]
    fn reject_palette_and_interlace() {
        let png = make_png(1, 1, 3, &[0, 0]);
        assert!(decode(&png).is_err());
        let mut png2 = make_png(1, 1, 0, &[0, 0]);
        // Flip interlace byte inside IHDR (offset: 8 sig + 8 hdr + 12 = byte 28).
        png2[28] = 1;
        assert!(decode(&png2).is_err());
    }

    #[test]
    fn reject_bad_signature() {
        assert!(decode(b"not a png at all").is_err());
    }
}
