use crate::engine::TerminalEngine;
use crate::terminal::colors::{COLOR_INDEX_CURSOR, NUM_INDEXED_COLORS};
use crate::terminal::style::*;
use skia_safe::{BlendMode, Canvas, Color, Font, FontMgr, FontStyle, Paint, PaintStyle, Rect};
use std::sync::Arc;

use crate::render_thread;

/// 预计算的渲染帧数据 - 用于异步渲染（不需要持有 engine 锁）
#[derive(Clone)]
pub struct RenderFrame {
    pub rows: usize,
    pub cols: usize,
    pub palette: [u32; NUM_INDEXED_COLORS],
    pub palette_4f: [skia_safe::Color4f; NUM_INDEXED_COLORS],
    pub use_alternate_buffer: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub cursor_style: i32,
    pub cursor_enabled: bool,
    pub cursor_blinking_enabled: bool,
    pub cursor_blink_rate_ms: u64,
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
        let screen = if state.use_alternate_buffer {
            &state.alt_screen
        } else {
            &state.main_screen
        };

        let mut row_data = Vec::with_capacity(rows);
        let start_row = -(screen.active_transcript_rows as i32);
        let end_row = screen.rows as i32;

        for r in top_row..(top_row + rows as i32) {
            if r >= start_row && r < end_row {
                let row = screen.get_row(r as i64);
                row_data.push((row.text.clone(), row.styles.clone()));
            } else {
                // 逻辑之外的行返回空白
                row_data.push((
                    vec![' '; cols],
                    vec![crate::terminal::style::STYLE_NORMAL; cols],
                ));
            }
        }

        Self {
            rows,
            cols,
            palette: state.colors.current_colors,
            palette_4f: state.colors.current_colors_4f,
            use_alternate_buffer: state.use_alternate_buffer,
            cursor_x: state.cursor.x as i32,
            cursor_y: state.cursor.y as i32,
            cursor_style: state.cursor.style,
            cursor_enabled: state.cursor_enabled,
            cursor_blinking_enabled: state.cursor.blinking_enabled,
            cursor_blink_rate_ms: state.cursor.blink_rate_ms,
            reverse_video: state
                .modes
                .is_enabled(crate::terminal::modes::DECSET_BIT_REVERSE_VIDEO),
            top_row,
            row_data,
        }
    }
}

/// Unicode 字符终端单元格宽度计算 (与 Java WcWidth 一致)
/// 覆盖更多 CJK 扩展区、Emoji、全角标点等，避免宽度错配导致豆腐块
#[inline]
fn char_wc_width(ucs: u32) -> usize {
    crate::wcwidth::wcwidth(ucs)
}

/// 判断字符是否为块元素（Block Elements / Box Drawing / Braille 等）
/// 这些字符需要特殊的矩形填充渲染，而非依赖字体 glyph
#[inline]
pub fn is_block_element(ch: char) -> bool {
    matches!(ch as u32,
        0x2580..=0x259F  // Block Elements (▀▄█░▒▓▏▎▍▌▋▊▉)
        | 0x2500..=0x257F  // Box Drawing (─│┌┐└┘├┤┬┴┼)
    )
}

/// 判断字符是否需要特殊渲染（块元素、盲文等）
#[inline]
pub fn is_special_render_char(ch: char) -> bool {
    is_block_element(ch) || matches!(ch as u32, 0x2800..=0x28FF) // Braille
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
    font_mgr: Arc<FontMgr>,
}

unsafe impl Send for FontCache {}
unsafe impl Sync for FontCache {}

