use skia_safe::{Canvas, Paint, Color, Font, Rect, PaintStyle, FontMgr, FontStyle, TextBlob, TextBlobBuilder};
use std::sync::Arc;
use std::collections::HashMap;
use crate::terminal::style::*;
use crate::terminal::colors::{COLOR_INDEX_CURSOR, NUM_INDEXED_COLORS};

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
    /// 预计算的行数据: (text: Vec<char>, styles: Vec<u64>, hash: u64)
    pub row_data: Vec<(Vec<char>, Vec<u64>, u64)>,
}

impl RenderFrame {
    /// 计算行的哈希值，用于增量渲染判断
    fn hash_row(text: &[char], styles: &[u64]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        styles.hash(&mut hasher);
        hasher.finish()
    }

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
        let start_row = -(screen.active_transcript_rows as i32);
        let end_row = screen.rows as i32;

        for r in top_row..(top_row + rows as i32) {
            if r >= start_row && r < end_row {
                let row = screen.get_row(r);
                let h = Self::hash_row(&row.text, &row.styles);
                row_data.push((row.text.clone(), row.styles.clone(), h));
            } else {
                let text = vec![' '; cols];
                let styles = vec![crate::terminal::style::STYLE_NORMAL; cols];
                let h = Self::hash_row(&text, &styles);
                row_data.push((text, styles, h));
            }
        }

        Self {
            rows,
            cols,
            palette: state.colors.current_colors,
            use_alternate_buffer: state.use_alternate_buffer,
            cursor_x: state.cursor.x,
            cursor_y: state.cursor.y,
            cursor_style: state.cursor.style,
            cursor_enabled: state.cursor.should_be_visible(state.cursor_enabled),
            reverse_video: state.modes.is_enabled(crate::terminal::modes::DECSET_BIT_REVERSE_VIDEO),
            top_row,
            row_data,
        }
    }
}

/// Unicode 字符终端单元格宽度计算
#[inline]
fn char_wc_width(ucs: u32) -> usize {
    crate::wcwidth::wcwidth(ucs)
}

/// 判断字符是否为块元素
#[inline]
pub fn is_block_element(ch: char) -> bool {
    matches!(ch as u32, 0x2580..=0x259F | 0x2500..=0x257F)
}

/// 判断字符是否需要特殊渲染
#[inline]
pub fn is_special_render_char(ch: char) -> bool {
    is_block_element(ch) || matches!(ch as u32, 0x2800..=0x28FF)
}

/// 预计算的字体和指标
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
    font_mgr: Arc<FontMgr>,
    /// 动态备用字体缓存：存储由系统匹配到的特定 Unicode 字符字体
    dynamic_fonts: std::sync::RwLock<std::collections::HashMap<u32, Font>>,
}

unsafe impl Send for FontCache {}
unsafe impl Sync for FontCache {}

