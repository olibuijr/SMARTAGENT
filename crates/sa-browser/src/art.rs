//! RGBA → high-DPI terminal art. Three pixel modes, best DPI first:
//!
//! - `braille` (default): 2×4 px per cell via Unicode Braille Patterns.
//!   Each cell carries 2 colors — pixels are clustered to a fg/bg pair
//!   (1-step 2-means on RGB) and the membership bitmap picks the glyph.
//!   8 px/cell.
//! - `sextant`: 2×3 px per cell via Unicode Symbols for Legacy Computing.
//!   Same two-color clustering. 6 px/cell.
//! - `quad`: 2×2 px per cell via quadrant blocks (▘▝▖▗…). 4 px/cell.
//! - `half`: 1×2 px per cell via ▀ with truecolor fg/bg — every pixel gets
//!   its exact color (color-true maximum). 2 px/cell.
//!
//! Fit is computed in CELL units against the physical 1:2 cell aspect, so all
//! modes show the same geometry — higher modes just sample more pixels into
//! it. Every line ends with an SGR reset so colors never bleed into the TUI.

use crate::png::Image;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Half,
    Quad,
    Sextant,
    Braille,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "half" => Mode::Half,
            "quad" | "quadrant" => Mode::Quad,
            "sextant" | "sextants" => Mode::Sextant,
            _ => Mode::Braille,
        }
    }

    /// Pixels sampled per cell (width, height).
    fn cell_px(self) -> (usize, usize) {
        match self {
            Mode::Half => (1, 2),
            Mode::Quad => (2, 2),
            Mode::Sextant => (2, 3),
            Mode::Braille => (2, 4),
        }
    }
}

/// Render `img` to ANSI lines fitting `cols` columns and at most `max_rows`
/// text rows. Returns (lines, used_cols, used_rows).
pub fn render(img: &Image, cols: usize, max_rows: usize, mode: Mode) -> (Vec<String>, usize, usize) {
    let cols = cols.max(1);
    let max_rows = max_rows.max(1);
    // Fit in cell units against physical cell aspect (1:2): a source pixel
    // maps to 1 cell-width × 2 cell-width of height. Same geometry every mode.
    let scale_w = cols as f64 / img.width as f64;
    let scale_h = (max_rows * 2) as f64 / img.height as f64;
    let scale = scale_w.min(scale_h).min(1.0); // never upscale (in cell terms)
    let w_cells = ((img.width as f64 * scale).round() as usize).clamp(1, cols);
    let h_cells = (((img.height as f64 * scale) / 2.0).round() as usize).clamp(1, max_rows);

    let (cw, ch) = mode.cell_px();
    let px = downscale(img, w_cells * cw, h_cells * ch);
    let mut lines = Vec::with_capacity(h_cells);
    for row in 0..h_cells {
        let mut line = String::with_capacity(w_cells * 24);
        let (mut last_fg, mut last_bg) = (None, None);
        for cx in 0..w_cells {
            // Gather this cell's pixel block.
            let mut cell = [(0u8, 0u8, 0u8); 8];
            for py in 0..ch {
                for pxx in 0..cw {
                    cell[py * cw + pxx] = px[(row * ch + py) * (w_cells * cw) + cx * cw + pxx];
                }
            }
            let block = &cell[..cw * ch];
            let (fg, bg, glyph) = match mode {
                Mode::Half => (block[0], block[1], '▀'),
                _ => two_color_cell(block, mode),
            };
            if last_fg != Some(fg) {
                line.push_str(&format!("\x1b[38;2;{};{};{}m", fg.0, fg.1, fg.2));
                last_fg = Some(fg);
            }
            if last_bg != Some(bg) {
                line.push_str(&format!("\x1b[48;2;{};{};{}m", bg.0, bg.1, bg.2));
                last_bg = Some(bg);
            }
            line.push(glyph);
        }
        line.push_str("\x1b[0m");
        lines.push(line);
    }
    (lines, w_cells, h_cells)
}

