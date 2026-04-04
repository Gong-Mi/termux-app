use skia_safe::{Canvas, Paint, Color, Font, Rect, PaintStyle, FontMgr, FontStyle};
use crate::engine::TerminalEngine;

pub struct TerminalRenderer {
    font: Font,
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

        // 精确计算字体度量
        let metrics = font.metrics().1;
        let font_height = metrics.descent - metrics.ascent + metrics.leading;
        
        // 测量字符宽度（以 'M' 或 'W' 为准）
        let (width, _) = font.measure_str("M", None);
        let font_width = width;

        let mut paint = Paint::default();
        paint.set_anti_alias(true);

        let mut bg_paint = Paint::default();
        bg_paint.set_style(PaintStyle::Fill);

        Self { font, paint, bg_paint, font_width, font_height }
    }

    pub fn draw_terminal(
        &mut self,
        canvas: &Canvas,
        engine: &TerminalEngine,
    ) {
        let state = &engine.state;
        let palette = &state.colors.current_colors;
        let screen = if state.use_alternate_buffer { &state.alt_screen } else { &state.main_screen };
        
        // 背景清屏
        let bg_default = Color::new(palette[257]);
        canvas.clear(bg_default);

        let rows = state.rows as usize;
        let cols = state.cols as usize;

        for r in 0..rows {
            if r >= screen.buffer.len() { break; }
            let row_data = &screen.buffer[r];
            let y = (r as f32 + 1.0) * self.font_height;
            
            for c in 0..cols {
                if c >= row_data.text.len() { break; }
                let char_val = row_data.text[c];
                let style = row_data.styles[c];
                
                let x = c as f32 * self.font_width;
                
                // 1. 颜色解码
                let fg_idx = crate::terminal::style::decode_fore_color(style) as usize;
                let bg_idx = crate::terminal::style::decode_back_color(style) as usize;
                let fg_color = Color::new(if fg_idx < 259 { palette[fg_idx] } else { palette[256] });
                let bg_color = Color::new(if bg_idx < 259 { palette[bg_idx] } else { palette[257] });

                // 2. 绘制背景 (如果是默认背景则跳过)
                if bg_idx != 257 {
                    self.bg_paint.set_color(bg_color);
                    canvas.draw_rect(
                        Rect::from_xywh(x, y - self.font_height, self.font_width, self.font_height),
                        &self.bg_paint
                    );
                }

                // 3. 绘制文字
                if char_val != ' ' && char_val != '\0' {
                    self.paint.set_color(fg_color);
                    canvas.draw_str(&char_val.to_string(), (x, y - 4.0), &self.font, &self.paint);
                }
            }
        }

        // 4. 绘制光标 (如果是反色块)
        if state.cursor_enabled {
            let cursor = &state.cursor;
            self.bg_paint.set_color(Color::WHITE);
            canvas.draw_rect(
                Rect::from_xywh(
                    cursor.x as f32 * self.font_width,
                    cursor.y as f32 * self.font_height,
                    self.font_width,
                    self.font_height
                ),
                &self.bg_paint
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_metrics_calculation() {
        // 创建一个测试用的渲染器（不带字体数据，使用系统默认）
        let renderer = TerminalRenderer::new(&[], 12.0);
        
        println!("Font Width: {}, Height: {}", renderer.font_width, renderer.font_height);
        
        assert!(renderer.font_width > 0.0);
        assert!(renderer.font_height > 0.0);
    }
}