impl FontCache {
    fn new(font_size: f32, custom_font_path: Option<&str>) -> Self {
        let font_mgr = Arc::new(FontMgr::new());
        let custom_typeface = custom_font_path.and_then(|path| {
            std::fs::read(path).ok().and_then(|data| {
                let font_data = skia_safe::Data::new_copy(&data);
                font_mgr.new_from_data(&font_data, 0)
            })
        });

        let tf_mono = custom_typeface.clone().or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal())).expect("monospace font");
        let tf_bold = custom_typeface.as_ref().map(|tf| tf.clone()).or_else(|| font_mgr.match_family_style("monospace", FontStyle::bold())).unwrap_or_else(|| tf_mono.clone());
        let tf_italic = font_mgr.match_family_style("monospace", FontStyle::italic()).unwrap_or_else(|| tf_mono.clone());
        let tf_bold_italic = font_mgr.match_family_style("monospace", FontStyle::bold_italic()).unwrap_or_else(|| tf_mono.clone());
        let tf_fallback = custom_typeface.clone().or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::normal())).unwrap_or_else(|| tf_mono.clone());
        let tf_fallback_bold = custom_typeface.clone().or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::bold())).unwrap_or_else(|| tf_mono.clone());

        let build_font = |tf: &skia_safe::Typeface| {
            let mut f = Font::new(tf.clone(), Some(font_size));
            f.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
            f.set_subpixel(true);
            f
        };

        let font_mono = build_font(&tf_mono);
        let metrics = font_mono.metrics();
        let font_height = (metrics.1.descent - metrics.1.ascent + metrics.1.leading).ceil();
        let (w, _) = font_mono.measure_str("M", None);

        Self {
            font_mono,
            font_bold: build_font(&tf_bold),
            font_italic: build_font(&tf_italic),
            font_bold_italic: build_font(&tf_bold_italic),
            font_fallback: build_font(&tf_fallback),
            font_fallback_bold: build_font(&tf_fallback_bold),
            font_width: w,
            font_height,
            font_ascent: metrics.1.ascent,
            font_mgr,
            dynamic_fonts: std::sync::RwLock::new(std::collections::HashMap::with_capacity(64)),
        }
    }

    fn get_font(&self, bold: bool, italic: bool, _has_non_ascii: bool) -> &Font {
        match (bold, italic) {
            (false, false) => &self.font_mono,
            (true, false) => &self.font_bold,
            (false, true) => &self.font_italic,
            (true, true) => &self.font_bold_italic,
        }
    }

    /// 通过字体类型索引获取字体引用（0=mono, 1=bold, 2=italic, 3=bold_italic, 4=fallback, 5=fallback_bold）
    fn get_font_by_index(&self, idx: usize) -> &Font {
        match idx {
            0 => &self.font_mono,
            1 => &self.font_bold,
            2 => &self.font_italic,
            3 => &self.font_bold_italic,
            4 => &self.font_fallback,
            5 => &self.font_fallback_bold,
            _ => &self.font_mono,
        }
    }

    /// 获取字体的 typeface ID
    fn get_typeface_id(&self, idx: usize) -> u32 {
        self.get_font_by_index(idx).typeface().unique_id()
    }

    /// 快速字体查找：返回 (font_type_index, is_fallback)
    fn get_font_type_for_char(&self, ch: char, bold: bool, italic: bool) -> (usize, bool) {
        let ucs = ch as u32;
        let primary_type = match (bold, italic) {
            (false, false) => 0, // Mono
            (true, false) => 1,  // Bold
            (false, true) => 2,  // Italic
            (true, true) => 3,   // BoldItalic
        };

        let primary_font = self.get_font_by_index(primary_type);
        let mut glyphs = [0u16; 1];
        primary_font.typeface().unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if glyphs[0] != 0 { return (primary_type, false); }

        // 尝试 fallback
        let fallback_type = if bold { 5 } else { 4 }; // FallbackBold or Fallback
        let fallback_font = self.get_font_by_index(fallback_type);
        fallback_font.typeface().unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if glyphs[0] != 0 { return (fallback_type, false); }

        // 需要动态匹配（这种情况较少，标记为 fallback）
        (fallback_type, true)
    }

    fn get_font_for_char(&self, ch: char, bold: bool, italic: bool) -> (Font, bool) {
        let ucs = ch as u32;
        
        // 1. 检查主字体
        let primary = self.get_font(bold, italic, false);
        let mut glyphs = [0u16; 1];
        primary.typeface().unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if glyphs[0] != 0 { return (primary.clone(), false); }

        // 2. 检查动态缓存
        {
            let cache = self.dynamic_fonts.read().unwrap();
            if let Some(font) = cache.get(&ucs) {
                return (font.clone(), true);
            }
        }

        // 3. 检查基础 fallback (sans-serif)
        let fallback = if bold { &self.font_fallback_bold } else { &self.font_fallback };
        fallback.typeface().unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if glyphs[0] != 0 { return (fallback.clone(), true); }

        // 4. 系统动态匹配 (核心：不再硬编码名字)
        let style = if bold {
            if italic { FontStyle::bold_italic() } else { FontStyle::bold() }
        } else {
            if italic { FontStyle::italic() } else { FontStyle::normal() }
        };
        
        if let Some(tf) = self.font_mgr.match_family_style_character("monospace", style, &[], ucs as i32) {
            let mut matched_font = Font::new(tf, Some(self.font_mono.size()));
            matched_font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
            matched_font.set_subpixel(true);
            
            // 写入缓存
            let mut cache = self.dynamic_fonts.write().unwrap();
            if cache.len() > 500 { cache.clear(); }
            cache.insert(ucs, matched_font.clone());
            return (matched_font, true);
        }

        // 5. 最终降级
        (fallback.clone(), true)
    }
}

/// 行缓存项
struct RowCacheEntry {
    hash: u64,
    picture: skia_safe::Picture,
    palette_hash: u64,
    selection_hash: u64,
}

/// 字形缓存
struct GlyphCache {
    ascii: [[u16; 128]; 4],
    map: std::collections::HashMap<u64, u16>,
    typeface_ids: [u32; 4],
}

impl GlyphCache {
    fn new() -> Self {
        Self {
            ascii: [[0u16; 128]; 4],
            map: std::collections::HashMap::with_capacity(2048),
            typeface_ids: [0; 4],
        }
    }

    #[inline]
    fn get_glyph(&mut self, font: &Font, ch: char) -> u16 {
        let ucs = ch as u32;
        let tf = font.typeface();
        let tf_id = tf.unique_id();

        if ucs < 128 {
            for i in 0..4 {
                if self.typeface_ids[i] == tf_id {
                    let g = self.ascii[i][ucs as usize];
                    if g != 0 { return g; }
                    let mut glyphs = [0u16; 1];
                    tf.unichars_to_glyphs(&[ucs as i32], &mut glyphs);
                    self.ascii[i][ucs as usize] = glyphs[0];
                    return glyphs[0];
                }
            }
        }

        let key = ((tf_id as u64) << 32) | (ucs as u64);
        if let Some(&g) = self.map.get(&key) { return g; }
        let mut glyphs = [0u16; 1];
        tf.unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if self.map.len() > 8192 { self.map.clear(); }
        self.map.insert(key, glyphs[0]);
        glyphs[0]
    }

    fn update_typeface_ids(&mut self, cache: &FontCache) {
        self.typeface_ids[0] = cache.font_mono.typeface().unique_id();
        self.typeface_ids[1] = cache.font_bold.typeface().unique_id();
        self.typeface_ids[2] = cache.font_italic.typeface().unique_id();
        self.typeface_ids[3] = cache.font_bold_italic.typeface().unique_id();
    }
}

/// 选区坐标
#[derive(Clone, Copy, Default)]
pub struct SelectionBounds {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
    pub active: bool,
}