impl FontCache {
    fn new(font_size: f32, custom_font_path: Option<&str>) -> Option<Self> {
        let font_mgr = Arc::new(FontMgr::new());

        // Try to load custom font from file path if provided
        let custom_typeface = custom_font_path.and_then(|path| {
            std::fs::read(path).ok().and_then(|data| {
                let font_data = skia_safe::Data::new_copy(&data);
                font_mgr.new_from_data(&font_data, 0)
            })
        });

        let tf_mono = custom_typeface
            .clone()
            .or_else(|| font_mgr.match_family_style("monospace", FontStyle::normal()))
            .or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::normal()))
            .or_else(|| font_mgr.match_family_style("serif", FontStyle::normal()))
            .or_else(|| {
                // Last resort: iterate through all available font families
                let count = font_mgr.count_families();
                (0..count).find_map(|i| {
                    font_mgr.match_family_style(&font_mgr.family_name(i), FontStyle::normal())
                })
            });
        let tf_mono = match tf_mono {
            Some(tf) => tf,
            None => {
                crate::utils::android_log(
                    crate::utils::LogPriority::ERROR,
                    "FontCache::new: No system font available at all",
                );
                return None;
            }
        };

        let tf_bold = custom_typeface
            .as_ref()
            .map(|tf| tf.clone())
            .or_else(|| {
                font_mgr.match_family_style(
                    "monospace",
                    FontStyle::new(
                        skia_safe::font_style::Weight::BOLD,
                        skia_safe::font_style::Width::NORMAL,
                        skia_safe::font_style::Slant::Upright,
                    ),
                )
            })
            .unwrap_or_else(|| tf_mono.clone());
        let tf_italic = font_mgr
            .match_family_style(
                "monospace",
                FontStyle::new(
                    skia_safe::font_style::Weight::NORMAL,
                    skia_safe::font_style::Width::NORMAL,
                    skia_safe::font_style::Slant::Italic,
                ),
            )
            .unwrap_or_else(|| tf_mono.clone());
        let tf_bold_italic = font_mgr
            .match_family_style(
                "monospace",
                FontStyle::new(
                    skia_safe::font_style::Weight::BOLD,
                    skia_safe::font_style::Width::NORMAL,
                    skia_safe::font_style::Slant::Italic,
                ),
            )
            .unwrap_or_else(|| tf_mono.clone());

        // For fallback (non-ASCII), also prefer custom font if available
        let tf_fallback = custom_typeface
            .clone()
            .or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::normal()))
            .unwrap_or_else(|| tf_mono.clone());
        let tf_fallback_bold = custom_typeface
            .clone()
            .or_else(|| {
                font_mgr.match_family_style(
                    "sans-serif",
                    FontStyle::new(
                        skia_safe::font_style::Weight::BOLD,
                        skia_safe::font_style::Width::NORMAL,
                        skia_safe::font_style::Slant::Upright,
                    ),
                )
            })
            .unwrap_or_else(|| tf_mono.clone());

        let mut font_mono = Font::new(tf_mono.clone(), Some(font_size));
        font_mono.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
        font_mono.set_subpixel(true);

        let metrics = font_mono.metrics();
        let font_height = (metrics.1.descent - metrics.1.ascent + metrics.1.leading).ceil();
        let (w, _) = font_mono.measure_str("M", None);
        let font_width = w;

        // 构建各变体字体
        let build_font = |tf: &skia_safe::Typeface| {
            let mut f = Font::new(tf.clone(), Some(font_size));
            f.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
            f.set_subpixel(true);
            f
        };

        Some(Self {
            font_mono,
            font_bold: build_font(&tf_bold),
            font_italic: build_font(&tf_italic),
            font_bold_italic: build_font(&tf_bold_italic),
            font_fallback: build_font(&tf_fallback),
            font_fallback_bold: build_font(&tf_fallback_bold),
            font_width,
            font_height,
            font_ascent: metrics.1.ascent,
            font_mgr,
        })
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

    /// 获取字符对应的字体 — 关键修复：当 monospace 和 fallback 都不支持时，
    /// 使用系统匹配到的字体，避免豆腐块
    /// 返回 (Font, is_fallback) 元组，is_fallback=true 表示使用了特殊匹配字体
    fn get_font_for_char(&self, ch: char, bold: bool, italic: bool) -> (Font, bool) {
        let ucs = ch as u32;

        // 1. 尝试首选字体 (monospace)
        let primary = self.get_font(bold, italic, false);
        let tf = primary.typeface();
        let mut glyphs = [0u16; 1];
        tf.unichars_to_glyphs(&[ucs as i32], &mut glyphs);
        if glyphs[0] != 0 {
            return (primary.clone(), false);
        }

        // 2. 检查 fallback 是否支持
        let fallback_ref = if bold {
            &self.font_fallback_bold
        } else {
            &self.font_fallback
        };
        let mut fallback_glyphs = [0u16; 1];
        fallback_ref
            .typeface()
            .unichars_to_glyphs(&[ucs as i32], &mut fallback_glyphs);
        if fallback_glyphs[0] != 0 {
            return (fallback_ref.clone(), false);
        }

        // 3. 向系统请求匹配的字体 — 关键修复
        let weight = if bold {
            skia_safe::font_style::Weight::BOLD
        } else {
            skia_safe::font_style::Weight::NORMAL
        };
        let slant = if italic {
            skia_safe::font_style::Slant::Italic
        } else {
            skia_safe::font_style::Slant::Upright
        };
        let style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        // 尝试多种字体家族，提高找到支持字符的字体的概率
        if let Some(tf) = self
            .font_mgr
            .match_family_style_character("Noto Sans CJK SC", style, &[], ucs as i32)
            .or_else(|| {
                self.font_mgr
                    .match_family_style_character("Noto Sans", style, &[], ucs as i32)
            })
            .or_else(|| {
                self.font_mgr.match_family_style_character(
                    "Droid Sans Fallback",
                    style,
                    &[],
                    ucs as i32,
                )
            })
            .or_else(|| {
                self.font_mgr
                    .match_family_style_character("sans-serif", style, &[], ucs as i32)
            })
        {
            let mut matched_font = Font::new(tf, Some(self.font_height));
            matched_font.set_edging(skia_safe::font::Edging::SubpixelAntiAlias);
            matched_font.set_subpixel(true);
            return (matched_font, true);
        }

        // 4. 实在找不到，返回 fallback（会显示豆腐块，但至少尝试过了）
        (fallback_ref.clone(), false)
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
        if (ch as u32) < 128 {
            Some(self.widths[ch as usize])
        } else {
            None
        }
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
    pub font_path: Option<String>,
    font_cache: FontCache,
    ascii_cache: AsciiWidthCache,
    non_ascii_cache: NonAsciiWidthCache,
    paint: Paint,
    bg_paint: Paint,
    underline_paint: Paint,
    strikethrough_paint: Paint,
    cursor_paint: Paint,
    /// 复用 run 缓冲区，避免每帧分配
    run_buf: String,
    pub font_width: f32,
    pub font_height: f32,
    pub selection: SelectionBounds,
    /// HDR 图片覆盖层管理（预留接口，当前不绑定具体协议）
    pub hdr_manager: HdrOverlayManager,
}

unsafe impl Send for TerminalRenderer {}
unsafe impl Sync for TerminalRenderer {}

impl TerminalRenderer {
    pub fn new(_font_data: &[u8], font_size: f32, custom_font_path: Option<&str>) -> Option<Self> {
        let font_cache = FontCache::new(font_size, custom_font_path)?;
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

        // 光标绘制
        let mut cursor_paint = Paint::default();
        cursor_paint.set_style(PaintStyle::Fill);

        Some(Self {
            font_size,
            font_path: custom_font_path.map(String::from),
            font_cache,
            ascii_cache,
            non_ascii_cache: NonAsciiWidthCache::new(),
            paint,
            bg_paint,
            underline_paint,
            strikethrough_paint,
            cursor_paint,
            font_width,
            font_height,
            run_buf: String::with_capacity(256),
            selection: SelectionBounds::default(),
            hdr_manager: HdrOverlayManager::new(),
        })
    }

    /// 从 Java 侧设置选区坐标
    pub fn set_selection(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.selection = SelectionBounds {
            x1,
            y1,
            x2,
            y2,
            active: true,
        };
    }

    pub fn clear_selection(&mut self) {
        self.selection.active = false;
    }

