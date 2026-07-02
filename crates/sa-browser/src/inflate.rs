//! zlib/DEFLATE decompressor (RFC 1950 + 1951), std-only. Just enough to
//! decode PNG IDAT streams from Chrome screenshots: stored blocks, fixed
//! Huffman, dynamic Huffman, 32K LZ77 window. Verifies the adler32 trailer.

/// Decode a zlib stream (2-byte header + deflate data + adler32 trailer).
pub fn zlib_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib stream too short".into());
    }
    let cmf = data[0];
    if cmf & 0x0F != 8 {
        return Err(format!("unsupported zlib method {}", cmf & 0x0F));
    }
    if data[1] & 0x20 != 0 {
        return Err("zlib preset dictionary unsupported".into());
    }
    let out = inflate(&data[2..data.len() - 4])?;
    let want = u32::from_be_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if adler32(&out) != want {
        return Err("zlib adler32 mismatch".into());
    }
    Ok(out)
}

/// adler32 checksum (RFC 1950). pub(crate) so PNG test fixtures can build streams.
pub(crate) fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // byte position
    bit: u32,   // bit position within current byte (LSB first)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, bit: 0 }
    }

    fn bits(&mut self, n: u32) -> Result<u32, String> {
        let mut v = 0u32;
        for i in 0..n {
            if self.pos >= self.data.len() {
                return Err("deflate: unexpected end of stream".into());
            }
            let b = (self.data[self.pos] >> self.bit) & 1;
            v |= (b as u32) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(v)
    }

    fn align_byte(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// Canonical Huffman table: decode by walking bit-by-bit through code space.
struct Huffman {
    /// counts[len] = number of codes of that bit length (index 0 unused)
    counts: [u16; 16],
    /// symbols ordered by (length, symbol)
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err("huffman length > 15".into());
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        // Over-subscription check.
        let mut left = 1i32;
        for len in 1..16 {
            left <<= 1;
            left -= counts[len] as i32;
            if left < 0 {
                return Err("huffman over-subscribed".into());
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, r: &mut BitReader) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("huffman: invalid code".into())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Inflate a raw DEFLATE stream (RFC 1951).
pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut r = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = r.bits(1)? == 1;
        match r.bits(2)? {
            0 => {
                // Stored block: LEN + ~LEN then raw bytes.
                r.align_byte();
                if r.pos + 4 > r.data.len() {
                    return Err("stored block header truncated".into());
                }
                let len = u16::from_le_bytes([r.data[r.pos], r.data[r.pos + 1]]) as usize;
                let nlen = u16::from_le_bytes([r.data[r.pos + 2], r.data[r.pos + 3]]) as usize;
                if len != (!nlen) & 0xFFFF {
                    return Err("stored block LEN/NLEN mismatch".into());
                }
                r.pos += 4;
                if r.pos + len > r.data.len() {
                    return Err("stored block truncated".into());
                }
                out.extend_from_slice(&r.data[r.pos..r.pos + len]);
                r.pos += len;
            }
            1 => {
                let (lit, dist) = fixed_tables()?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut r)?;
                inflate_block(&mut r, &mut out, &lit, &dist)?;
            }
            _ => return Err("deflate: reserved block type".into()),
        }
        if last {
            return Ok(out);
        }
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), String> {
    let mut lit = [0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lit)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_tables(r: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let hlit = r.bits(5)? as usize + 257;
    let hdist = r.bits(5)? as usize + 1;
    let hclen = r.bits(4)? as usize + 4;
    let mut clen = [0u8; 19];
    for &idx in ORDER.iter().take(hclen) {
        clen[idx] = r.bits(3)? as u8;
    }
    let cl_huff = Huffman::new(&clen)?;
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < lengths.len() {
        let sym = cl_huff.decode(r)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("repeat with no previous length".into());
                }
                let prev = lengths[i - 1];
                for _ in 0..(3 + r.bits(2)?) {
                    if i >= lengths.len() {
                        return Err("code lengths overflow".into());
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                for _ in 0..(3 + r.bits(3)?) {
                    if i >= lengths.len() {
                        return Err("code lengths overflow".into());
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            18 => {
                for _ in 0..(11 + r.bits(7)?) {
                    if i >= lengths.len() {
                        return Err("code lengths overflow".into());
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err("bad code-length symbol".into()),
        }
    }
    if lengths[256] == 0 {
        return Err("no end-of-block code".into());
    }
    Ok((Huffman::new(&lengths[..hlit])?, Huffman::new(&lengths[hlit..])?))
}

fn inflate_block(
    r: &mut BitReader,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Ok(()),
            257..=285 => {
                let li = (sym - 257) as usize;
                let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li] as u32)? as usize;
                let dsym = dist.decode(r)? as usize;
                if dsym >= 30 {
                    return Err("bad distance symbol".into());
                }
                let d = DIST_BASE[dsym] as usize + r.bits(DIST_EXTRA[dsym] as u32)? as usize;
                if d > out.len() {
                    return Err("distance beyond output".into());
                }
                let start = out.len() - d;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return Err("bad literal/length symbol".into()),
        }
    }
}

/// Build a zlib stream of stored (uncompressed) blocks around `data`.
/// Test helper shared with the PNG decoder's fixtures.
#[cfg(test)]
pub(crate) fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut v = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(65535).collect() };
    for (i, c) in chunks.iter().enumerate() {
        let last = i == chunks.len() - 1;
        v.push(if last { 1 } else { 0 }); // BFINAL, BTYPE=00
        v.extend_from_slice(&(c.len() as u16).to_le_bytes());
        v.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        v.extend_from_slice(c);
    }
    v.extend_from_slice(&adler32(data).to_be_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_roundtrip() {
        let data = b"hello stored block";
        assert_eq!(zlib_decode(&zlib_stored(data)).unwrap(), data);
    }

    #[test]
    fn empty_stored() {
        assert_eq!(zlib_decode(&zlib_stored(b"")).unwrap(), b"");
    }

    #[test]
    fn fixed_huffman_vector() {
        // zlib.deflateSync(Buffer.from("hello hello hello\n"), {level: 1}) — fixed-Huffman
        // (BTYPE=1) stream with an LZ77 back-reference, generated once with bun.
        let stream: &[u8] = &[
            0x78, 0x01, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0x40, 0x22, 0xb9, 0x00, 0x40, 0xb5,
            0x06, 0x87,
        ];
        assert_eq!(zlib_decode(stream).unwrap(), b"hello hello hello\n");
    }

    #[test]
    fn dynamic_huffman_vector() {
        // zlib.deflateSync (level 9) of 200 numbered pangram sentences —
        // dynamic-Huffman (BTYPE=2, verified). Generated once with bun; stored
        // as a fixture to keep this file readable.
        let expected: Vec<u8> = {
            let mut s = Vec::new();
            for i in 0..200 {
                s.extend_from_slice(format!("The quick brown fox {i} jumps over the lazy dog. ").as_bytes());
            }
            s
        };
        let stream = include_bytes!("../tests/fixtures/dynamic.zlib");
        assert_eq!((stream[2] >> 1) & 3, 2, "fixture must be a dynamic-Huffman block");
        assert_eq!(zlib_decode(stream).unwrap(), expected);
    }

    #[test]
    fn adler32_known() {
        // adler32("Wikipedia") = 0x11E60398
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn corrupt_adler_rejected() {
        let mut s = zlib_stored(b"abc");
        let n = s.len();
        s[n - 1] ^= 0xFF;
        assert!(zlib_decode(&s).is_err());
    }
}
