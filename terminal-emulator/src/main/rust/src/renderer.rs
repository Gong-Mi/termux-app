use skia_safe::{Canvas, Paint, Color, Font, Rect, PaintStyle, FontMgr, FontStyle, BlendMode};
use crate::engine::TerminalEngine;
use crate::terminal::style::*;
use crate::terminal::colors::{COLOR_INDEX_CURSOR, NUM_INDEXED_COLORS};

use crate::render_thread;

/// 预计算的渲染帧数据 - 用于异步渲染（不需要持有 engine 锁）
#[derive(Clone)]
pub struct RenderFrame {
    pub rows: usize,
    pub cols: usize,
    pub palette: [u32; NUM_INDEXED_COLORS],
    pub use_alternate_buffer: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub cursor_style: i32,
    pub cursor_enabled: bool,
    pub reverse_video: bool,
    pub top_row: i32,
    /// 预计算的行数据: (text: String, styles: Vec<u64>)
    pub row_data: Vec<(Vec<char>, Vec<u64>)>,
}

impl RenderFrame {
    /// 从 engine 快照创建 RenderFrame（快速复制，<1ms）
    pub fn from_engine(
        engine: &crate::engine::TerminalEngine,
        rows: usize,
        cols: usize,
        top_row: i32,
    ) -> Self {
        let state = &engine.state;
        let screen = if state.use_alternate_buffer { &state.alt_screen } else { &state.main_screen };

        let mut row_data = Vec::with_capacity(rows);
        for r in top_row..(top_row + rows as i32) {
            let row = screen.get_row(r);
            row_data.push((row.text.clone(), row.styles.clone()));
        }

        Self {
            rows,
            cols,
            palette: state.colors.current_colors,
            use_alternate_buffer: state.use_alternate_buffer,
            cursor_x: state.cursor.x,
            cursor_y: state.cursor.y,
            cursor_style: state.cursor.style,
            cursor_enabled: state.cursor_enabled,
            reverse_video: state.modes.is_enabled(crate::terminal::modes::DECSET_BIT_REVERSE_VIDEO),
            top_row,
            row_data,
        }
    }
}

/// Unicode 字符终端单元格宽度计算 (与 Java WcWidth 一致)
#[inline]
fn char_wc_width(ucs: u32) -> usize {
    if ucs == 0 || ucs == 32 { return 1; }
    if ucs < 32 || (ucs >= 0x7F && ucs < 0xA0) { return 0; }
    if (ucs >= 0x2E80 && ucs <= 0x9FFF) ||
       (ucs >= 0xAC00 && ucs <= 0xD7A3) ||
       (ucs >= 0xFF01 && ucs <= 0xFF60) { return 2; }
    1
}

/// 预计算的字体和指标，避免每帧重建
struct FontCache {
    font_mono: Font,
    font_bold: Font,
    font_italic: Font,
    font_bold_italic: Font,
    font_fallback: Font,
    font_fallback_bold: Font,
    font_width: f32,
    font_height: f32,
    font_ascent: f32,
}