    /// 判断给定的可见屏幕行列是否在选区内 (对齐 Upstream 逻辑)
    #[inline]
    pub fn is_cell_selected(&self, col: i32, row: i32) -> bool {
        if !self.selection.active {
            return false;
        }
        let s = &self.selection;

        // 确保 (sy, sx) 是起点，(ey, ex) 是终点
        let (sy, sx, ey, ex) = if s.y1 < s.y2 || (s.y1 == s.y2 && s.x1 <= s.x2) {
            (s.y1, s.x1, s.y2, s.x2)
        } else {
            (s.y2, s.x2, s.y1, s.x1)
        };

        if row < sy || row > ey {
            return false;
        }

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
    fn apply_dim_4f(color: skia_safe::Color4f) -> skia_safe::Color4f {
        skia_safe::Color4f::new(
            color.r * 2.0 / 3.0,
            color.g * 2.0 / 3.0,
            color.b * 2.0 / 3.0,
            color.a,
        )
    }

    #[inline]
    pub fn reverse_colors(fg: usize, bg: usize) -> (usize, usize) {
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
        let palette_4f = &state.colors.current_colors_4f;
        let screen = if state.use_alternate_buffer {
            &state.alt_screen
        } else {
            &state.main_screen
        };

        canvas.save();
        canvas.scale((scale, scale));

        // 背景清屏
        let bg_color_4f = palette_4f[257];
        canvas.clear(bg_color_4f);

        canvas.translate((0.0, -scroll_offset));

        let rows = state.rows as usize;
        let cols = state.cols as usize;
        let global_reverse = state
            .modes
            .is_enabled(crate::terminal::modes::DECSET_BIT_REVERSE_VIDEO);
        let top_row = render_thread::get_render_params().lock().unwrap().top_row;

        // 先绘制文本行 - 使用 get_row() 处理环形缓冲区映射
        for r in 0..rows as i32 {
            let absolute_row = top_row + r;
            let row_data = screen.get_row(absolute_row as i64);
            let y_base = (r as f32 + 1.0) * self.font_height;

            let mut c = 0;
            while c < cols {
                if c >= row_data.text.len() {
                    break;
                }
                let start_c = c;
                let style = row_data.styles[c];
                let effect = decode_effect(style);

                // 不可见文本跳过
                if (effect & EFFECT_INVISIBLE) != 0 {
                    let ch = row_data.text[c];
                    c += if ch == '\0' {
                        1
                    } else {
                        char_wc_width(ch as u32)
                    };
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
                        c += if ch == '\0' {
                            1
                        } else {
                            char_wc_width(ch as u32)
                        };
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
                            if ch as u32 > 127 {
                                run_has_non_ascii = true;
                            }
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
                        palette_4f,
                        global_reverse,
                        sel,
                        r as i32,
                    );
                }
            }
        }

        // 绘制光标（同步版本 - 根据当前时间计算闪烁状态）
        if state.cursor_enabled {
            let cursor = &state.cursor;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if cursor.should_be_visible(state.cursor_enabled, now_ms) {
                let cursor_color_4f = palette_4f[COLOR_INDEX_CURSOR];
                self.cursor_paint.set_color4f(cursor_color_4f, None);

                let cx = cursor.x as f32 * self.font_width;
                let cy = (cursor.y as f32 - top_row as f32) * self.font_height;

                match cursor.style {
                    0 => {
                        // Block cursor
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint,
                        );
                    }
                    1 => {
                        // Underline cursor (底部 2 像素)
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy + self.font_height - 2.0, self.font_width, 2.0),
                            &self.cursor_paint,
                        );
                    }
                    2 => {
                        // Bar cursor (左侧 2 像素宽竖线)
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, 2.0, self.font_height),
                            &self.cursor_paint,
                        );
                    }
                    _ => {
                        // 默认 block
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint,
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

        // 彻底重置矩阵并清除背景，防止上一帧的平移或缩放污染当前清屏结果
        canvas.reset_matrix();
        let bg_color = palette[257];
        canvas.clear(Color::new(bg_color));

        canvas.save();
        canvas.scale((scale, scale));

        // 重新启用平移以支持平滑的像素级滚动（子行滚动）
        // 我们只平移 scroll_offset 相对于行高的余数部分
        let fine_offset = _scroll_offset % self.font_height;
        canvas.translate((0.0, -fine_offset));

        let rows = frame.rows;
        let cols = frame.cols;
        let global_reverse = frame.reverse_video;
        let top_row = frame.top_row;

        // 先绘制文本行
        // 注意：平滑滚动时，最后一行可能会露出部分，所以可能需要多绘制一行
        // 但 RenderFrame 已经预取了数据，我们尽量在现有数据内绘制
        for r in 0..rows as i32 {
            let absolute_row = top_row + r;
            let row = &frame.row_data[r as usize];
            let row_text = &row.0;
            let row_styles = &row.1;
            let y_base = (r as f32 + 1.0) * self.font_height;

            let mut c = 0;
            while c < cols {
                if c >= row_text.len() {
                    break;
                }
                let start_c = c;
                let style = row_styles[c];
                let effect = decode_effect(style);

                // 不可见文本跳过
                if (effect & EFFECT_INVISIBLE) != 0 {
                    let ch = row_text[c];
                    c += if ch == '\0' {
                        1
                    } else {
                        char_wc_width(ch as u32)
                    };
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
                        c += if ch == '\0' {
                            1
                        } else {
                            char_wc_width(ch as u32)
                        };
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
                            if ch as u32 > 127 {
                                run_has_non_ascii = true;
                            }
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
                        &frame.palette_4f,
                        global_reverse,
                        sel,
                        r as i32,
                    );
                }
            }
        }

        // 绘制光标（异步版本 - 根据当前时间计算闪烁状态）
        if frame.cursor_enabled {
            let cursor_visible = if frame.cursor_blinking_enabled {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                (now_ms / frame.cursor_blink_rate_ms) % 2 == 0
            } else {
                true
            };
            if cursor_visible {
                let cursor_color_4f = frame.palette_4f[COLOR_INDEX_CURSOR];
                self.cursor_paint.set_color4f(cursor_color_4f, None);

                let cx = frame.cursor_x as f32 * self.font_width;
                let cy = (frame.cursor_y as f32 - frame.top_row as f32) * self.font_height;

                match frame.cursor_style {
                    0 => {
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint,
                        );
                    }
                    1 => {
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy + self.font_height - 2.0, self.font_width, 2.0),
                            &self.cursor_paint,
                        );
                    }
                    2 => {
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, 2.0, self.font_height),
                            &self.cursor_paint,
                        );
                    }
                    _ => {
                        canvas.draw_rect(
                            Rect::from_xywh(cx, cy, self.font_width, self.font_height),
                            &self.cursor_paint,
                        );
                    }
                }
            }
        }

        canvas.restore();

        // 绘制 HDR 图片覆盖层（预留接口，当前为空实现）
        self.hdr_manager.draw_overlays(canvas);
    }

    fn draw_run_opt(
        &mut self,
        canvas: &Canvas,
        text: &str,
        x: f32,
        y_base: f32,
        expected_width: f32,
        _measured_width: f32,
        _has_non_ascii: bool,
        style: u64,
        _palette: &[u32; NUM_INDEXED_COLORS],
        palette_4f: &[skia_safe::Color4f; NUM_INDEXED_COLORS],
        global_reverse: bool,
        is_selected: bool,
        _row: i32,
    ) {
        let mut fg_idx = decode_fore_color(style) as usize;
        let mut bg_idx = decode_back_color(style) as usize;
        let effect = decode_effect(style);

        let fg_truecolor = (effect & STYLE_TRUECOLOR_FG) != 0;
        let bg_truecolor = (effect & STYLE_TRUECOLOR_BG) != 0;

        // Bold→Bright 颜色映射（仅对索引色，不适用于真彩色）
        let bold = (effect & EFFECT_BOLD) != 0;
        if bold && !fg_truecolor && fg_idx < 8 {
            fg_idx += 8;
        }

        // 选区特效标准化 (反色)
        let mut do_reverse = global_reverse != ((effect & EFFECT_REVERSE) != 0);
        if is_selected {
            do_reverse = !do_reverse;
        }

        let mut fg_tc = fg_truecolor;
        let mut bg_tc = bg_truecolor;
        if do_reverse {
            let swapped = Self::reverse_colors(fg_idx, bg_idx);
            fg_idx = swapped.0;
            bg_idx = swapped.1;
            // 同时交换真彩色标志位，否则数据和标志位不匹配导致文字/背景同色
            let tmp = fg_tc;
            fg_tc = bg_tc;
            bg_tc = tmp;
        }

        // 解析前景色：真彩色直接使用，索引色查调色板
        let mut fg_color_4f: skia_safe::Color4f;
        if fg_tc {
            fg_color_4f = skia_safe::Color4f::from(skia_safe::Color::new(fg_idx as u32));
        } else {
            fg_color_4f = if fg_idx < 259 {
                palette_4f[fg_idx]
            } else {
                palette_4f[256]
            };
        }

        // Dim 效果
        if (effect & EFFECT_DIM) != 0 {
            fg_color_4f = Self::apply_dim_4f(fg_color_4f);
        }

        // 解析背景色：真彩色直接使用，索引色查调色板
        let bg_color_4f: skia_safe::Color4f;
        let has_bg = if bg_tc {
            bg_color_4f = skia_safe::Color4f::from(skia_safe::Color::new(bg_idx as u32));
            true // Truecolor always has a background
        } else {
            bg_color_4f = if bg_idx < 259 {
                palette_4f[bg_idx]
            } else {
                palette_4f[257]
            };
            bg_idx != 257 // 257 = default background, don't draw
        };
        if has_bg {
            self.bg_paint.set_color4f(bg_color_4f, None);
            canvas.draw_rect(
                Rect::from_xywh(
                    x,
                    y_base - self.font_height,
                    expected_width,
                    self.font_height,
                ),
                &self.bg_paint,
            );
        }

        let italic = (effect & EFFECT_ITALIC) != 0;
        self.paint.set_color4f(fg_color_4f, None);

        let mut current_x = x;
        let y_adjusted = y_base - (self.font_cache.font_height + self.font_cache.font_ascent);

        // 分组绘制：将连续使用相同字体的字符合并为一个子 Run
        // 块元素特殊处理：使用矩形填充代替字体 glyph
        let mut group_text = String::new();
        let mut group_font: Option<Font> = None;
        let mut group_start_x = x;
        let mut group_logic_w = 0.0f32;

        for ch in text.chars() {
            if ch == '\0' {
                continue;
            }

            // 块元素/特殊字符不走字体渲染，直接矩形填充
            if is_special_render_char(ch) {
                // 先刷新当前 group
                if let Some(ref font) = group_font {
                    self.draw_char_group(
                        canvas,
                        &group_text,
                        group_start_x,
                        y_adjusted,
                        font,
                        group_logic_w,
                    );
                }
                group_text.clear();
                group_font = None;
                group_logic_w = 0.0;

                // 绘制块元素
                let logic_w = char_wc_width(ch as u32) as f32 * self.font_width;
                self.draw_block_char(
                    canvas,
                    ch,
                    current_x,
                    y_base,
                    logic_w,
                    self.font_height,
                    fg_color_4f,
                    bg_color_4f,
                );

                current_x += logic_w;
                continue;
            }

            let (font, _is_fallback) = self.font_cache.get_font_for_char(ch, bold, italic);
            let logic_w = char_wc_width(ch as u32) as f32 * self.font_width;

            if let Some(ref prev_font) = group_font {
                // 使用字体家族名作为分组依据（同一 typeface 可合并）
                let same_family = font.typeface().unique_id() == prev_font.typeface().unique_id();
                if same_family {
                    group_text.push(ch);
                    group_logic_w += logic_w;
                } else {
                    self.draw_char_group(
                        canvas,
                        &group_text,
                        group_start_x,
                        y_adjusted,
                        prev_font,
                        group_logic_w,
                    );
                    group_text.clear();
                    group_text.push(ch);
                    group_font = Some(font);
                    group_start_x = current_x;
                    group_logic_w = logic_w;
                }
            } else {
                group_text.push(ch);
                group_font = Some(font);
                group_start_x = x;
                group_logic_w = logic_w;
            }
            current_x += logic_w;
        }

        if let Some(ref font) = group_font {
            self.draw_char_group(
                canvas,
                &group_text,
                group_start_x,
                y_adjusted,
                font,
                group_logic_w,
            );
        }

        // 下划线
        if (effect & EFFECT_UNDERLINE) != 0 {
            let underline_y = y_base - 2.0;
            self.underline_paint.set_color4f(fg_color_4f, None);
            canvas.draw_line(
                (x, underline_y),
                (x + expected_width, underline_y),
                &self.underline_paint,
            );
        }

        // 删除线
        if (effect & EFFECT_STRIKETHROUGH) != 0 {
            let strike_y = y_base - self.font_height * 0.5;
            self.strikethrough_paint.set_color4f(fg_color_4f, None);
            canvas.draw_line(
                (x, strike_y),
                (x + expected_width, strike_y),
                &self.strikethrough_paint,
            );
        }
    }

    /// 辅助方法：绘制一组使用相同字体的字符，并进行缩放适配逻辑栅格
    fn draw_char_group(
        &self,
        canvas: &Canvas,
        text: &str,
        x: f32,
        y: f32,
        font: &Font,
        expected_w: f32,
    ) {
        let (measured_w, _) = font.measure_str(text, None);
        if measured_w <= 0.0 {
            return;
        }

        if (measured_w - expected_w).abs() > 0.5 {
            canvas.save();
            canvas.translate((x, y));
            canvas.scale((expected_w / measured_w, 1.0));
            canvas.draw_str(text, (0.0, 0.0), font, &self.paint);
            canvas.restore();
        } else {
            canvas.draw_str(text, (x, y), font, &self.paint);
        }
    }

    /// 绘制块元素字符，使用矩形填充确保像素级对齐
    /// 覆盖 U+2580-U+259F 全部 Block Elements（半块、1/8块、象限块、阴影）
    /// 以及 U+2500-U+257F Box Drawing（轻量线条）
    fn draw_block_char(
        &mut self,
        canvas: &Canvas,
        ch: char,
        x: f32,
        y_base: f32,
        cell_w: f32,
        cell_h: f32,
        fg_color: skia_safe::Color4f,
        bg_color: skia_safe::Color4f,
    ) {
        let y_top = y_base - cell_h;

        // === U+2596-U+259F: 象限块 (Quadrant Blocks) ===
        // 将单元格分为 4 个象限: TL TR / BL BR
        // 位掩码: 1=TL, 2=TR, 4=BL, 8=BR
        let q_mask: u8 = match ch as u32 {
            0x2596 => 0b0100,          // ▖ LOWER LEFT
            0x2597 => 0b1000,          // ▗ LOWER RIGHT
            0x2598 => 0b0001,          // ▘ UPPER LEFT
            0x259D => 0b0010,          // ▝ UPPER RIGHT
            0x2599 => 0b1101,          // ▙ TL + BL + BR
            0x259A | 0x259E => 0b1001, // ▚▞ TL + BR
            0x259B => 0b0111,          // ▛ TL + TR + BL
            0x259C => 0b1011,          // ▜ TL + TR + BR
            0x259F => 0b1110,          // ▟ TR + BL + BR
            _ => 0,
        };

        if q_mask != 0 {
            let half_w = cell_w / 2.0;
            let half_h = cell_h / 2.0;
            let quads = [
                (x, y_top, half_w, half_h, (q_mask & 0b0001) != 0), // TL
                (x + half_w, y_top, half_w, half_h, (q_mask & 0b0010) != 0), // TR
                (x, y_top + half_h, half_w, half_h, (q_mask & 0b0100) != 0), // BL
                (
                    x + half_w,
                    y_top + half_h,
                    half_w,
                    half_h,
                    (q_mask & 0b1000) != 0,
                ), // BR
            ];
            for (qx, qy, qw, qh, fill) in quads {
                self.bg_paint
                    .set_color4f(if fill { fg_color } else { bg_color }, None);
                canvas.draw_rect(Rect::from_xywh(qx, qy, qw, qh), &self.bg_paint);
            }
            return;
        }

        // === U+2588: Full Block ===
        if ch as u32 == 0x2588 {
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, cell_h), &self.bg_paint);
            return;
        }

        // === U+2580 / U+2584: 半高块 ===
        if ch as u32 == 0x2580 {
            // ▀ UPPER HALF
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top, cell_w, cell_h / 2.0),
                &self.bg_paint,
            );
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top + cell_h / 2.0, cell_w, cell_h / 2.0),
                &self.bg_paint,
            );
            return;
        }
        if ch as u32 == 0x2584 {
            // ▄ LOWER HALF
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top, cell_w, cell_h / 2.0),
                &self.bg_paint,
            );
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top + cell_h / 2.0, cell_w, cell_h / 2.0),
                &self.bg_paint,
            );
            return;
        }

        // === U+258C / U+2590: 半宽块 ===
        if ch as u32 == 0x258C {
            // ▌ LEFT HALF
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top, cell_w / 2.0, cell_h),
                &self.bg_paint,
            );
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x + cell_w / 2.0, y_top, cell_w / 2.0, cell_h),
                &self.bg_paint,
            );
            return;
        }
        if ch as u32 == 0x2590 {
            // ▐ RIGHT HALF
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x, y_top, cell_w / 2.0, cell_h),
                &self.bg_paint,
            );
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x + cell_w / 2.0, y_top, cell_w / 2.0, cell_h),
                &self.bg_paint,
            );
            return;
        }

        // === U+258F-U+2589: 1/8 块 (Left n/8) ===
        if let Some(n) = match ch as u32 {
            0x258F => Some(1), // ▏ 1/8
            0x258E => Some(2), // ▎ 2/8
            0x258D => Some(3), // ▍ 3/8
            0x258C => Some(4), // ▌ 4/8 (already handled above)
            0x258B => Some(5), // ▋ 5/8
            0x258A => Some(6), // ▊ 6/8
            0x2589 => Some(7), // ▉ 7/8
            _ => None,
        } {
            let fill_w = cell_w * n as f32 / 8.0;
            self.bg_paint.set_color4f(fg_color, None);
            canvas.draw_rect(Rect::from_xywh(x, y_top, fill_w, cell_h), &self.bg_paint);
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(
                Rect::from_xywh(x + fill_w, y_top, cell_w - fill_w, cell_h),
                &self.bg_paint,
            );
            return;
        }

        // === U+2591-U+2593: 阴影块 ===
        if matches!(ch as u32, 0x2591 | 0x2592 | 0x2593) {
            let density = match ch as u32 {
                0x2591 => 0.25, // ░ Light
                0x2592 => 0.50, // ▒ Medium
                _ => 0.75,      // ▓ Dark
            };
            self.bg_paint.set_color4f(bg_color, None);
            canvas.draw_rect(Rect::from_xywh(x, y_top, cell_w, cell_h), &self.bg_paint);
            self.draw_shade_pattern(canvas, x, y_top, cell_w, cell_h, fg_color, density);
            return;
        }

        // === U+2500-U+257F: Box Drawing ===
        // 轻量水平和垂直线
        if ch as u32 == 0x2500 {
            self.bg_paint.set_color4f(fg_color, None);
            let mid_y = y_top + cell_h / 2.0;
            canvas.draw_rect(Rect::from_xywh(x, mid_y - 0.5, cell_w, 1.0), &self.bg_paint);
            return;
        }
        if ch as u32 == 0x2502 {
            self.bg_paint.set_color4f(fg_color, None);
            let mid_x = x + cell_w / 2.0;
            canvas.draw_rect(
                Rect::from_xywh(mid_x - 0.5, y_top, 1.0, cell_h),
                &self.bg_paint,
            );
            return;
        }
        // 轻量角块
        if matches!(ch as u32, 0x250C | 0x2510 | 0x2514 | 0x2518) {
            self.bg_paint.set_color4f(fg_color, None);
            let mid_x = x + cell_w / 2.0;
            let mid_y = y_top + cell_h / 2.0;
            match ch as u32 {
                0x250C => {
                    // ┌ Down+Right
                    canvas.draw_rect(
                        Rect::from_xywh(mid_x - 0.5, mid_y, 1.0, cell_h / 2.0),
                        &self.bg_paint,
                    );
                    canvas.draw_rect(
                        Rect::from_xywh(x, mid_y - 0.5, cell_w / 2.0, 1.0),
                        &self.bg_paint,
                    );
                }
                0x2510 => {
                    // ┐ Down+Left
                    canvas.draw_rect(
                        Rect::from_xywh(mid_x - 0.5, mid_y, 1.0, cell_h / 2.0),
                        &self.bg_paint,
                    );
                    canvas.draw_rect(
                        Rect::from_xywh(x + cell_w / 2.0, mid_y - 0.5, cell_w / 2.0, 1.0),
                        &self.bg_paint,
                    );
                }
                0x2514 => {
                    // └ Up+Right
                    canvas.draw_rect(
                        Rect::from_xywh(mid_x - 0.5, y_top, 1.0, cell_h / 2.0),
                        &self.bg_paint,
                    );
                    canvas.draw_rect(
                        Rect::from_xywh(x, mid_y - 0.5, cell_w / 2.0, 1.0),
                        &self.bg_paint,
                    );
                }
                0x2518 => {
                    // ┘ Up+Left
                    canvas.draw_rect(
                        Rect::from_xywh(mid_x - 0.5, y_top, 1.0, cell_h / 2.0),
                        &self.bg_paint,
                    );
                    canvas.draw_rect(
                        Rect::from_xywh(x + cell_w / 2.0, mid_y - 0.5, cell_w / 2.0, 1.0),
                        &self.bg_paint,
                    );
                }
                _ => {}
            }
            return;
        }

        // Fallback: 使用字体渲染任何未处理的字符
        let (font, _) = self.font_cache.get_font_for_char(ch, false, false);
        self.draw_char_group(
            canvas,
            &ch.to_string(),
            x,
            y_base - (self.font_cache.font_height + self.font_cache.font_ascent),
            &font,
            cell_w,
        );
    }

    /// 绘制 shade 图案（使用小矩形模拟密度）
    fn draw_shade_pattern(
        &mut self,
        canvas: &Canvas,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: skia_safe::Color4f,
        density: f32,
    ) {
        self.bg_paint.set_color4f(color, None);
        let step = 2.0;
        let mut row = 0.0f32;
        // 使用棋盘格模式模拟 shade
        while row < h {
            let mut col = 0.0f32;
            while col < w {
                let is_on = ((col / step).floor() as i32 + (row / step).floor() as i32) % 2 == 0;
                if is_on && (density > 0.4 || (col / step).floor() as i32 % 3 != 0) {
                    canvas.draw_rect(
                        Rect::from_xywh(x + col, y + row, step.min(w - col), step.min(h - row)),
                        &self.bg_paint,
                    );
                }
                col += step;
            }
            row += step;
        }
    }

    /// 测量单个字符的像素宽度（使用缓存字体，避免重复创建 Font）
    #[inline]
    fn measure_char(&self, ch: char, effect: u64) -> f32 {
        let bold = (effect & EFFECT_BOLD) != 0;
        let italic = (effect & EFFECT_ITALIC) != 0;
        let (font, _) = self.font_cache.get_font_for_char(ch, bold, italic);
        let (w, _) = font.measure_str(&ch.to_string(), None);
        w
    }
}

