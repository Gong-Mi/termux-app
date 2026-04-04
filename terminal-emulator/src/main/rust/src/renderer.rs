use skia_safe::{Canvas, Paint, Color, Font, Rect, PaintStyle, FontMgr, FontStyle};
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
        let state = &engine.state;
        let palette = &state.colors.current_colors;
        
        // 1. 获取当前活动的缓冲区
        let screen = if state.use_alternate_buffer { &state.alt_screen } else { &state.main_screen };
        let bg_default = Color::new(palette[257]); // 注意：257 是背景索引
        canvas.clear(bg_default);

        let rows = state.rows as usize;
        let cols = state.cols as usize;

        // 2. 遍历可见区域 (Row 从 0 到 rows-1)
        for r in 0..rows {
            if r >= screen.buffer.len() { break; }
            let row_data = &screen.buffer[r];
            let y = (r as f32 + 1.0) * font_height;
            
            for c in 0..cols {
                if c >= row_data.text.len() { break; }
                let char_val = row_data.text[c];
                if char_val == ' ' || char_val == '\0' {
                    // 如果背景色不是默认背景，仍需绘制背景
                    let style = row_data.styles[c];
                    let bg_idx = crate::terminal::style::decode_back_color(style) as usize;
                    if bg_idx != 257 {
                        let bg_color = Color::new(if bg_idx < 259 { palette[bg_idx] } else { palette[257] });
                        self.bg_paint.set_color(bg_color);
                        canvas.draw_rect(
                            Rect::from_xywh(c as f32 * font_width, y - font_height, font_width, font_height),
                            &self.bg_paint
                        );
                    }
                    continue;
                }

                let x = c as f32 * font_width;
                let style = row_data.styles[c];
                
                // 解析颜色
                let fg_idx = crate::terminal::style::decode_fore_color(style) as usize;
                let bg_idx = crate::terminal::style::decode_back_color(style) as usize;
                
                let fg_color = Color::new(if fg_idx < 259 { palette[fg_idx] } else { palette[256] });
                let bg_color = Color::new(if bg_idx < 259 { palette[bg_idx] } else { palette[257] });

                // 绘制背景
                if bg_idx != 257 {
                    self.bg_paint.set_color(bg_color);
                    canvas.draw_rect(
                        Rect::from_xywh(x, y - font_height, font_width, font_height),
                        &self.bg_paint
                    );
                }

                // 绘制文字
                self.paint.set_color(fg_color);
                canvas.draw_str(&char_val.to_string(), (x, y - 4.0), &self.font, &self.paint);
            }
        }

        // 3. 绘制光标
        if state.cursor_enabled {
            let cursor = &state.cursor;
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
}