impl FontCache {
    fn new(font_size: f32) -> Self {
        let font_mgr = FontMgr::new();

        let tf_mono = font_mgr.match_family_style("monospace", FontStyle::normal())
            .expect("monospace font");
        let tf_bold = font_mgr.match_family_style("monospace",
            FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright))
            .unwrap_or_else(|| tf_mono.clone());
        let tf_italic = font_mgr.match_family_style("monospace",
            FontStyle::new(skia_safe::font_style::Weight::NORMAL, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Italic))
            .unwrap_or_else(|| tf_mono.clone());
        let tf_bold_italic = font_mgr.match_family_style("monospace",
            FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Italic))
            .unwrap_or_else(|| tf_mono.clone());
        let tf_fallback = font_mgr.match_family_style("sans-serif", FontStyle::normal())
            .unwrap_or_else(|| tf_mono.clone());
        let tf_fallback_bold = font_mgr.match_family_style("sans-serif",
            FontStyle::new(skia_safe::font_style::Weight::BOLD, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright))
            .unwrap_or_else(|| tf_mono.clone());

        let mut font_mono = Font::new(tf_mono.clone(), Some(font_size));
        font_mono.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
        font_mono.set_subpixel(true);

        let metrics = font_mono.metrics();
        let font_height = (metrics.1.descent - metrics.1.ascent + metrics.1.leading).ceil();
        let (w, _) = font_mono.measure_str("M", None);
        let font_width = w;

        // 构建各变体字体
        let mut build_font = |tf: &skia_safe::Typeface| {
            let mut f = Font::new(tf.clone(), Some(font_size));
            f.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
            f.set_subpixel(true);
            f
        };

        Self {
            font_mono,
            font_bold: build_font(&tf_bold),
            font_italic: build_font(&tf_italic),
            font_bold_italic: build_font(&tf_bold_italic),
            font_fallback: build_font(&tf_fallback),
            font_fallback_bold: build_font(&tf_fallback_bold),
            font_width,
            font_height,
            font_ascent: metrics.1.ascent,
        }
    }

    fn get_font(&self, bold: bool, italic: bool, has_non_ascii: bool) -> &Font {
        match (has_non_ascii, bold, italic) {
            (false, false, false) => &self.font_mono,
            (false, true, false) => &self.font_bold,
            (false, false, true) => &self.font_italic,
            (false, true, true) => &self.font_bold_italic,
            (true, false, _) => &self.font_fallback,
            (true, true, _) => &self.font_fallback_bold,
        }
    }
}

/// ASCII 字符宽度缓存（避免重复 measure_str）
struct AsciiWidthCache {
    widths: [f32; 128],
}

impl AsciiWidthCache {
    fn new(font: &Font) -> Self {
        let mut widths = [0.0f32; 128];
        for i in 32u8..127 {
            let ch = i as u8 as char;
            let (w, _) = font.measure_str(&ch.to_string(), None);
            widths[i as usize] = w;
        }
        Self { widths }
    }

    #[inline]
    fn get(&self, ch: char) -> Option<f32> {
        if (ch as u32) < 128 { Some(self.widths[ch as usize]) } else { None }
    }
}

/// 选区坐标（屏幕缓冲区坐标）
#[derive(Clone, Copy, Default)]
pub struct SelectionBounds {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub active: bool,
}

/// 非 ASCII 字符宽度缓存 (LRU 风格，常用 CJK/Emoji 字符)
struct NonAsciiWidthCache {
    entries: [(u32, f32); 64],
    mask: usize,
}

impl NonAsciiWidthCache {
    fn new() -> Self {
        Self {
            entries: [(0, 0.0); 64],
            mask: 63,
        }
    }

    fn get(&self, ch: u32) -> Option<f32> {
        let idx = (ch as usize) & self.mask;
        let (key, val) = self.entries[idx];
        if key == ch { Some(val) } else { None }
    }

    fn insert(&mut self, ch: u32, w: f32) {
        let idx = (ch as usize) & self.mask;
        self.entries[idx] = (ch, w);
    }
}

pub struct TerminalRenderer {
    pub font_size: f32,
    font_cache: FontCache,
    ascii_cache: AsciiWidthCache,
    non_ascii_cache: NonAsciiWidthCache,
    paint: Paint,
    bg_paint: Paint,
    underline_paint: Paint,
    strikethrough_paint: Paint,
    selection_bg_paint: Paint,
    cursor_paint: Paint,
    /// 复用 run 缓冲区，避免每帧分配
    run_buf: String,
    pub font_width: f32,
    pub font_height: f32,
    pub selection: SelectionBounds,
}