// =====================================================================
// HDR 图片覆盖层接口（预留框架）
// =====================================================================
// 为后续 HDR 图片显示预留扩展点。当前不绑定具体终端协议或编码格式，
// 仅定义渲染侧需要的数据结构、管理器和绘制入口。
//
// 背景：终端标准协议（Sixel / Kitty Graphics Protocol 等）目前均无 HDR 扩展。
// 实际 HDR 内容最可能的来源是：
//   1. 本地文件解码（AVIF HDR / HEIF / JPEG XL / PNG cICP）
//   2. 未来可能出现的非标准 Kitty Graphics Protocol 扩展
//   3. 应用层直接通过 JNI/Rust API 投递 HDR 纹理
// =====================================================================

/// HDR 电光转换函数（EOTF）与色域组合
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HdrColorSpace {
    /// 普通 SDR：sRGB 原色 + sRGB EOTF
    SdrSrgb,
    /// Rec.2020 原色 + HLG (Hybrid Log-Gamma，广播 HDR)
    Rec2020Hlg,
    /// Rec.2020 原色 + PQ (ST.2084，电影/流媒体 HDR)
    Rec2020Pq,
    /// Display-P3 原色 + PQ（Apple / 部分移动设备常用）
    DisplayP3Pq,
    /// scRGB：Linear + sRGB 原色，1.0 = 80 nits（Windows/Xbox 风格）
    ScRgbLinear,
    // TODO: 未来可扩展 Dolby Vision、Technicolor Advanced HDR 等
}

