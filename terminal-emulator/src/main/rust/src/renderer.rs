use skia_safe::{Canvas, Paint, Color, Font, Typeface, Rect, PaintStyle, FontMgr, FontStyle};
use crate::terminal::style::*;

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
        rows: i32,
        cols: i32,
        font_width: f32,
        font_height: f32,
        font_ascent: f32,
        palette: &[u32; 258],
        cursor_x: i32,
        cursor_y: i32,
        cursor_style: i32,
        cursor_visible: bool,
        mut get_row: impl FnMut(i32, &mut Vec<u32>, &mut Vec<u64>),
    ) {
        let bg_default = palette[256];
        canvas.clear(Color::new(bg_default));

        let mut code_points = Vec::with_capacity(cols as usize);
        let mut styles = Vec::with_capacity(cols as usize);

        for row in 0..rows {
            get_row(row, &mut code_points, &mut styles);
            
            let y_base = (row as f32 + 1.0) * font_height;
            let mut col = 0;
            
            while col < cols as usize {
                let cp = code_points[col];
                if cp == 0 { col += 1; continue; }

                let style = styles[col];
                let mut run_text = String::new();
                run_text.push(std::char::from_u32(cp).unwrap_or(' '));
                
                let mut run_width_cols = 1;
                let mut current = col + 1;
                
                while current < cols as usize && styles[current] == style {
                    let next_cp = code_points[current];
                    if next_cp != 0 {
                        run_text.push(std::char::from_u32(next_cp).unwrap_or(' '));
                        run_width_cols += 1;
                    }
                    current += 1;
                }

                self.draw_run(
                    canvas,
                    &run_text,
                    col as f32 * font_width,
                    y_base,
                    run_width_cols as f32 * font_width,
                    font_height,
                    font_ascent,
                    style,
                    palette,
                );

                col = current;
            }
        }

        if cursor_visible && cursor_x >= 0 && cursor_x < cols && cursor_y >= 0 && cursor_y < rows {
            self.draw_cursor(
                canvas,
                cursor_x as f32 * font_width,
                (cursor_y as f32 + 1.0) * font_height,
                font_width,
                font_height,
                palette[257],
                cursor_style,
            );
        }
    }

    fn draw_cursor(&mut self, canvas: &Canvas, x: f32, y: f32, width: f32, height: f32, color: u32, style: i32) {
        self.bg_paint.set_color(Color::new(color));
        let cursor_rect = match style {
            1 => Rect::from_xywh(x, y - height / 4.0, width, height / 4.0),
            2 => Rect::from_xywh(x, y - height, width / 4.0, height),
            _ => Rect::from_xywh(x, y - height, width, height),
        };
        canvas.draw_rect(cursor_rect, &self.bg_paint);
    }

    fn draw_run(
        &mut self,
        canvas: &Canvas,
        text: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        ascent: f32,
        style: u64,
        palette: &[u32; 258],
    ) {
        let mut fg_idx = decode_fore_color(style) as usize;
        let mut bg_idx = decode_back_color(style) as usize;
        let effect = decode_effect(style);

        if (effect & EFFECT_REVERSE) != 0 {
            std::mem::swap(&mut fg_idx, &mut bg_idx);
        }

        let fg_color = if fg_idx < 258 { palette[fg_idx] } else { palette[257] };
        let bg_color = if bg_idx < 258 { palette[bg_idx] } else { palette[256] };

        if bg_idx != 256 {
            self.bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x, y - height, width, height), &self.bg_paint);
        }

        if (effect & EFFECT_INVISIBLE) == 0 {
            let mut color = fg_color;
            if (effect & EFFECT_DIM) != 0 {
                let r = ((color >> 16) & 0xFF) * 2 / 3;
                let g = ((color >> 8) & 0xFF) * 2 / 3;
                let b = (color & 0xFF) * 2 / 3;
                color = 0xFF000000 | (r << 16) | (g << 8) | b;
            }
            self.paint.set_color(Color::new(color));
            canvas.draw_str(text, (x, y + ascent), &self.font, &self.paint);
        }
    }
}