impl TerminalRenderer {
    pub fn new(_font_data: &[u8], font_size: f32) -> Self {
        let font_cache = FontCache::new(font_size);
        let ascii_cache = AsciiWidthCache::new(&font_cache.font_mono);
        let font_width = font_cache.font_width;
        let font_height = font_cache.font_height;

        // 主文本绘制
        let mut paint = Paint::default();
        paint.set_anti_alias(true); // 保持抗锯齿以确保文字边缘清晰
        paint.set_blend_mode(BlendMode::SrcOver);

        // 背景矩形填充
        let mut bg_paint = Paint::default();
        bg_paint.set_style(PaintStyle::Fill);

        // 下划线绘制
        let mut underline_paint = Paint::default();
        underline_paint.set_anti_alias(false);
        underline_paint.set_stroke_width(1.0);

        // 删除线绘制
        let mut strikethrough_paint = Paint::default();
        strikethrough_paint.set_anti_alias(false);
        strikethrough_paint.set_stroke_width(1.0);

        // 选区高亮背景
        let mut selection_bg_paint = Paint::default();
        selection_bg_paint.set_style(PaintStyle::Fill);
        selection_bg_paint.set_blend_mode(BlendMode::SrcOver);

        // 光标绘制
        let mut cursor_paint = Paint::default();
        cursor_paint.set_style(PaintStyle::Fill);

        Self {
            font_size,
            font_cache,
            ascii_cache,
            non_ascii_cache: NonAsciiWidthCache::new(),
            paint,
            bg_paint,
            underline_paint,
            strikethrough_paint,
            selection_bg_paint,
            cursor_paint,
            font_width,
            font_height,
            run_buf: String::with_capacity(256),
            selection: SelectionBounds::default(),
        }
    }

    /// 从 Java 侧设置选区坐标
    pub fn set_selection(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.selection = SelectionBounds { x1, y1, x2, y2, active: true };
    }

    pub fn clear_selection(&mut self) {
        self.selection.active = false;
    }

    /// 判断给定的可见屏幕行列是否在选区内 (对齐 Upstream 逻辑)
    #[inline]
    pub fn is_cell_selected(&self, col: i32, row: i32) -> bool {
        if !self.selection.active { return false; }
        let s = &self.selection;
        
        // 确保 (sy, sx) 是起点，(ey, ex) 是终点
        let (sy, sx, ey, ex) = if s.y1 < s.y2 || (s.y1 == s.y2 && s.x1 <= s.x2) {
            (s.y1, s.x1, s.y2, s.x2)
        } else {
            (s.y2, s.x2, s.y1, s.x1)
        };

        if row < sy || row > ey { return false; }
        
        if row == sy && row == ey {
            return col >= sx && col <= ex;
        }
        
        if row == sy {
            return col >= sx;
        }
        
        if row == ey {
            return col <= ex;
        }
        
        true // 中间行全选
    }

    #[inline]
    fn apply_dim(color: u32) -> u32 {
        // 2/3 亮度淡化（与 Java 一致）
        let r = (((color >> 16) & 0xFF) as u32 * 2 / 3).min(255);
        let g = (((color >> 8) & 0xFF) as u32 * 2 / 3).min(255);
        let b = ((color & 0xFF) as u32 * 2 / 3).min(255);
        (color & 0xFF000000) | (r << 16) | (g << 8) | b
    }

    #[inline]
    fn reverse_colors(fg: usize, bg: usize) -> (usize, usize) {
        (bg, fg)
    }