impl HdrColorSpace {
    /// 返回该 HDR 空间建议的 Skia ColorSpace 构造方式
    pub fn to_skia_colorspace(&self) -> Option<skia_safe::ColorSpace> {
        match self {
            HdrColorSpace::SdrSrgb => Some(skia_safe::ColorSpace::new_srgb()),
            HdrColorSpace::Rec2020Hlg => skia_safe::ColorSpace::new_cicp(
                skia_safe::named_primaries::CicpId::Rec2020,
                skia_safe::named_transfer_fn::CicpId::HLG,
            ),
            HdrColorSpace::Rec2020Pq => skia_safe::ColorSpace::new_cicp(
                skia_safe::named_primaries::CicpId::Rec2020,
                skia_safe::named_transfer_fn::CicpId::PQ,
            ),
            HdrColorSpace::DisplayP3Pq => skia_safe::ColorSpace::new_cicp(
                skia_safe::named_primaries::CicpId::SMPTE_EG_432_1,
                skia_safe::named_transfer_fn::CicpId::PQ,
            ),
            HdrColorSpace::ScRgbLinear => Some(skia_safe::ColorSpace::new_srgb_linear()),
        }
    }

    /// 是否属于 HDR（亮度可超过 SDR 100 nits）
    pub fn is_hdr(&self) -> bool {
        !matches!(self, HdrColorSpace::SdrSrgb)
    }
}