pub struct TerminalRenderer {
    pub font_size: f32,
    pub font_path: Option<String>,
    font_cache: FontCache,
    paint: Paint,
    bg_paint: Paint,
    underline_paint: Paint,
    strikethrough_paint: Paint,
    cursor_paint: Paint,
    run_buf: String,
    pub font_width: f32,
    pub font_height: f32,
    pub selection: SelectionBounds,
    row_cache: Vec<Option<RowCacheEntry>>,
    recorder: skia_safe::PictureRecorder,
    glyph_cache: GlyphCache,
    // === 优化：预分配缓冲区，避免热路径中的分配 ===
    text_blob_builder: TextBlobBuilder,           // 重用 TextBlobBuilder
    group_chars_buf: Vec<(char, f32)>,           // 字符分组缓冲区
    row_selection_buf: Vec<bool>,                // 选区缓冲区
    // === TextBlob 缓存：避免每帧重新创建相同的 blob ===
    text_blob_cache: HashMap<u64, TextBlob>,     // (hash, style) -> TextBlob
}

unsafe impl Send for TerminalRenderer {}
unsafe impl Sync for TerminalRenderer {}

impl TerminalRenderer {
    pub fn new(_font_data: &[u8], font_size: f32, custom_font_path: Option<&str>) -> Self {
        let font_cache = FontCache::new(font_size, custom_font_path);
        let mut glyph_cache = GlyphCache::new();
        glyph_cache.update_typeface_ids(&font_cache);

        let font_width = font_cache.font_width;
        let font_height = font_cache.font_height;

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        let mut bg_paint = Paint::default();
        bg_paint.set_style(PaintStyle::Fill);
        let mut underline_paint = Paint::default();
        underline_paint.set_stroke_width(1.0);
        let mut strikethrough_paint = Paint::default();
        strikethrough_paint.set_stroke_width(1.0);
        let mut cursor_paint = Paint::default();
        cursor_paint.set_style(PaintStyle::Fill);

        Self {
            font_size,
            font_path: custom_font_path.map(String::from),
            font_cache,
            paint,
            bg_paint,
            underline_paint,
            strikethrough_paint,
            cursor_paint,
            font_width,
            font_height,
            run_buf: String::with_capacity(256),
            selection: SelectionBounds::default(),
            row_cache: Vec::with_capacity(100),
            recorder: skia_safe::PictureRecorder::new(),
            glyph_cache,
            // 预分配缓冲区
            text_blob_builder: TextBlobBuilder::new(),
            group_chars_buf: Vec::with_capacity(256),
            row_selection_buf: Vec::with_capacity(512),
            text_blob_cache: HashMap::with_capacity(512),
        }
    }

    pub fn set_selection(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.selection = SelectionBounds { x1, y1, x2, y2, active: true };
    }

    pub fn clear_selection(&mut self) {
        self.selection.active = false;
    }

    #[inline]
    pub fn is_cell_selected(&self, col: i32, row: i32) -> bool {
        if !self.selection.active { return false; }
        let s = &self.selection;
        let (sy, sx, ey, ex) = if s.y1 < s.y2 || (s.y1 == s.y2 && s.x1 <= s.x2) { (s.y1, s.x1, s.y2, s.x2) } else { (s.y2, s.x2, s.y1, s.x1) };
        if row < sy || row > ey { return false; }
        if row == sy && row == ey { return col >= sx && col <= ex; }
        if row == sy { return col >= sx; }
        if row == ey { return col <= ex; }
        true
    }

    #[inline]
    fn apply_dim(color: u32) -> u32 {
        let r = (((color >> 16) & 0xFF) as u32 * 2 / 3).min(255);
        let g = (((color >> 8) & 0xFF) as u32 * 2 / 3).min(255);
        let b = ((color & 0xFF) as u32 * 2 / 3).min(255);
        (color & 0xFF000000) | (r << 16) | (g << 8) | b
    }

    #[inline]
    pub fn reverse_colors(fg: usize, bg: usize) -> (usize, usize) { (bg, fg) }

    pub fn draw_frame(&mut self, canvas: &Canvas, frame: &RenderFrame, _scale: f32, _scroll_offset: f32) {
        let palette = &frame.palette;
        let palette_h = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            palette.hash(&mut h);
            h.finish()
        };

        canvas.reset_matrix();
        canvas.clear(Color::new(palette[257]));

        if self.row_cache.len() < frame.rows { self.row_cache.resize_with(frame.rows, || None); }

        let rows = frame.rows;
        let cols = frame.cols;
        let global_reverse = frame.reverse_video;
        let top_row = frame.top_row;

        // 优化：预分配选区缓冲区，避免每行分配
        if self.row_selection_buf.len() < cols {
            self.row_selection_buf.resize(cols, false);
        }

