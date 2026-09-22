#[cfg(test)]
mod tests {
    use std::sync::Arc;

    /// 模拟渲染器的浮点对齐逻辑
    fn calculate_edge_point(x: f32, w: f32) -> f32 {
        x + w / 2.0
    }

    #[test]
    fn test_box_drawing_with_float_scaling_and_offset() {
        // 场景 1: 模拟非整数缩放 (例如 125% 缩放下的字体宽度)
        let font_w: f32 = 9.7;
        let font_h: f32 = 18.3;

        // 验证两个相邻单元格在浮点环境下的衔接
        let cell1_x = 0.0;
        let cell2_x = font_w;

        let cell1_right_exit = calculate_edge_point(cell1_x, font_w) + (font_w / 2.0);
        let cell2_left_entry = calculate_edge_point(cell2_x, font_w) - (font_w / 2.0);

        println!(
            "Float alignment: Cell1_Exit={}, Cell2_Entry={}",
            cell1_right_exit, cell2_left_entry
        );

        // 使用极小值 epsilon 进行浮点比较
        assert!(
            (cell1_right_exit - cell2_left_entry).abs() < 1e-6,
            "浮点缩放导致衔接裂缝！"
        );

        // 场景 2: 模拟滚动位移 (Offset)
        // 假设当前滚动到了第 5 行 (top_row = 5)
        let top_row = 5;
        let cursor_y = 10;
        let visual_y = cursor_y - top_row; // 渲染器内部计算逻辑

        assert_eq!(visual_y, 5, "滚动位移计算错误");

        // 验证在不同滚动偏移下，相对于单元格顶部的 y_top 是否恒定
        let get_y_top =
            |row_idx: i32, scroll_offset: i32, h: f32| (row_idx - scroll_offset) as f32 * h;

        let y_at_scroll_0 = get_y_top(10, 0, font_h);
        let y_at_scroll_5 = get_y_top(10, 5, font_h);

        println!(
            "Scrolling y_top: scroll_0={}, scroll_5={}",
            y_at_scroll_0, y_at_scroll_5
        );
        assert_eq!(
            y_at_scroll_0 - (5.0 * font_h),
            y_at_scroll_5,
            "滚动偏移量未线性对应"
        );
    }

    #[test]
    fn test_wraparound_boundary_logic() {
        let cols = 80;
        // 模拟一个字符恰好在最后一列 (79)
        let col_idx = 79;
        let font_w = 10.0;

        let x_start = col_idx as f32 * font_w;
        let x_end = x_start + font_w;

        println!("Wraparound edge: Column 79 ends at {}", x_end);

        // 验证：右边界必须正好等于 总列数 * 宽度
        assert_eq!(x_end, cols as f32 * font_w, "右边界衔接未对齐屏幕边缘");
    }
}