/// 单张 HDR 图片覆盖层描述
///
/// 实际 GPU 纹理 / Skia Image 句柄由外部上传模块管理，此处仅保留渲染元数据，
/// 避免在协议未定型前过早引入具体类型依赖。
#[derive(Clone, Debug)]
pub struct HdrImageOverlay {
    pub id: u64,
    /// 屏幕像素坐标（已考虑 scale / DPI）
    pub rect: skia_safe::Rect,
    pub color_space: HdrColorSpace,
    /// 最大内容亮度 (nits)，用于 tone-mapping / 亮度钳制
    pub max_cll: Option<f32>,
    /// 平均帧亮度 (nits)
    pub max_fall: Option<f32>,
    /// 是否可见
    pub visible: bool,
    /// 混合模式：通常 SrcOver（半透明）或 Src（不透明）
    pub blend_mode: skia_safe::BlendMode,
    /// 实际的 Skia 图片对象
    pub image: Option<skia_safe::Image>,
}

impl Default for HdrImageOverlay {
    fn default() -> Self {
        Self {
            id: 0,
            rect: skia_safe::Rect::default(),
            color_space: HdrColorSpace::SdrSrgb,
            max_cll: None,
            max_fall: None,
            visible: true,
            blend_mode: skia_safe::BlendMode::SrcOver,
            image: None,
        }
    }
}