    pub fn draw_terminal(
        &mut self,
        canvas: &Canvas,
        engine: &TerminalEngine,
        scale: f32,
        scroll_offset: f32,
    ) {
        let state = &engine.state;
        let palette = &state.colors.current_colors;
        let screen = if state.use_alternate_buffer { &state.alt_screen } else { &state.main_screen };

        canvas.save();
        canvas.scale((scale, scale));

        // 背景清屏
        let bg_color = palette[257];
        canvas.clear(Color::new(bg_color));

        canvas.translate((0.0, -scroll_offset));

        let rows = state.rows as usize;
        let cols = state.cols as usize;
        let global_reverse = state.modes.is_enabled(crate::terminal::modes::DECSET_BIT_REVERSE_VIDEO);
        let top_row = *render_thread::get_render_top_row().lock().unwrap();

        // 先绘制文本行 - 使用 get_row() 处理环形缓冲区映射
        for r in 0..rows as i32 {
            let absolute_row = top_row + r;
            let row_data = screen.get_row(r);
            let y_base = (r as f32 + 1.0) * self.font_height;

            let mut c = 0;
            while c < cols {
                if c >= row_data.text.len() { break; }
                let start_c = c;
                let style = row_data.styles[c];
                let effect = decode_effect(style);

                // 不可见文本跳过
                if (effect & EFFECT_INVISIBLE) != 0 {
                    let ch = row_data.text[c];
                    c += if ch == '\0' { 1 } else { char_wc_width(ch as u32) };
                    continue;
                }

                // 复用 run 缓冲区 (clear 但保留容量)
                self.run_buf.clear();
                let mut run_cells = 0usize;
                let mut run_measured = 0.0f32;
                let mut run_has_non_ascii = false;

                // 合并相同样式 + 相同选区状态的 run
                let sel = self.is_cell_selected(c as i32, absolute_row);
                while c < cols && c < row_data.text.len() {
                    let cell_style = row_data.styles[c];
                    let cell_effect = decode_effect(cell_style);

                    // 不可见单元格跳过
                    if (cell_effect & EFFECT_INVISIBLE) != 0 {
                        let ch = row_data.text[c];
                        c += if ch == '\0' { 1 } else { char_wc_width(ch as u32) };
                        continue;
                    }

                    let cell_sel = self.is_cell_selected(c as i32, absolute_row);
                    let style_match = cell_style == style;
                    let sel_match = cell_sel == sel;

                    // 核心修复：宽字符占位符 \0 必须跟随其前导字符，即使样式不匹配也不应断开 run
                    // 否则会导致渲染列偏移，出现颜色与字符对不上的现象
                    let is_placeholder = row_data.text[c] == '\0';

                    if (style_match && sel_match) || is_placeholder {
                        let ch = row_data.text[c];
                        if ch != '\0' {
                            self.run_buf.push(ch);
                            let wc_w = char_wc_width(ch as u32);
                            run_cells += wc_w;
                            if ch as u32 > 127 { run_has_non_ascii = true; }
                            // 像素宽度计算 - 优先缓存
                            if let Some(w) = self.ascii_cache.get(ch) {
                                run_measured += w;
                            } else if let Some(w) = self.non_ascii_cache.get(ch as u32) {
                                run_measured += w;
                            } else {
                                let w = self.measure_char(ch, cell_effect);
                                self.non_ascii_cache.insert(ch as u32, w);
                                run_measured += w;
                            }
                        }
                        // 移动到下一个单元格
                        c += 1;
                    } else {
                        break;
                    }
                }

                if !self.run_buf.is_empty() {
                    // 期望像素宽度 = 单元格数 * 单格宽度 (与 Java canvas.scale 一致)
                    let expected_width = run_cells as f32 * self.font_width;
                    // Clone to avoid borrow conflict with &mut self
                    let run_text = self.run_buf.clone();

                    self.draw_run_opt(
                        canvas,
                        &run_text,
                        start_c as f32 * self.font_width,
                        y_base,
                        expected_width,
                        run_measured,
                        run_has_non_ascii,
                        style,
                        palette,
                        global_reverse,
                        sel,
                        r as i32,
                    );
                }
            }
        }

        // 绘制光标
        if state.cursor_enabled {
            let cursor = &state.cursor;
            if cursor.should_be_visible(state.cursor_enabled) {
                let cursor_color = palette[COLOR_INDEX_CURSOR];
                self.cursor_paint.set_color(Color::new(cursor_color));

                let cx = cursor.x as f32 * self.font_width;
                let cy = cursor.y as f32 * self.font_height;

                match cursor.style {
                    0 => {
                        // Block cursor
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint
                        );
                    }
                    1 => {
                        // Underline cursor (底部 2 像素)
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy + self.font_height - 2.0, self.font_width, 2.0),
                            &self.cursor_paint
                        );
                    }
                    2 => {
                        // Bar cursor (左侧 2 像素宽竖线)
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, 2.0, self.font_height),
                            &self.cursor_paint
                        );
                    }
                    _ => {
                        // 默认 block
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint
                        );
                    }
                }
            }
        }

        canvas.restore();
    }

    /// 异步渲染 - 使用预计算的 RenderFrame，完全不需要 engine 锁
    pub fn draw_frame(
        &mut self,
        canvas: &Canvas,
        frame: &RenderFrame,
        scale: f32,
        _scroll_offset: f32,
    ) {
        let palette = &frame.palette;

        canvas.save();
        canvas.scale((scale, scale));

        // 背景清屏
        let bg_color = palette[257];
        canvas.clear(Color::new(bg_color));

        // canvas.translate((0.0, -scroll_offset)); // 不再使用 translate，因为我们已经截取了正确的可见行

        let rows = frame.rows;
        let cols = frame.cols;
        let global_reverse = frame.reverse_video;
        let top_row = frame.top_row;

        // 先绘制文本行 - 使用预计算的数据，不需要任何锁
        for r in 0..rows as i32 {
            let absolute_row = top_row + r;
            let row = &frame.row_data[r as usize];
            let row_text = &row.0;
            let row_styles = &row.1;
            let y_base = (r as f32 + 1.0) * self.font_height;

            let mut c = 0;
            while c < cols {
                if c >= row_text.len() { break; }
                let start_c = c;
                let style = row_styles[c];
                let effect = decode_effect(style);

                // 不可见文本跳过
                if (effect & EFFECT_INVISIBLE) != 0 {
                    let ch = row_text[c];
                    c += if ch == '\0' { 1 } else { char_wc_width(ch as u32) };
                    continue;
                }

                // 复用 run 缓冲区
                self.run_buf.clear();
                let mut run_cells = 0usize;
                let mut run_measured = 0.0f32;
                let mut run_has_non_ascii = false;

                // 合并相同样式 + 相同选区状态的 run
                let sel = self.is_cell_selected(c as i32, absolute_row);
                while c < cols && c < row_text.len() {
                    let cell_style = row_styles[c];
                    let cell_effect = decode_effect(cell_style);

                    if (cell_effect & EFFECT_INVISIBLE) != 0 {
                        let ch = row_text[c];
                        c += if ch == '\0' { 1 } else { char_wc_width(ch as u32) };
                        continue;
                    }

                    let cell_sel = self.is_cell_selected(c as i32, absolute_row);
                    let style_match = cell_style == style;
                    let sel_match = cell_sel == sel;

                    // 核心修复：宽字符占位符 \0 必须跟随其前导字符
                    let is_placeholder = row_text[c] == '\0';

                    if (style_match && sel_match) || is_placeholder {
                        let ch = row_text[c];
                        if ch != '\0' {
                            self.run_buf.push(ch);
                            let wc_w = char_wc_width(ch as u32);
                            run_cells += wc_w;
                            if ch as u32 > 127 { run_has_non_ascii = true; }
                            if let Some(w) = self.ascii_cache.get(ch) {
                                run_measured += w;
                            } else if let Some(w) = self.non_ascii_cache.get(ch as u32) {
                                run_measured += w;
                            } else {
                                let w = self.measure_char(ch, cell_effect);
                                self.non_ascii_cache.insert(ch as u32, w);
                                run_measured += w;
                            }
                        }
                        c += 1;
                    } else {
                        break;
                    }
                }

                if !self.run_buf.is_empty() {
                    let expected_width = run_cells as f32 * self.font_width;
                    let run_text = self.run_buf.clone();

                    self.draw_run_opt(
                        canvas,
                        &run_text,
                        start_c as f32 * self.font_width,
                        y_base,
                        expected_width,
                        run_measured,
                        run_has_non_ascii,
                        style,
                        palette,
                        global_reverse,
                        sel,
                        r as i32,
                    );
                }
            }
        }

        // 绘制光标
        if frame.cursor_enabled {
            let cursor_color = palette[COLOR_INDEX_CURSOR];
            self.cursor_paint.set_color(Color::new(cursor_color));

            let cx = frame.cursor_x as f32 * self.font_width;
            let cy = frame.cursor_y as f32 * self.font_height;

            match frame.cursor_style {
                0 => {
                    canvas.draw_rect(
                        Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                        &self.cursor_paint
                    );
                }
                1 => {
                    canvas.draw_rect(
                        Rect::from_xywh(cx, cy + self.font_height - 2.0, self.font_width, 2.0),
                        &self.cursor_paint
                    );
                }
                2 => {
                    canvas.draw_rect(
                        Rect::from_xywh(cx, cy, 2.0, self.font_height),
                        &self.cursor_paint
                    );
                }
                _ => {
                    canvas.draw_rect(
                        Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                        &self.cursor_paint
                    );
                }
            }
        }

        canvas.restore();
    }

    fn draw_run_opt(
        &mut self,
        canvas: &Canvas,
        text: &str,
        x: f32,
        y_base: f32,
        expected_width: f32,
        measured_width: f32,
        has_non_ascii: bool,
        style: u64,
        palette: &[u32; 259],
        global_reverse: bool,
        is_selected: bool,
        _row: i32,
    ) {
        let mut fg_idx = decode_fore_color(style) as usize;
        let mut bg_idx = decode_back_color(style) as usize;
        let effect = decode_effect(style);

        // Bold→Bright 颜色映射 (与 Java TerminalRenderer.java:230 一致)
        // 前景色在 0-7 范围且加粗时，映射到 8-15 亮色
        let bold = (effect & EFFECT_BOLD) != 0;
        if bold && fg_idx < 8 {
            fg_idx += 8;
        }

        // 选区特效标准化 (对齐 Upstream): 选区通过反色实现
        // 最终是否反色 = (全局反色 ^ 字符反色 ^ 是否被选中)
        let mut do_reverse = global_reverse != ((effect & EFFECT_REVERSE) != 0);
        if is_selected {
            do_reverse = !do_reverse;
        }

        if do_reverse {
            let swapped = Self::reverse_colors(fg_idx, bg_idx);
            fg_idx = swapped.0;
            bg_idx = swapped.1;
        }

        // Dim 效果
        let mut fg_color_val = if fg_idx < 259 { palette[fg_idx] } else { palette[256] };
        if (effect & EFFECT_DIM) != 0 {
            fg_color_val = Self::apply_dim(fg_color_val);
        }

        // 背景绘制 - 标准化后移除蓝色背景，完全依赖反色
        let bg_color_val = if bg_idx < 259 { palette[bg_idx] } else { palette[257] };
        if bg_idx != 257 {
            self.bg_paint.set_color(Color::new(bg_color_val));
            canvas.draw_rect(Rect::from_xywh(x, y_base - self.font_height, expected_width, self.font_height), &self.bg_paint);
        }

        // 字体选择 - has_non_ascii 已由调用方预计算
        let italic = (effect & EFFECT_ITALIC) != 0;
        let font = self.font_cache.get_font(bold, italic, has_non_ascii);

        let fg_color = Color::new(fg_color_val);
        self.paint.set_color(fg_color);

        // 文本绘制 - 与 Java 一致：如果测量宽度与期望宽度不同，使用 canvas.scale 缩放
        let y_adjusted = y_base + self.font_cache.font_ascent * 0.15;
        if measured_width > 0.0 && (expected_width - measured_width).abs() > 0.5 {
            canvas.save();
            canvas.scale((expected_width / measured_width, 1.0));
            let x_scaled = x / (expected_width / measured_width);
            canvas.draw_str(text, (x_scaled, y_adjusted), font, &self.paint);
            canvas.restore();
        } else {
            canvas.draw_str(text, (x, y_adjusted), font, &self.paint);
        }

        // 下划线
        if (effect & EFFECT_UNDERLINE) != 0 {
            let underline_y = y_base - 2.0;
            self.underline_paint.set_color(fg_color);
            canvas.draw_line((x, underline_y), (x + expected_width, underline_y), &self.underline_paint);
        }

        // 删除线
        if (effect & EFFECT_STRIKETHROUGH) != 0 {
            let strike_y = y_base - self.font_height * 0.5;
            self.strikethrough_paint.set_color(fg_color);
            canvas.draw_line((x, strike_y), (x + expected_width, strike_y), &self.strikethrough_paint);
        }
    }

    /// 测量单个字符的像素宽度（使用缓存字体，避免重复创建 Font）
    #[inline]
    fn measure_char(&self, ch: char, effect: u64) -> f32 {
        let has_non_ascii = ch as u32 > 127;
        let bold = (effect & EFFECT_BOLD) != 0;
        let italic = (effect & EFFECT_ITALIC) != 0;
        let font = self.font_cache.get_font(bold, italic, has_non_ascii);
        // 直接使用预创建的 Font，避免临时分配
        let (w, _) = font.measure_str(&ch.to_string(), None);
        w
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
    fn test_dim_color() {
        let white = 0xffffffff;
        let dimmed = TerminalRenderer::apply_dim(white);
        assert_eq!((dimmed >> 16) & 0xFF, 170); // 255 * 2/3 = 170
        assert_eq!((dimmed >> 8) & 0xFF, 170);
        assert_eq!(dimmed & 0xFF, 170);
    }

    #[test]
    fn test_selection_bounds() {
        let mut renderer = TerminalRenderer::new(&[], 12.0);
        renderer.set_selection(2, 1, 5, 3);
        assert!(renderer.is_cell_selected(3, 2));
        assert!(renderer.is_cell_selected(2, 1));
        assert!(!renderer.is_cell_selected(0, 0));
        assert!(!renderer.is_cell_selected(6, 3));
    }

    #[test]
    fn test_selection_reversed() {
        let mut renderer = TerminalRenderer::new(&[], 12.0);
        renderer.set_selection(5, 3, 2, 1); // 反向设置
        assert!(renderer.is_cell_selected(3, 2));
        assert!(renderer.is_cell_selected(2, 1));
    }

    #[test]
    fn test_selection_with_7_colors() {
        // 定义 7 种不同的随机颜色索引 (模拟 ANSI 颜色 1-7)
        let colors = [1, 2, 3, 4, 5, 6, 7];
        let global_reverse = false;
        let is_selected = true;

        for bg_idx in colors {
            let fg_idx = 0; // 默认前景黑色
            let effect = 0u64; // 无特效

            // 模拟 draw_run_opt 中的反色逻辑
            let mut current_fg = fg_idx;
            let mut current_bg = bg_idx;
            
            // 最终是否反色 = (全局反色 ^ 字符反色 ^ 是否被选中)
            let mut do_reverse = global_reverse != ((effect & crate::terminal::style::EFFECT_REVERSE) != 0);
            if is_selected {
                do_reverse = !do_reverse;
            }

            if do_reverse {
                let (new_fg, new_bg) = TerminalRenderer::reverse_colors(current_fg, current_bg);
                current_fg = new_fg;
                current_bg = new_bg;
            }

            // 验证：在选中状态下且无其他反色标记时，颜色应该被反转
            assert_eq!(current_fg, bg_idx, "Foreground should be reversed to background color for index {}", bg_idx);
            assert_eq!(current_bg, fg_idx, "Background should be reversed to foreground color for index {}", bg_idx);
        }
    }
}
