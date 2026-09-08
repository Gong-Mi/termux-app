//! CI experiment only: draw real terminal text through the production
//! `TerminalRenderer::draw_frame` path onto a Ganesh Vulkan render target and
//! read back pixels. Unlike `skia_backend_probe` (an 8x8 API rectangle), this
//! exercises the font/TextBlob/frame pipeline end to end.
use skia_safe::{AlphaType, ColorType, ImageInfo, gpu};

use crate::renderer::{RenderFrame, TerminalRenderer};
use crate::terminal::colors::TerminalColors;
use crate::terminal::style::{STYLE_NORMAL, encode_style};

const TEXT: &str = "Hi";
const COLS: usize = 16;
const ROWS: usize = 3;
const FONT_SIZE: f32 = 16.0;

pub(crate) fn draw_text_and_readback(context: &mut gpu::DirectContext) -> Result<(), &'static str> {
    if context.backend() != gpu::BackendAPI::Vulkan {
        return Err("not a Vulkan context");
    }
    let mut renderer = TerminalRenderer::new(&[], FONT_SIZE, None);
    if renderer.font_width <= 0.0 || renderer.font_height <= 0.0 {
        return Err("font metrics unavailable");
    }

    let width = (COLS as f32 * renderer.font_width).ceil() as i32;
    let height = (ROWS as f32 * renderer.font_height).ceil() as i32;
    if width <= 0 || height <= 0 {
        return Err("invalid probe surface size");
    }
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = gpu::surfaces::render_target(
        context,
        gpu::Budgeted::Yes,
        &info,
        None,
        gpu::SurfaceOrigin::TopLeft,
        None,
        false,
        false,
    )
    .ok_or("GPU render target allocation failed")?;

    let palette = TerminalColors::new().current_colors;
    let mut row_data = Vec::with_capacity(ROWS);
    for r in 0..ROWS {
        let mut text: Vec<char> = " ".repeat(COLS).chars().collect();
        let mut styles = vec![STYLE_NORMAL; COLS];
        if r == 0 {
            // Index 15 = bright white foreground, 257 = default background.
            let text_style = encode_style(15, 257, 0);
            for (i, ch) in TEXT.chars().enumerate() {
                if i >= COLS {
                    break;
                }
                text[i] = ch;
                styles[i] = text_style;
            }
        }
        row_data.push((text, styles, 0u64));
    }
    let frame = RenderFrame {
        rows: ROWS,
        cols: COLS,
        palette,
        use_alternate_buffer: false,
        cursor_x: 0,
        cursor_y: 0,
        cursor_style: 0,
        cursor_enabled: false,
        reverse_video: false,
        top_row: 0,
        row_data,
    };

    renderer.draw_frame(surface.canvas(), &frame, 1.0, 0.0);
    context.flush_and_submit();
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    if !surface.read_pixels(&info, &mut pixels, width as usize * 4, (0, 0)) {
        return Err("GPU read_pixels failed");
    }
    verify_text_pixels(
        &pixels,
        width as usize,
        height as usize,
        renderer.font_height,
    )
}

fn is_foreground(pixel: &[u8]) -> bool {
    // Bright-white glyph on the default dark background; tolerance covers
    // SwiftShader antialiasing without admitting the dark background itself.
    pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200 && pixel[3] > 200
}

/// Structure checks that distinguish a rendered glyph row from a blank frame
/// or a solid fill: foreground exists in the text band, occupies a minority
/// of it (glyph counters), stays out of the empty rows below, and never fills
/// its own bounding box (a solid rectangle has no glyph strokes or gaps).
fn verify_text_pixels(
    pixels: &[u8],
    width: usize,
    height: usize,
    font_height: f32,
) -> Result<(), &'static str> {
    let band = font_height.ceil() as usize;
    if band == 0 || band * 2 > height || width == 0 {
        return Err("probe geometry mismatch");
    }
    let mut band_fg = 0usize;
    let mut min_x = width;
    let mut max_x = 0usize;
    let mut min_y = band;
    let mut max_y = 0usize;
    for y in 0..band {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            if is_foreground(&pixels[offset..offset + 4]) {
                band_fg += 1;
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }
    let band_total = band * width;
    if band_fg == 0 {
        return Err("no glyph pixels in text band");
    }
    if band_fg * 100 > band_total * 60 {
        return Err("text band looks like a solid fill");
    }
    if band_fg < 20 {
        return Err("too few glyph pixels to be a rendered string");
    }
    for y in band..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            if is_foreground(&pixels[offset..offset + 4]) {
                return Err("foreground leaked into empty rows");
            }
        }
    }
    // A glyph's ink never fills its own bounding box: strokes leave gaps
    // between and inside glyphs. This holds for any baseline placement, so it
    // does not assume where inside the cell the renderer draws the text.
    let bbox = (max_x - min_x + 1) * (max_y - min_y + 1);
    if band_fg >= bbox {
        return Err("glyph band is a solid rectangle");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 16;
    const BAND: usize = 8;
    const H: usize = 24;

    fn blank() -> Vec<u8> {
        vec![0u8; W * H * 4]
    }

    fn fg(buffer: &mut [u8], x: usize, y: usize) {
        let offset = (y * W + x) * 4;
        buffer[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
    }

    #[test]
    fn oracle_rejects_blank_fill_leak_and_inverted_mass() {
        assert_eq!(
            verify_text_pixels(&blank(), W, H, BAND as f32),
            Err("no glyph pixels in text band")
        );
        let mut solid = blank();
        for y in 0..BAND {
            for x in 0..W {
                fg(&mut solid, x, y);
            }
        }
        assert_eq!(
            verify_text_pixels(&solid, W, H, BAND as f32),
            Err("text band looks like a solid fill")
        );
        let mut leak = blank();
        for y in 0..BAND {
            for x in 0..W {
                fg(&mut leak, x, y);
            }
        }
        // Make it non-solid by removing most of the band, then leak into row 1.
        for y in 0..BAND {
            for x in 4..W {
                let offset = (y * W + x) * 4;
                leak[offset..offset + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        fg(&mut leak, 0, BAND);
        assert_eq!(
            verify_text_pixels(&leak, W, H, BAND as f32),
            Err("foreground leaked into empty rows")
        );
        let mut inverted = blank();
        for y in BAND / 2..BAND {
            for x in 0..8 {
                fg(&mut inverted, x, y);
            }
        }
        assert_eq!(
            verify_text_pixels(&inverted, W, H, BAND as f32),
            Err("glyph band is a solid rectangle")
        );
    }

    #[test]
    fn oracle_accepts_glyph_like_pattern() {
        let mut ok = blank();
        // Upper-heavy glyph pattern with counters, density ~25% of the band.
        for y in 0..BAND / 2 {
            for x in 0..W {
                if (x + y) % 2 == 0 {
                    fg(&mut ok, x, y);
                }
            }
        }
        for x in 0..W {
            if x % 3 == 0 {
                fg(&mut ok, x, BAND / 2);
            }
        }
        assert_eq!(verify_text_pixels(&ok, W, H, BAND as f32), Ok(()));
    }
}