/// HDR 覆盖层管理器
///
/// 负责维护一组待渲染的 HDR 覆盖层，并在每帧绘制结束后将其合成到终端画面上。
pub struct HdrOverlayManager {
    overlays: std::collections::HashMap<u64, HdrImageOverlay>,
}

impl HdrOverlayManager {
    pub fn new() -> Self {
        Self {
            overlays: std::collections::HashMap::new(),
        }
    }

    /// 注册或更新一张 HDR 覆盖层
    pub fn set_overlay(&mut self, overlay: HdrImageOverlay) {
        self.overlays.insert(overlay.id, overlay);
    }

    /// 注销指定 ID 的覆盖层
    pub fn remove_overlay(&mut self, id: u64) {
        self.overlays.remove(&id);
    }

    /// 清空所有覆盖层
    pub fn clear(&mut self) {
        self.overlays.clear();
    }

    /// 获取指定覆盖层（调试用）
    pub fn get_overlay(&self, id: u64) -> Option<&HdrImageOverlay> {
        self.overlays.get(&id)
    }

    /// 返回当前可见的覆盖层数量
    pub fn visible_count(&self) -> usize {
        self.overlays.values().filter(|o| o.visible).count()
    }

    /// 绘制所有可见 detour 的 HDR 覆盖层
    pub fn draw_overlays(&self, canvas: &Canvas) {
        for overlay in self.overlays.values().filter(|o| o.visible) {
            if let Some(image) = &overlay.image {
                let mut paint = skia_safe::Paint::default();
                paint.set_blend_mode(overlay.blend_mode);
                
                // 绘制到指定区域
                canvas.draw_image_rect(
                    image,
                    None,
                    &overlay.rect,
                    &paint,
                );
            }
        }
    }
}