        for r in 0..rows as i32 {
            let absolute_row = top_row + r;
            let row_info = &frame.row_data[r as usize];
            let (row_text, row_styles, row_h) = (&row_info.0, &row_info.1, row_info.2);
            let y_base = (r as f32 + 1.0) * self.font_height;

            // 内联选区计算，用于哈希校验
            let sel_bounds = &self.selection;
            let sel_active = sel_bounds.active;
            let (sy, sx, ey, ex) = if sel_active {
                if sel_bounds.y1 < sel_bounds.y2 || (sel_bounds.y1 == sel_bounds.y2 && sel_bounds.x1 <= sel_bounds.x2) {
                    (sel_bounds.y1, sel_bounds.x1, sel_bounds.y2, sel_bounds.x2)
                } else {
                    (sel_bounds.y2, sel_bounds.x2, sel_bounds.y1, sel_bounds.x1)
                }
            } else {
                (0, 0, 0, 0)
            };

            // 计算当前行的选区哈希
            let row_sel_hash = if !sel_active || absolute_row < sy || absolute_row > ey {
                0u64
            } else if (absolute_row > sy && absolute_row < ey) 
                || (absolute_row == sy && sx == 0 && absolute_row < ey)
                || (absolute_row == ey && ex >= cols as i32 - 1 && absolute_row > sy)
                || (absolute_row == sy && absolute_row == ey && sx == 0 && ex >= cols as i32 - 1) 
            {
                u64::MAX // 整行选中
            } else {
                // 部分选中：对列范围进行哈希
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                if absolute_row == sy { sx.hash(&mut h); }
                if absolute_row == ey { ex.hash(&mut h); }
                h.finish()
            };

            // 行缓存命中
            if let Some(ref entry) = self.row_cache[r as usize] {
                if entry.hash == row_h && entry.palette_hash == palette_h && entry.selection_hash == row_sel_hash {
                    canvas.draw_picture(&entry.picture, None, None);
                    continue;
                }
            }

            // 克隆行数据以避免借用冲突
            let row_text_clone = row_text.clone();
            let row_styles_clone = row_styles.clone();


            let row_sel = &mut self.row_selection_buf[..cols];
            let abs_row = absolute_row;
            for c_idx in 0..cols {
                if !sel_active {
                    row_sel[c_idx] = false;
                } else {
                    let r = abs_row;
                    let c = c_idx as i32;
                    row_sel[c_idx] = if r < sy || r > ey {
                        false
                    } else if r == sy && r == ey {
                        c >= sx && c <= ex
                    } else if r == sy {
                        c >= sx
                    } else if r == ey {
                        c <= ex
                    } else {
                        true
                    };
                }
            }

            let row_rect = Rect::from_xywh(0.0, r as f32 * self.font_height, cols as f32 * self.font_width, self.font_height);
            let font_w = self.font_width;
            let font_h = self.font_height;
            let f_cache = &self.font_cache;
            let g_cache = &mut self.glyph_cache;
            let r_buf = &mut self.run_buf;
            let p_mut = &mut self.paint;
            let bg_p_mut = &mut self.bg_paint;
            let u_p_mut = &mut self.underline_paint;
            let s_p_mut = &mut self.strikethrough_paint;

            let recording_canvas = self.recorder.begin_recording(row_rect, false);
            let row_text = &row_text_clone;
            let row_styles = &row_styles_clone;

            let mut c = 0;
            while c < cols {
                if c >= row_text.len() { break; }
                let start_c = c;
                let style = row_styles[c];
                let effect = decode_effect(style);
                if (effect & EFFECT_INVISIBLE) != 0 {
                    let ch = row_text[c];
                    c += if ch == '\0' { 1 } else { char_wc_width(ch as u32) };
                    continue;
                }

                r_buf.clear();
                let mut run_cells = 0usize;
                let sel = row_sel[c];

                while c < cols && c < row_text.len() {
                    let cell_style = row_styles[c];
                    let cell_sel = row_sel[c];
                    if (cell_style == style && cell_sel == sel) || row_text[c] == '\0' {
                        let ch = row_text[c];
                        if ch != '\0' {
                            r_buf.push(ch);
                            run_cells += char_wc_width(ch as u32);
                        }
                        c += 1;
                    } else { break; }
                }

                if !r_buf.is_empty() {
                    // 优化：直接传递 &r_buf，避免 drain().collect() 创建新 String
                    let run_text = r_buf.as_str();
                    Self::draw_run_optimized(
                        recording_canvas, run_text, start_c as f32 * font_w, y_base,
                        run_cells as f32 * font_w, f_cache, g_cache,
                        p_mut, bg_p_mut, u_p_mut, s_p_mut, font_w, font_h,
                        style, palette, global_reverse, sel,
                        &mut self.text_blob_builder, &mut self.group_chars_buf,
                        &mut self.text_blob_cache,
                    );
                    r_buf.clear();
                }
            }

            if let Some(pic) = self.recorder.finish_recording_as_picture(None) {
                canvas.draw_picture(&pic, None, None);
                self.row_cache[r as usize] = Some(RowCacheEntry {
                    hash: row_h,
                    picture: pic,
                    palette_hash: palette_h,
                    selection_hash: row_sel_hash,
                });
            }
        }

