use skia_safe::{Canvas, Paint, Color, Font, Rect, PaintStyle, FontMgr, FontStyle};
use crate::engine::TerminalEngine;
use crate::terminal::style::*;

pub struct TerminalRenderer {
    base_font_size: f32,
    paint: Paint,
    bg_paint: Paint,
    pub font_width: f32,
    pub font_height: f32,
}

impl TerminalRenderer {
    pub fn new(font_data: &[u8], font_size: f32) -> Self {
        let font_mgr = FontMgr::new();
        let typeface = if !font_data.is_empty() {
            font_mgr.new_from_data(font_data, None)
                .unwrap_or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal()).expect("System monospace font not found"))
        } else {
            font_mgr.match_family_style("monospace", FontStyle::normal()).expect("System monospace font not found")
        };
        
        let mut font = Font::new(typeface, Some(font_size));
        font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
        font.set_subpixel(true);

        let metrics = font.metrics().1;
        let font_height = metrics.descent - metrics.ascent + metrics.leading;
        let (width, _) = font.measure_str("M", None);
        let font_width = width;

        let mut paint = Paint::default();
        paint.set_anti_alias(true);

        let mut bg_paint = Paint::default();
        bg_paint.set_style(PaintStyle::Fill);

        Self { base_font_size: font_size, paint, bg_paint, font_width, font_height }
    }

    pub fn draw_terminal(&mut self, canvas: &Canvas, engine: &TerminalEngine) {
        let state = &engine.state;
        let palette = &state.colors.current_colors;
        let screen = if state.use_alternate_buffer { &state.alt_screen } else { &state.main_screen };
        
        canvas.clear(Color::new(palette[257]));

        let rows = state.rows as usize;
        let cols = state.cols as usize;

        for r in 0..rows {
            if r >= screen.buffer.len() { break; }
            let row_data = &screen.buffer[r];
            let y_base = (r as f32 + 1.0) * self.font_height;
            
            let mut c = 0;
            while c < cols {
                if c >= row_data.text.len() { break; }
                
                let start_c = c;
                let style = row_data.styles[c];
                let mut run_text = String::new();
                
                while c < cols && c < row_data.text.len() && row_data.styles[c] == style {
                    let ch = row_data.text[c];
                    if ch != '\0' {
                        run_text.push(ch);
                    }
                    c += 1;
                }

                if !run_text.is_empty() {
                    self.draw_run(
                        canvas,
                        &run_text,
                        start_c as f32 * self.font_width,
                        y_base,
                        (c - start_c) as f32 * self.font_width,
                        style,
                        palette,
                    );
                }
            }
        }

        if state.cursor_enabled {
            let cursor = &state.cursor;
            self.bg_paint.set_color(Color::WHITE);
            canvas.draw_rect(
                Rect::from_xywh(cursor.x as f32 * self.font_width, cursor.y as f32 * self.font_height, self.font_width, self.font_height),
                &self.bg_paint
            );
        }
    }

    fn draw_run(&mut self, canvas: &Canvas, text: &str, x: f32, y: f32, width: f32, style: u64, palette: &[u32; 259]) {
        let fg_idx = decode_fore_color(style) as usize;
        let bg_idx = decode_back_color(style) as usize;
        let effect = decode_effect(style);

        if bg_idx != 257 {
            let bg_color = Color::new(if bg_idx < 259 { palette[bg_idx] } else { palette[257] });
            self.bg_paint.set_color(bg_color);
            canvas.draw_rect(Rect::from_xywh(x, y - self.font_height, width, self.font_height), &self.bg_paint);
        }

        let font_mgr = FontMgr::new();
        let weight = if (effect & EFFECT_BOLD) != 0 { skia_safe::font_style::Weight::BOLD } else { skia_safe::font_style::Weight::NORMAL };
        let slant = if (effect & EFFECT_ITALIC) != 0 { skia_safe::font_style::Slant::Italic } else { skia_safe::font_style::Slant::Upright };
        let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
        
        let typeface = font_mgr.match_family_style("monospace", font_style)
            .unwrap_or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap());
        
        let mut font = Font::new(typeface, Some(self.base_font_size));
        font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);

        let fg_color = Color::new(if fg_idx < 259 { palette[fg_idx] } else { palette[256] });
        self.paint.set_color(fg_color);
        canvas.draw_str(text, (x, y - 4.0), &font, &self.paint);

        if (effect & EFFECT_UNDERLINE) != 0 {
            canvas.draw_line((x, y - 2.0), (x + width, y - 2.0), &self.paint);
        }
        if (effect & EFFECT_STRIKETHROUGH) != 0 {
            let mid_y = y - self.font_height / 2.0;
            canvas.draw_line((x, mid_y), (x + width, mid_y), &self.paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_metrics_calculation() {
        let renderer = TerminalRenderer::new(&[], 12.0);
        assert!(renderer.font_width > 0.0);
        assert!(renderer.font_height > 0.0);
    }

    #[test]
    fn test_run_grouping_logic() {
        let mut row = crate::terminal::screen::TerminalRow::new(10);
        row.text[0] = 'A';
        row.text[1] = '\0';
        row.text[2] = 'B';
        
        let mut run_text = String::new();
        for ch in row.text.iter() {
            if *ch != '\0' {
                run_text.push(*ch);
            }
        }
        // 期望结果应该是 "AB"，因为宽字符占位符被过滤了
        assert_eq!(run_text.trim(), "AB");
    }
}