/// Cluster a cell's pixels to a fg/bg pair (seeded by min/max luminance, one
/// reassignment pass) and pick the glyph whose bitmap matches membership.
fn two_color_cell(block: &[(u8, u8, u8)], mode: Mode) -> ((u8, u8, u8), (u8, u8, u8), char) {
    let luma = |c: (u8, u8, u8)| c.0 as i32 * 299 + c.1 as i32 * 587 + c.2 as i32 * 114;
    let (mut lo, mut hi) = (block[0], block[0]);
    for &c in block {
        if luma(c) < luma(lo) {
            lo = c;
        }
        if luma(c) > luma(hi) {
            hi = c;
        }
    }
    if lo == hi {
        // Flat cell: background-only glyph keeps runs long (cheap diffing).
        return (hi, lo, ' ');
    }
    let dist = |a: (u8, u8, u8), b: (u8, u8, u8)| {
        let (dr, dg, db) = (a.0 as i32 - b.0 as i32, a.1 as i32 - b.1 as i32, a.2 as i32 - b.2 as i32);
        dr * dr + dg * dg + db * db
    };
    let mut bits = 0u8;
    let (mut fsum, mut fn_, mut bsum, mut bn) = ([0u32; 3], 0u32, [0u32; 3], 0u32);
    for (i, &c) in block.iter().enumerate() {
        if dist(c, hi) <= dist(c, lo) {
            bits |= 1 << i;
            fsum[0] += c.0 as u32;
            fsum[1] += c.1 as u32;
            fsum[2] += c.2 as u32;
            fn_ += 1;
        } else {
            bsum[0] += c.0 as u32;
            bsum[1] += c.1 as u32;
            bsum[2] += c.2 as u32;
            bn += 1;
        }
    }
    let fg = ((fsum[0] / fn_.max(1)) as u8, (fsum[1] / fn_.max(1)) as u8, (fsum[2] / fn_.max(1)) as u8);
    let bg = if bn == 0 { fg } else { ((bsum[0] / bn) as u8, (bsum[1] / bn) as u8, (bsum[2] / bn) as u8) };
    let glyph = match mode {
        Mode::Quad => QUAD[bits as usize],
        Mode::Sextant => sextant_char(bits),
        Mode::Braille => braille_char(bits),
        Mode::Half => '▀',
    };
    (fg, bg, glyph)
}

/// Quadrant glyphs indexed by bitmap: bit0=TL, bit1=TR, bit2=BL, bit3=BR.
const QUAD: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// Sextant glyph for a 6-bit bitmap (bit i = pixel i lit; row-major 2×3,
/// bit0=top-left … bit5=bottom-right). U+1FB00.. block skips the patterns
/// that already exist elsewhere: empty (space), left half ▌, right half ▐,
/// full █.
pub fn sextant_char(bits: u8) -> char {
    match bits {
        0 => ' ',
        0b010101 => '▌',
        0b101010 => '▐',
        63 => '█',
        b => {
            let mut offset = b as u32 - 1;
            if b > 0b010101 {
                offset -= 1;
            }
            if b > 0b101010 {
                offset -= 1;
            }
            char::from_u32(0x1FB00 + offset).unwrap_or('▒')
        }
    }
}

/// Braille glyph for a 2×4 bitmap. Input bits are row-major; Unicode braille
/// dot order is left column dots 1–4 then right column dots 5–8.
pub fn braille_char(bits: u8) -> char {
    let map = [0x01u8, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];
    let mut dots = 0u8;
    for (i, dot) in map.iter().enumerate() {
        if bits & (1 << i) != 0 {
            dots |= *dot;
        }
    }
    char::from_u32(0x2800 + dots as u32).unwrap_or('⣿')
}