        // 光标绘制
        if frame.cursor_enabled {
            self.cursor_paint.set_color(Color::new(palette[COLOR_INDEX_CURSOR]));
            // 修复: 考虑滚动偏移 (top_row)
            let visual_y = frame.cursor_y - frame.top_row;
            if visual_y >= 0 && visual_y < frame.rows as i32 {
                let cx = frame.cursor_x as f32 * self.font_width;
                let cy = visual_y as f32 * self.font_height;
                
                // 修复: 考虑宽字符宽度
                let mut cursor_width = self.font_width;
                if let Some(row) = frame.row_data.get(visual_y as usize) {
                    if let Some(&ch) = row.0.get(frame.cursor_x as usize) {
                        if char_wc_width(ch as u32) > 1 {
                            cursor_width *= 2.0;
                        }
                    }
                }

                let rect = match frame.cursor_style {
                    1 => Rect::from_xywh(cx, cy + self.font_height - 2.0, cursor_width, 2.0),
                    2 => Rect::from_xywh(cx, cy, 2.0, self.font_height),
                    _ => Rect::from_xywh(cx, cy, cursor_width, self.font_height),
                };

                // 改进: 使用 Difference 混合模式，使光标下的文字可见
                let old_blend = self.cursor_paint.blend_mode_or(skia_safe::BlendMode::SrcOver);
                self.cursor_paint.set_blend_mode(skia_safe::BlendMode::Difference);
                canvas.draw_rect(rect, &self.cursor_paint);
                self.cursor_paint.set_blend_mode(old_blend);
            }
        }
    }

    fn draw_run_static(canvas: &Canvas, text: &str, x: f32, y_base: f32, expected_width: f32, font_cache: &FontCache, glyph_cache: &mut GlyphCache, paint: &mut Paint, bg_paint: &mut Paint, underline_paint: &mut Paint, strikethrough_paint: &mut Paint, font_width: f32, font_height: f32, style: u64, palette: &[u32; NUM_INDEXED_COLORS], global_reverse: bool, is_selected: bool) {
        let effect = decode_effect(style);
        let mut fg_idx = decode_fore_color(style) as usize;
        let mut bg_idx = decode_back_color(style) as usize;
        let (fg_tc, bg_tc) = ((effect & STYLE_TRUECOLOR_FG) != 0, (effect & STYLE_TRUECOLOR_BG) != 0);
        let bold = (effect & EFFECT_BOLD) != 0;
        if bold && !fg_tc && fg_idx < 8 { fg_idx += 8; }
        let mut do_reverse = global_reverse != ((effect & EFFECT_REVERSE) != 0);
        if is_selected { do_reverse = !do_reverse; }
        let (mut fg_tc_final, mut bg_tc_final) = (fg_tc, bg_tc);
        if do_reverse {
            let (nf, nb) = Self::reverse_colors(fg_idx, bg_idx);
            fg_idx = nf; bg_idx = nb;
            std::mem::swap(&mut fg_tc_final, &mut bg_tc_final);
        }
        let mut fg_color_val = if fg_tc_final { fg_idx as u32 } else { palette[fg_idx.min(258)] };
        if (effect & EFFECT_DIM) != 0 { fg_color_val = Self::apply_dim(fg_color_val); }
        let bg_color_val = if bg_tc_final { bg_idx as u32 } else { palette[bg_idx.min(258)] };
        if bg_tc_final || bg_idx != 257 {
            bg_paint.set_color(Color::new(bg_color_val));
            canvas.draw_rect(Rect::from_xywh(x, y_base - font_height, expected_width, font_height), bg_paint);
        }
        let fg_color = Color::new(fg_color_val);
        paint.set_color(fg_color);
        let mut current_x = x;
        let italic = (effect & EFFECT_ITALIC) != 0;
        let font_ascent = font_cache.font_ascent;

        // 使用 TextBlob 渲染文本，按字体分组
        let mut builder = TextBlobBuilder::new();
        let mut group_chars: Vec<(char, f32)> = Vec::new(); // (char, relative_x)
        let mut group_font: Option<Font> = None;

        for ch in text.chars() {
            if ch == '\0' { continue; }
            let logic_w = char_wc_width(ch as u32) as f32 * font_width;

            if is_special_render_char(ch) {
                // 先刷新 TextBlob
                if !group_chars.is_empty() {
                    if let Some(font) = group_font.take() {
                        Self::flush_text_group_blob(&mut builder, &mut group_chars, &font, glyph_cache);
                    }
                }
                // 绘制块元素（直接在 recording canvas 上）
                Self::draw_block_char_blob(canvas, ch, current_x - x, y_base, logic_w, font_height, fg_color_val, bg_color_val, bg_paint, paint, font_cache, &mut builder, glyph_cache);
                current_x += logic_w;
                continue;
            }

            let (font, _) = font_cache.get_font_for_char(ch, bold, italic);
            let font_id = font.typeface().unique_id();

            // 如果字体切换，先刷新当前组
            if let Some(ref prev_font) = group_font {
                if font_id != prev_font.typeface().unique_id() {
                    Self::flush_text_group_blob(&mut builder, &mut group_chars, prev_font, glyph_cache);
                }
            }

            group_font = Some(font);
            group_chars.push((ch, current_x - x)); // 使用相对 X 坐标
            current_x += logic_w;
        }

        // 刷新剩余的文本组
        if let Some(font) = group_font.take() {
            if !group_chars.is_empty() {
                Self::flush_text_group_blob(&mut builder, &mut group_chars, &font, glyph_cache);
            }
        }

        // 一次性绘制所有 TextBlob（相对于 x 坐标）
        if let Some(blob) = builder.make() {
            let blob_y = y_base + font_ascent * 0.15;
            canvas.draw_text_blob(&blob, (x, blob_y), paint);
        }

        // 特效绘制
        if (effect & EFFECT_UNDERLINE) != 0 {
            underline_paint.set_color(fg_color);
            canvas.draw_line((x, y_base - 2.0), (x + expected_width, y_base - 2.0), underline_paint);
        }
        if (effect & EFFECT_STRIKETHROUGH) != 0 {
            strikethrough_paint.set_color(fg_color);
            canvas.draw_line((x, y_base - font_height * 0.5), (x + expected_width, y_base - font_height * 0.5), strikethrough_paint);
        }
    }

    /// 优化版本的 draw_run：使用预分配缓冲区和字体索引查找
    fn draw_run_optimized(
        canvas: &Canvas,
        text: &str,
        x: f32,
        y_base: f32,
        expected_width: f32,
        font_cache: &FontCache,
        glyph_cache: &mut GlyphCache,
        paint: &mut Paint,
        bg_paint: &mut Paint,
        underline_paint: &mut Paint,
        strikethrough_paint: &mut Paint,
        font_width: f32,
        font_height: f32,
        style: u64,
        palette: &[u32; NUM_INDEXED_COLORS],
        global_reverse: bool,
        is_selected: bool,
        builder: &mut TextBlobBuilder,
        group_chars: &mut Vec<(char, f32)>,
        blob_cache: &mut HashMap<u64, TextBlob>,
    ) {
        let effect = decode_effect(style);
        let mut fg_idx = decode_fore_color(style) as usize;
        let mut bg_idx = decode_back_color(style) as usize;
        let (fg_tc, bg_tc) = ((effect & STYLE_TRUECOLOR_FG) != 0, (effect & STYLE_TRUECOLOR_BG) != 0);
        let bold = (effect & EFFECT_BOLD) != 0;
        if bold && !fg_tc && fg_idx < 8 { fg_idx += 8; }
        let mut do_reverse = global_reverse != ((effect & EFFECT_REVERSE) != 0);
        if is_selected { do_reverse = !do_reverse; }
        let (mut fg_tc_final, mut bg_tc_final) = (fg_tc, bg_tc);
        if do_reverse {
            let (nf, nb) = Self::reverse_colors(fg_idx, bg_idx);
            fg_idx = nf; bg_idx = nb;
            std::mem::swap(&mut fg_tc_final, &mut bg_tc_final);
        }
        let mut fg_color_val = if fg_tc_final { fg_idx as u32 } else { palette[fg_idx.min(258)] };
        if (effect & EFFECT_DIM) != 0 { fg_color_val = Self::apply_dim(fg_color_val); }
        let bg_color_val = if bg_tc_final { bg_idx as u32 } else { palette[bg_idx.min(258)] };
        if bg_tc_final || bg_idx != 257 {
            bg_paint.set_color(Color::new(bg_color_val));
            canvas.draw_rect(Rect::from_xywh(x, y_base - font_height, expected_width, font_height), bg_paint);
        }
        let fg_color = Color::new(fg_color_val);
        paint.set_color(fg_color);

        let font_ascent = font_cache.font_ascent;
        let blob_y = y_base + font_ascent * 0.15;

        // === TextBlob 缓存逻辑 ===
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        style.hash(&mut hasher);
        global_reverse.hash(&mut hasher);
        is_selected.hash(&mut hasher);
        let cache_key = hasher.finish();

        if let Some(blob) = blob_cache.get(&cache_key) {
            canvas.draw_text_blob(blob, (x, blob_y), paint);
        } else {
            let mut current_x = x;
            let italic = (effect & EFFECT_ITALIC) != 0;

            // 优化：使用预分配的 group_chars 缓冲区
            group_chars.clear();
            let mut current_group_font: Option<Font> = None;

            for ch in text.chars() {
                if ch == '\0' { continue; }
                let logic_w = crate::wcwidth::wcwidth(ch as u32) as f32 * font_width;

                if is_special_render_char(ch) {
                    // 刷新 TextBlob
                    if !group_chars.is_empty() {
                        if let Some(ref f) = current_group_font {
                            Self::flush_text_group_blob(builder, group_chars, f, glyph_cache);
                        }
                    }
                    // 绘制块元素
                    Self::draw_block_char_blob(canvas, ch, current_x - x, y_base, logic_w, font_height, fg_color_val, bg_color_val, bg_paint, paint, font_cache, builder, glyph_cache);
                    current_x += logic_w;
                    continue;
                }

                // 核心改动：使用动态匹配逻辑
                let (font, _) = font_cache.get_font_for_char(ch, bold, italic);

                // 如果字体切换，刷新当前组
                if let Some(ref prev_font) = current_group_font {
                    if font.typeface().unique_id() != prev_font.typeface().unique_id() {
                        Self::flush_text_group_blob(builder, group_chars, prev_font, glyph_cache);
                    }
                }

                current_group_font = Some(font);
                group_chars.push((ch, current_x - x));
                current_x += logic_w;
            }

            // 刷新剩余的文本组
            if let Some(ref f) = current_group_font {
                if !group_chars.is_empty() {
                    Self::flush_text_group_blob(builder, group_chars, f, glyph_cache);
                }
            }

            // 一次性绘制所有 TextBlob 并存入缓存
            if let Some(blob) = builder.make() {
                canvas.draw_text_blob(&blob, (x, blob_y), paint);
                // 缓存生成的 blob (Skia TextBlob 是引用计数的，克隆成本极低)
                if blob_cache.len() < 2000 {
                    blob_cache.insert(cache_key, blob);
                }
            }
        }

        // 特效绘制
        if (effect & EFFECT_UNDERLINE) != 0 {
            underline_paint.set_color(fg_color);
            canvas.draw_line((x, y_base - 2.0), (x + expected_width, y_base - 2.0), underline_paint);
        }
        if (effect & EFFECT_STRIKETHROUGH) != 0 {
            strikethrough_paint.set_color(fg_color);
            canvas.draw_line((x, y_base - font_height * 0.5), (x + expected_width, y_base - font_height * 0.5), strikethrough_paint);
        }
    }

    /// 将一组相同字体的字符刷新到 TextBlob
    fn flush_text_group_blob(builder: &mut TextBlobBuilder, chars: &mut Vec<(char, f32)>, font: &Font, glyph_cache: &mut GlyphCache) {
        if chars.is_empty() { return; }
        let count = chars.len();
        let (run_glyphs, run_pos) = builder.alloc_run_pos_h(font, count, 0.0, None);
        for (i, (ch, rel_x)) in chars.iter().enumerate() {
            run_glyphs[i] = glyph_cache.get_glyph(font, *ch);
            run_pos[i] = *rel_x;
        }
        chars.clear();
    }

    /// 绘制块元素字符，使用 TextBlob 回退到字体渲染
    fn draw_block_char_blob(
        canvas: &Canvas,
        ch: char,
        rel_x: f32,
        y_base: f32,
        cell_w: f32,
        cell_h: f32,
        fg_color: u32,
        bg_color: u32,
        bg_paint: &mut Paint,
        _paint: &mut Paint,
        font_cache: &FontCache,
        builder: &mut TextBlobBuilder,
        glyph_cache: &mut GlyphCache,
    ) {
        let x = rel_x;
        let y_top = y_base - cell_h;

        // 象限块
        let q_mask: u8 = match ch as u32 {
            0x2596 => 0b0100, 0x2597 => 0b1000, 0x2598 => 0b0001, 0x259D => 0b0010,
            0x2599 => 0b1101, 0x259A => 0b1001, 0x259E => 0b0110, 0x259B => 0b0111,
            0x259C => 0b1011, 0x259F => 0b1110, _ => 0,
        };
        if q_mask != 0 {
            let (hw, hh) = (cell_w / 2.0, cell_h / 2.0);
            let quads = [
                (x, y_top, hw, hh, (q_mask & 1) != 0),
                (x + hw, y_top, cell_w - hw, hh, (q_mask & 2) != 0),
                (x, y_top + hh, hw, cell_h - hh, (q_mask & 4) != 0),
                (x + hw, y_top + hh, cell_w - hw, cell_h - hh, (q_mask & 8) != 0),
            ];
            for (qx, qy, qw, qh, fill) in quads {
                bg_paint.set_color(Color::new(if fill { fg_color } else { bg_color }));
                canvas.draw_rect(Rect::from_xywh(qx, qy, qw, qh), bg_paint);
            }
            return;
        }

        // 全块
        if ch as u32 == 0x2588 {
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, cell_h), bg_paint);
            return;
        }

        // 垂直分数块 (上)
        if let Some(n) = match ch as u32 {
            0x2580 => Some(4), // 1/2
            0x2594 => Some(1), // 1/8
            _ => None,
        } {
            let fh = cell_h * n as f32 / 8.0;
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, fh), bg_paint);
            bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top + fh, cell_w, cell_h - fh), bg_paint);
            return;
        }

        // 垂直分数块 (下)
        if let Some(n) = match ch as u32 {
            0x2581 => Some(1), 0x2582 => Some(2), 0x2583 => Some(3),
            0x2584 => Some(4), // 1/2
            0x2585 => Some(5), 0x2586 => Some(6), 0x2587 => Some(7), _ => None,
        } {
            let fh = cell_h * n as f32 / 8.0;
            bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, cell_h - fh), bg_paint);
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top + cell_h - fh, cell_w, fh), bg_paint);
            return;
        }

        // 水平分数块 (左)
        if let Some(n) = match ch as u32 {
            0x258F => Some(1), 0x258E => Some(2), 0x258D => Some(3),
            0x258C => Some(4), // 1/2
            0x258B => Some(5), 0x258A => Some(6), 0x2589 => Some(7), _ => None,
        } {
            let fw = cell_w * n as f32 / 8.0;
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, fw, cell_h), bg_paint);
            bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x + fw, y_top, cell_w - fw, cell_h), bg_paint);
            return;
        }

        // 水平分数块 (右)
        if let Some(n) = match ch as u32 {
            0x2590 => Some(4), // 1/2
            0x2595 => Some(1), // 1/8
            _ => None,
        } {
            let fw = cell_w * n as f32 / 8.0;
            bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w - fw, cell_h), bg_paint);
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x + cell_w - fw, y_top, fw, cell_h), bg_paint);
            return;
        }

        // 阴影块
        if matches!(ch as u32, 0x2591..=0x2593) {
            let d = match ch as u32 { 0x2591 => 0.25, 0x2592 => 0.50, _ => 0.75 };
            bg_paint.set_color(Color::new(bg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, cell_h), bg_paint);
            Self::draw_shade_pattern_blob(canvas, x, y_top, cell_w, cell_h, fg_color, d, bg_paint);
            return;
        }

        // 盒绘图 - 水平线
        if ch as u32 == 0x2500 {
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x, y_top + cell_h / 2.0 - 0.5, cell_w, 1.0), bg_paint);
            return;
        }
        // 盒绘图 - 垂直线
        if ch as u32 == 0x2502 {
            bg_paint.set_color(Color::new(fg_color));
            canvas.draw_rect(Rect::from_xywh(x + cell_w / 2.0 - 0.5, y_top, 1.0, cell_h), bg_paint);
            return;
        }

        // 回退：使用 TextBlob 渲染未处理的字符
        let (font, _) = font_cache.get_font_for_char(ch, false, false);
        let (run_glyphs, run_pos) = builder.alloc_run_pos_h(&font, 1, 0.0, None);
        run_glyphs[0] = glyph_cache.get_glyph(&font, ch);
        run_pos[0] = rel_x;
    }

    /// 绘制阴影图案
    fn draw_shade_pattern_blob(canvas: &Canvas, x: f32, y: f32, w: f32, h: f32, color: u32, density: f32, bg_paint: &mut Paint) {
        bg_paint.set_color(Color::new(color));
        let step = 2.0;
        let mut row = 0.0f32;
        while row < h {
            let mut col = 0.0f32;
            while col < w {
                if ((col / step).floor() as i32 + (row / step).floor() as i32) % 2 == 0
                    && (density > 0.4 || (col / step).floor() as i32 % 3 != 0)
                {
                    canvas.draw_rect(Rect::from_xywh(x + col, y + row, step.min(w - col), step.min(h - row)), bg_paint);
                }
                col += step;
            }
            row += step;
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_font_metrics_calculation() { let renderer = TerminalRenderer::new(&[], 12.0, None); assert!(renderer.font_width > 0.0); assert!(renderer.font_height > 0.0); }
    #[test]
    fn test_dim_color() { let white = 0xffffffff; let dimmed = TerminalRenderer::apply_dim(white); assert_eq!((dimmed >> 16) & 0xFF, 170); }
    #[test]
    fn test_selection_bounds() { let mut renderer = TerminalRenderer::new(&[], 12.0, None); renderer.set_selection(2, 1, 5, 3); assert!(renderer.is_cell_selected(3, 2)); }

    #[test]
    fn test_cursor_visibility_logic() {
        let mut engine = crate::engine::TerminalEngine::new(80, 24, 100, 10, 20);
        
        // 1. 默认状态：光标启用，不闪烁 -> 应可见
        engine.state.cursor_enabled = true;
        engine.state.cursor.blinking_enabled = false;
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0);
        assert!(frame.cursor_enabled);

        // 2. 启用闪烁，状态为可见 -> 应可见
        engine.state.cursor.blinking_enabled = true;
        engine.state.cursor.blink_state = true;
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0);
        assert!(frame.cursor_enabled);

        // 3. 启用闪烁，状态为不可见 -> 应不可见
        engine.state.cursor.blink_state = false;
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0);
        assert!(!frame.cursor_enabled);

        // 4. 关闭光标 -> 无论闪烁状态如何都不可见
        engine.state.cursor_enabled = false;
        engine.state.cursor.blink_state = true;
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0);
        assert!(!frame.cursor_enabled);
    }

    #[test]
    fn test_selection_pipeline_invalidation() {
        // 1. 初始化引擎和渲染器
        let mut engine = crate::engine::TerminalEngine::new(80, 24, 100, 10, 20);
        let mut renderer = TerminalRenderer::new(&[], 12.0, None);
        let mut surface = skia_safe::surfaces::raster(&skia_safe::ImageInfo::new_n32_premul((800, 600), None), None, None).unwrap();
        let canvas = surface.canvas();

        // 填充一行测试数据
        engine.state.main_screen.get_row_mut(0).text[0] = 'A';
        engine.state.main_screen.get_row_mut(0).text[1] = 'B';

        // 2. 第一次绘制：无选区
        renderer.clear_selection();
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0);
        renderer.draw_frame(canvas, &frame, 1.0, 0.0);

        let hash_no_sel = renderer.row_cache[0].as_ref().unwrap().selection_hash;
        assert_eq!(hash_no_sel, 0, "无选区时 hash 应为 0");
        let pic_no_sel = renderer.row_cache[0].as_ref().unwrap().picture.unique_id();

        // 3. 第二次绘制：设置选区（选中第一行的前两个字符）
        renderer.set_selection(0, 0, 1, 0); 
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0); // 重新生成帧
        renderer.draw_frame(canvas, &frame, 1.0, 0.0);

        let hash_with_sel = renderer.row_cache[0].as_ref().unwrap().selection_hash;
        let pic_with_sel = renderer.row_cache[0].as_ref().unwrap().picture.unique_id();

        assert!(hash_with_sel != hash_no_sel, "选区变化后 hash 必须改变");
        assert!(pic_with_sel != pic_no_sel, "选区变化后必须重新记录 picture");

        // 4. 第三次绘制：选区跨行（第一行变为全选，因为它从 0 开始且不是最后一行）
        renderer.set_selection(0, 0, 10, 1); 
        let frame = RenderFrame::from_engine(&engine, 24, 80, 0); 
        renderer.draw_frame(canvas, &frame, 1.0, 0.0);

        let hash_full_sel = renderer.row_cache[0].as_ref().unwrap().selection_hash;
        assert_eq!(hash_full_sel, u64::MAX, "整行选中时 hash 应为 u64::MAX");
        
        // 校验第二行（部分选中）
        let hash_part_sel = renderer.row_cache[1].as_ref().unwrap().selection_hash;
        assert!(hash_part_sel != 0 && hash_part_sel != u64::MAX, "第二行应为部分选中哈希");
    }

    #[test]
    fn test_font_fallback_generic() {
        let renderer = TerminalRenderer::new(&[], 12.0, None);
        // 测试一组主字体肯定不支持的字符
        let characters = ['✦', '😊', '中', '𐐷'];
        for &ch in &characters {
            let (font, is_fallback) = renderer.font_cache.get_font_for_char(ch, false, false);
            let tf = font.typeface();
            let mut glyphs = [0u16; 1];
            tf.unichars_to_glyphs(&[ch as i32], &mut glyphs);
            println!("Char: {}, Fallback: {}, Glyph ID: {}", ch, is_fallback, glyphs[0]);
            assert!(glyphs[0] != 0, "Character {} resulted in TOFU!", ch);
        }
    }
}
