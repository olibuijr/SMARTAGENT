//! RGBA → high-DPI terminal art. Each cell is a half-block `▀` with truecolor
//! foreground (top pixel) and background (bottom pixel): 2 color-true pixels
//! per cell — the physical maximum for color-faithful terminal rendering.
//! Box-filter downscale, aspect preserved under 1:2 cell geometry, every line
//! ends with an SGR reset so colors never bleed into the surrounding TUI.

use crate::png::Image;

/// Render `img` to ANSI lines fitting `cols` columns and at most `max_rows`
/// text rows. Returns (lines, used_cols, used_rows).
pub fn render(img: &Image, cols: usize, max_rows: usize) -> (Vec<String>, usize, usize) {
    let cols = cols.max(1);
    let max_rows = max_rows.max(1);
    // Target pixel grid: W x H where each text row holds 2 pixel rows.
    // A terminal cell is ~1:2 (w:h), and the half-block packs 2 vertical
    // pixels per cell, so square source pixels map 1:1 — scale to fit.
    let scale_w = cols as f64 / img.width as f64;
    let scale_h = (max_rows * 2) as f64 / img.height as f64;
    let scale = scale_w.min(scale_h).min(1.0); // never upscale
    let w = ((img.width as f64 * scale).round() as usize).clamp(1, cols);
    let mut h = (img.height as f64 * scale).round() as usize;
    if h == 0 {
        h = 1;
    }
    if h % 2 == 1 {
        h += 1; // whole cells
    }
    h = h.min(max_rows * 2);

    let px = downscale(img, w, h);
    let mut lines = Vec::with_capacity(h / 2);
    for row in 0..h / 2 {
        let mut line = String::with_capacity(w * 24);
        let (mut last_fg, mut last_bg) = (None, None);
        for x in 0..w {
            let top = px[row * 2 * w + x];
            let bot = if row * 2 + 1 < h { px[(row * 2 + 1) * w + x] } else { top };
            if last_fg != Some(top) {
                line.push_str(&format!("\x1b[38;2;{};{};{}m", top.0, top.1, top.2));
                last_fg = Some(top);
            }
            if last_bg != Some(bot) {
                line.push_str(&format!("\x1b[48;2;{};{};{}m", bot.0, bot.1, bot.2));
                last_bg = Some(bot);
            }
            line.push('▀');
        }
        line.push_str("\x1b[0m");
        lines.push(line);
    }
    (lines, w, h / 2)
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
    fn one_cell_red_over_blue() {
        // 1x2 image: red top pixel, blue bottom pixel → one ▀ cell.
        let i = img(1, 2, &[255, 0, 0, 255, 0, 0, 255, 255]);
        let (lines, w, rows) = render(&i, 10, 10);
        assert_eq!((w, rows), (1, 1));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀\x1b[0m");
    }

    #[test]
    fn every_line_ends_with_reset() {
        let i = img(4, 4, &[128u8; 4 * 4 * 4]);
        let (lines, _, _) = render(&i, 2, 2);
        assert!(!lines.is_empty());
        for l in &lines {
            assert!(l.ends_with("\x1b[0m"), "line missing SGR reset: {l:?}");
        }
    }

    #[test]
    fn aspect_fits_width_and_height_cap() {
        // 100x100 source into 20 cols → 20x20 px → 10 rows.
        let i = img(100, 100, &vec![200u8; 100 * 100 * 4]);
        let (lines, w, rows) = render(&i, 20, 50);
        assert_eq!((w, rows), (20, 10));
        assert_eq!(lines.len(), 10);
        // Height-capped: 100x100 into 100 cols but only 5 rows → 10 px tall, 10 wide.
        let (_, w2, rows2) = render(&i, 100, 5);
        assert_eq!(rows2, 5);
        assert!(w2 <= 10 + 1, "width should shrink to preserve aspect, got {w2}");
    }

    #[test]
    fn never_upscales() {
        let i = img(2, 2, &[10u8; 2 * 2 * 4]);
        let (_, w, rows) = render(&i, 80, 40);
        assert_eq!((w, rows), (2, 1));
    }

    #[test]
    fn alpha_composites_over_white() {
        // Fully transparent pixel renders white.
        let i = img(1, 2, &[0, 0, 0, 0, 0, 0, 0, 0]);
        let (lines, _, _) = render(&i, 1, 1);
        assert!(lines[0].contains("38;2;255;255;255"));
    }
}
