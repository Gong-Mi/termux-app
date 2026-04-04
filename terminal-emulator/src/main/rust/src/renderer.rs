use skia_safe::{Canvas, Paint, Color, Font, Typeface, Rect, PaintStyle, FontMgr, FontStyle};
use crate::engine::TerminalEngine;

pub struct TerminalRenderer {
    font: Font,
    paint: Paint,
    bg_paint: Paint,
}

impl TerminalRenderer {
    pub fn new(font_data: &[u8], font_size: f32) -> Self {
        let font_mgr = FontMgr::new();
        let typeface = if !font_data.is_empty() {
            font_mgr.new_from_data(font_data, None)
                .unwrap_or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap())
        } else {
            font_mgr.match_family_style("monospace", FontStyle::normal()).unwrap()
        };
        
        let mut font = Font::new(typeface, Some(font_size));
        font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);

        let mut paint = Paint::default();
        paint.set_anti_alias(true);

        let mut bg_paint = Paint::default();
        bg_paint.set_style(PaintStyle::Fill);

        Self { font, paint, bg_paint }
    }

    pub fn draw_terminal(
        &mut self,
        canvas: &Canvas,
        engine: &TerminalEngine,
        font_width: f32,
        font_height: f32,
    ) {
        // 1. 获取颜色方案
        let palette = &engine.state.colors.palette;
        let bg_default = Color::new(palette[256]);
        canvas.clear(bg_default);

        // 2. 遍历屏幕缓冲区
        let screen = &engine.screen;
        let rows = screen.get_rows();
        let cols = screen.get_cols();

        for row in 0..rows {
            let y = (row as f32 + 1.0) * font_height;
            
            // 获取该行的数据（简化：直接逐格获取）
            for col in 0..cols {
                let (code_point, style) = screen.get_cell_at(col, row);
                if code_point == 0 { continue; }

                let x = col as f32 * font_width;
                
                // 解析颜色
                let fg_idx = crate::terminal::style::decode_fore_color(style) as usize;
                let bg_idx = crate::terminal::style::decode_back_color(style) as usize;
                
                let fg_color = Color::new(if fg_idx < 258 { palette[fg_idx] } else { palette[257] });
                let bg_color = Color::new(if bg_idx < 258 { palette[bg_idx] } else { palette[256] });

                // 绘制背景
                if bg_idx != 256 {
                    self.bg_paint.set_color(bg_color);
                    canvas.draw_rect(
                        Rect::from_xywh(x, y - font_height, font_width, font_height),
                        &self.bg_paint
                    );
                }

                // 绘制文字
                self.paint.set_color(fg_color);
                let text = std::char::from_u32(code_point as u32).unwrap_or(' ').to_string();
                canvas.draw_str(&text, (x, y - 4.0), &self.font, &self.paint);
            }
        }

        // 3. 绘制光标
        let cursor = &engine.cursor;
        self.bg_paint.set_color(Color::WHITE);
        canvas.draw_rect(
            Rect::from_xywh(
                cursor.x as f32 * font_width,
                cursor.y as f32 * font_height,
                font_width,
                font_height
            ),
            &self.bg_paint
        );
    }
}