/// Box-filter downscale to w×h, alpha composited over white (screenshots are
/// opaque; transparent regions read as page background).
fn downscale(img: &Image, w: usize, h: usize) -> Vec<(u8, u8, u8)> {
    let mut out = Vec::with_capacity(w * h);
    for ty in 0..h {
        let y0 = ty * img.height / h;
        let y1 = (((ty + 1) * img.height).div_ceil(h)).min(img.height).max(y0 + 1);
        for tx in 0..w {
            let x0 = tx * img.width / w;
            let x1 = (((tx + 1) * img.width).div_ceil(w)).min(img.width).max(x0 + 1);
            let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
            let mut n = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * img.width + x) * 4;
                    let a = img.pixels[i + 3] as u64;
                    // Composite over white: c*a/255 + 255*(255-a)/255
                    r += (img.pixels[i] as u64 * a + 255 * (255 - a)) / 255;
                    g += (img.pixels[i + 1] as u64 * a + 255 * (255 - a)) / 255;
                    b += (img.pixels[i + 2] as u64 * a + 255 * (255 - a)) / 255;
                    n += 1;
                }
            }
            out.push(((r / n) as u8, (g / n) as u8, (b / n) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(width: usize, height: usize, rgba: &[u8]) -> Image {
        Image { width, height, pixels: rgba.to_vec() }
    }

    #[test]
    fn half_one_cell_red_over_blue() {
        let i = img(1, 2, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let (lines, w, rows) = render(&i, 10, 10, Mode::Half);
        assert_eq!((w, rows), (1, 1));
        assert_eq!(lines[0], "\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀\x1b[0m");
    }

    #[test]
    fn sextant_codepoints_known() {
        assert_eq!(sextant_char(0), ' ');
        assert_eq!(sextant_char(0b000001), '\u{1FB00}'); // top-left only
        assert_eq!(sextant_char(0b000011), '\u{1FB02}'); // top row
        assert_eq!(sextant_char(0b010101), '▌'); // left column
        assert_eq!(sextant_char(0b101010), '▐'); // right column
        assert_eq!(sextant_char(63), '█');
        assert_eq!(sextant_char(62), '\u{1FB3B}'); // last block glyph
    }

    #[test]
    fn sextant_cell_splits_white_top_black_bottom() {
        // 2x3 source: top row white, bottom two rows black → bits = 0b000011
        // (top-left + top-right lit) → U+1FB02, fg white, bg black.
        let mut rgba = Vec::new();
        for y in 0..3 {
            for _x in 0..2 {
                let v = if y == 0 { 255u8 } else { 0u8 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let i = img(2, 3, &rgba);
        let (lines, w, rows) = render(&i, 1, 1, Mode::Sextant);
        assert_eq!((w, rows), (1, 1));
        assert!(lines[0].contains('\u{1FB02}'), "got {:?}", lines[0]);
        assert!(lines[0].contains("38;2;255;255;255"));
        assert!(lines[0].contains("48;2;0;0;0"));
    }

    #[test]
    fn quad_cell_left_column_lit() {
        // 2x2: left column white, right column black → bits TL|BL = 0b0101 → ▌
        let rgba = [255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255];
        let i = img(2, 2, &rgba);
        let (lines, ..) = render(&i, 1, 1, Mode::Quad);
        assert!(lines[0].contains('▌'), "got {:?}", lines[0]);
    }

    #[test]
    fn flat_cell_uses_space_run() {
        let rgba: Vec<u8> = (0..6).flat_map(|_| [128, 128, 128, 255]).collect();
        let i = img(2, 3, &rgba);
        let (lines, ..) = render(&i, 1, 1, Mode::Sextant);
        assert!(lines[0].contains(' '));
        assert!(lines[0].contains("48;2;128;128;128"), "got {:?}", lines[0]);
    }

    #[test]
    fn every_line_ends_with_reset_all_modes() {
        let i = img(8, 12, &[90u8; 8 * 12 * 4]);
        for mode in [Mode::Half, Mode::Quad, Mode::Sextant] {
            let (lines, ..) = render(&i, 4, 2, mode);
            for l in &lines {
                assert!(l.ends_with("\x1b[0m"), "line missing SGR reset: {l:?}");
            }
        }
    }

    #[test]
    fn geometry_identical_across_modes() {
        // Fit is in cell units — same cols/rows regardless of pixel density.
        let i = img(100, 100, &vec![200u8; 100 * 100 * 4]);
        for mode in [Mode::Half, Mode::Quad, Mode::Sextant] {
            let (lines, w, rows) = render(&i, 20, 50, mode);
            assert_eq!((w, rows), (20, 10));
            assert_eq!(lines.len(), 10);
        }
        // Height-capped: width shrinks to preserve aspect.
        let (_, w2, rows2) = render(&i, 100, 5, Mode::Sextant);
        assert_eq!(rows2, 5);
        assert!(w2 <= 11, "width should shrink to preserve aspect, got {w2}");
    }

    #[test]
    fn never_upscales() {
        let i = img(2, 2, &[10u8; 2 * 2 * 4]);
        let (_, w, rows) = render(&i, 80, 40, Mode::Sextant);
        assert_eq!((w, rows), (2, 1));
    }

    #[test]
    fn alpha_composites_over_white() {
        let i = img(1, 2, &[0, 0, 0, 0, 0, 0, 0, 0]);
        let (lines, ..) = render(&i, 1, 1, Mode::Half);
        assert!(lines[0].contains("38;2;255;255;255"));
    }
}