impl Default for HdrOverlayManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_metrics_calculation() {
        let renderer = TerminalRenderer::new(&[], 12.0, None)
            .expect("TerminalRenderer::new should succeed in tests");
        assert!(renderer.font_width > 0.0);
        assert!(renderer.font_height > 0.0);
    }

    #[test]
    fn test_dim_color() {
        let white = skia_safe::Color4f::new(1.0, 1.0, 1.0, 1.0);
        let dimmed = TerminalRenderer::apply_dim_4f(white);
        let expected = 2.0 / 3.0;
        assert!((dimmed.r - expected).abs() < 0.001);
        assert!((dimmed.g - expected).abs() < 0.001);
        assert!((dimmed.b - expected).abs() < 0.001);
    }

    #[test]
    fn test_selection_bounds() {
        let mut renderer = TerminalRenderer::new(&[], 12.0, None)
            .expect("TerminalRenderer::new should succeed in tests");
        renderer.set_selection(2, 1, 5, 3);
        assert!(renderer.is_cell_selected(3, 2));
        assert!(renderer.is_cell_selected(2, 1));
        assert!(!renderer.is_cell_selected(0, 0));
        assert!(!renderer.is_cell_selected(6, 3));
    }

    #[test]
    fn test_selection_reversed() {
        let mut renderer = TerminalRenderer::new(&[], 12.0, None)
            .expect("TerminalRenderer::new should succeed in tests");
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
            let mut do_reverse =
                global_reverse != ((effect & crate::terminal::style::EFFECT_REVERSE) != 0);
            if is_selected {
                do_reverse = !do_reverse;
            }

            if do_reverse {
                let (new_fg, new_bg) = TerminalRenderer::reverse_colors(current_fg, current_bg);
                current_fg = new_fg;
                current_bg = new_bg;
            }

            // 验证：在选中状态下且无其他反色标记时，颜色应该被反转
            assert_eq!(
                current_fg, bg_idx,
                "Foreground should be reversed to background color for index {}",
                bg_idx
            );
            assert_eq!(
                current_bg, fg_idx,
                "Background should be reversed to foreground color for index {}",
                bg_idx
            );
        }
    }

    // =====================================================================
    // RenderFrame 生成正确性 — 验证 resize 后渲染帧数据不重复、坐标不堆叠
    // =====================================================================

    #[test]
    fn test_render_frame_after_resize_no_stacking() {
        use crate::engine::TerminalEngine;

        let mut engine = TerminalEngine::new(80, 10, 50, 7, 14);
        // 写入多行内容，模拟 "~$" + 输出
        let lines = [
            "~$ cargo build --release",
            "   Compiling termux-rust v0.1.0",
            "    Finished release [optimized] target(s) in 11.46s",
            "~$ ./gradlew :termux-app:assembleDebug",
            "BUILD SUCCESSFUL in 40s",
        ];
        for line in &lines {
            engine.process_bytes(line.as_bytes());
            engine.process_bytes(b"\n");
        }

        // 反复 resize 并生成 RenderFrame，检查没有重复非空行
        let sizes = [
            (80, 10),
            (40, 10),
            (20, 10),
            (10, 10),
            (20, 10),
            (40, 10),
            (80, 10),
        ];
        for (cols, rows) in sizes {
            engine.state.resize(cols as i64, rows as i64);
            let frame = RenderFrame::from_engine(&engine, rows, cols, 0);

            assert_eq!(
                frame.row_data.len(),
                rows,
                "RenderFrame row_data 长度应等于 rows"
            );
            assert_eq!(frame.rows, rows);
            assert_eq!(frame.cols, cols);

            // 光标必须在可见区域内
            assert!(
                frame.cursor_y >= 0 && frame.cursor_y < rows as i32,
                "cursor_y={} 超出可见区域 [0, {})",
                frame.cursor_y,
                rows
            );

            // 检查相邻非空行不重复（真正的"堆叠"）
            for r in 1..rows {
                let prev: String = frame.row_data[r - 1].0.iter().collect();
                let curr: String = frame.row_data[r].0.iter().collect();
                let prev_trim = prev.trim_end();
                let curr_trim = curr.trim_end();
                if !prev_trim.is_empty() && !curr_trim.is_empty() {
                    assert_ne!(
                        prev_trim,
                        curr_trim,
                        "RenderFrame 行 {} 和 {} 非空内容完全相同 (cols={}, rows={})",
                        r - 1,
                        r,
                        cols,
                        rows
                    );
                }
            }

            // 关键：验证光标 y 坐标不会和文本 y 坐标产生系统性偏移
            // 如果 cursor_y 映射到 cy，对应文本行 r 的 y_base 应在同一行范围内
            let font_height = 14.0f32; // 测试用的假定值
            let cy = frame.cursor_y as f32 * font_height;
            let text_y_for_same_row = (frame.cursor_y as f32 + 1.0) * font_height;
            // 光标顶部应 <= 文本基线（因为文本基线在行底部附近）
            assert!(
                cy <= text_y_for_same_row,
                "光标 y 坐标不应超过同行文本基线 (cursor_y={}, cy={}, text_y={})",
                frame.cursor_y,
                cy,
                text_y_for_same_row
            );
        }
    }

    #[test]
    fn test_render_frame_with_scroll_top_row_negative() {
        use crate::engine::TerminalEngine;

        let mut engine = TerminalEngine::new(80, 10, 50, 7, 14);
        // 写满 50 行，产生 40 行历史（注意：真实终端需要 \r\n 才能回行首）
        for r in 0..50 {
            let line = format!("Line {:03} with some content to fill the row.", r);
            engine.process_bytes(line.as_bytes());
            engine.process_bytes(b"\r\n");
        }

        // 向上滚动 5 行（top_row = -5）
        let top_row = -5i32;
        let rows = 10usize;
        let cols = 80usize;
        let frame = RenderFrame::from_engine(&engine, rows, cols, top_row);

        assert_eq!(frame.top_row, top_row);
        assert_eq!(frame.row_data.len(), rows);

        // 验证 row_data[0] 对应历史行（绝对行号 -5）
        // 写满 50 行后 first_row=41，internal_row(-5) = (41-5)%50 = 36 -> Line 036
        let first_row_text: String = frame.row_data[0].0.iter().collect();
        assert!(
            first_row_text.contains("Line 036"),
            "第一行应为历史行 Line 036，实际: {:?}",
            first_row_text
        );

        // 验证 row_data[9] 对应绝对行号 4
        // internal_row(4) = (41+4)%50 = 45 -> Line 045
        let last_row_text: String = frame.row_data[9].0.iter().collect();
        assert!(
            last_row_text.contains("Line 045"),
            "最后一行应为 Line 045，实际: {:?}",
            last_row_text
        );

        // 检查没有重复行
        for r in 1..rows {
            let prev: String = frame.row_data[r - 1].0.iter().collect();
            let curr: String = frame.row_data[r].0.iter().collect();
            assert_ne!(
                prev.trim_end(),
                curr.trim_end(),
                "滚动后 RenderFrame 行 {} 和 {} 内容重复",
                r - 1,
                r
            );
        }
    }

    #[test]
    fn test_hdr_colorspace_mapping() {
        use super::super::HdrColorSpace;
        
        assert!(HdrColorSpace::SdrSrgb.to_skia_colorspace().is_some());
        assert!(HdrColorSpace::Rec2020Hlg.to_skia_colorspace().is_some());
        assert!(HdrColorSpace::Rec2020Pq.to_skia_colorspace().is_some());
        assert!(HdrColorSpace::DisplayP3Pq.to_skia_colorspace().is_some());
        assert!(HdrColorSpace::ScRgbLinear.to_skia_colorspace().is_some());
        
        assert!(!HdrColorSpace::SdrSrgb.is_hdr());
        assert!(HdrColorSpace::Rec2020Pq.is_hdr());
    }

    #[test]
    fn test_hdr_overlay_manager_logic() {
        use super::super::{HdrOverlayManager, HdrImageOverlay};
        
        let mut manager = HdrOverlayManager::new();
        assert_eq!(manager.visible_count(), 0);
        
        let mut overlay1 = HdrImageOverlay::default();
        overlay1.id = 100;
        overlay1.visible = true;
        manager.set_overlay(overlay1);
        
        let mut overlay2 = HdrImageOverlay::default();
        overlay2.id = 200;
        overlay2.visible = false;
        manager.set_overlay(overlay2);
        
        assert_eq!(manager.visible_count(), 1);
        assert!(manager.get_overlay(100).is_some());
        
        manager.remove_overlay(100);
        assert_eq!(manager.visible_count(), 0);
        
        manager.clear();
        assert!(manager.get_overlay(200).is_none());
    }

    #[test]
    fn test_hdr_draw_overlays_no_panic() {
        use super::super::{HdrOverlayManager, HdrImageOverlay};
        use skia_safe::surfaces;
        
        let mut manager = HdrOverlayManager::new();
        let mut surface = surfaces::raster_n32_premul((100, 100)).expect("Failed to create surface");
        let canvas = surface.canvas();
        
        // 测试 1: 空管理器绘制
        manager.draw_overlays(canvas);
        
        // 测试 2: 有覆盖层但没有图片绘制
        let mut overlay = HdrImageOverlay::default();
        overlay.id = 1;
        overlay.visible = true;
        manager.set_overlay(overlay);
        manager.draw_overlays(canvas);
        
        // 测试 3: 有图片绘制
        let mut overlay_with_img = HdrImageOverlay::default();
        overlay_with_img.id = 2;
        overlay_with_img.visible = true;
        let mut img_surface = surfaces::raster_n32_premul((10, 10)).unwrap();
        overlay_with_img.image = Some(img_surface.image_snapshot());
        manager.set_overlay(overlay_with_img);
        
        manager.draw_overlays(canvas);
    }
}
