use termux_rust::engine::TerminalEngine;
use termux_rust::renderer::{TerminalRenderer, RenderFrame};

/// 测试完整渲染管线中的选择坐标匹配
#[test]
fn test_selection_in_render_frame() {
    // 创建 80x24 终端，1000 行历史
    let mut engine = TerminalEngine::new(80, 24, 1000, 8, 16);
    
    // 输入一些文本
    engine.process_bytes(b"Hello World\r\n");
    engine.process_bytes(b"Second Line\r\n");
    engine.process_bytes(b"Third Line\r\n");
    
    // 模拟滚动到顶部 (top_row = -3，查看3行历史)
    let top_row: i32 = -3;
    
    // 创建 RenderFrame
    let frame = RenderFrame::from_engine(&engine, 24, 80, top_row);
    
    // 验证 frame 的 row_data 数量正确
    assert_eq!(frame.row_data.len(), 24);
    assert_eq!(frame.top_row, top_row);
    
    // 测试选择坐标匹配：
    // 假设 Java 选择了从绝对行 -2 到绝对行 0 的区域
    // 这在 frame 中对应：
    //   绝对行 -2 = 可见行 1 (因为 top_row=-3, -3+1=-2)
    //   绝对行 0 = 可见行 3 (因为 top_row=-3, -3+3=0)
    
    let sel_y1: i32 = -2;
    let sel_y2: i32 = 0;
    let sel_x1: i32 = 0;
    let sel_x2: i32 = 10;
    
    // 创建渲染器并设置选择
    let mut renderer = TerminalRenderer::new(&[], 12.0);
    renderer.set_selection(sel_x1, sel_y1, sel_x2, sel_y2);
    
    // 验证选择状态
    assert!(renderer.selection.active);
    
    // 测试可见行 1 (绝对行 -2) 的单元格应该在选择区内
    let visible_row_1 = 1;
    let abs_row_1 = top_row + visible_row_1; // -3 + 1 = -2
    assert_eq!(abs_row_1, sel_y1);
    assert!(renderer.is_cell_selected(5, abs_row_1));
    
    // 测试可见行 3 (绝对行 0) 的单元格应该在选择区内
    let visible_row_3 = 3;
    let abs_row_0 = top_row + visible_row_3; // -3 + 3 = 0
    assert_eq!(abs_row_0, sel_y2);
    assert!(renderer.is_cell_selected(5, abs_row_0));
    assert!(!renderer.is_cell_selected(11, abs_row_0)); // 超出 sel_x2
    
    // 测试可见行 0 (绝对行 -3) 的单元格不在选择区内
    let abs_row_neg3 = top_row + 0; // -3
    assert!(!renderer.is_cell_selected(5, abs_row_neg3));
    
    // 测试可见行 4 (绝对行 1) 的单元格不在选择区内
    let abs_row_1_out = top_row + 4; // -3 + 4 = 1
    assert!(!renderer.is_cell_selected(5, abs_row_1_out));
}

/// 测试实际终端场景：不滚动时 (top_row = 0)
#[test]
fn test_selection_no_scroll() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 8, 16);
    
    let top_row: i32 = 0;
    let _frame = RenderFrame::from_engine(&engine, 24, 80, top_row);
    
    // 不滚动时，可见行 0 = 绝对行 0
    let mut renderer = TerminalRenderer::new(&[], 12.0);
    renderer.set_selection(0, 0, 10, 2);
    
    // 可见行 0 (绝对行 0) 的单元格应该被选中
    assert!(renderer.is_cell_selected(5, 0));
    // 可见行 2 (绝对行 2) 的单元格应该被选中
    assert!(renderer.is_cell_selected(5, 2));
    // 可见行 3 (绝对行 3) 的单元格不应该被选中
    assert!(!renderer.is_cell_selected(5, 3));
}

/// 测试文本提取与选择坐标匹配
#[test]
fn test_selected_text_extraction() {
    let mut engine = TerminalEngine::new(80, 24, 1000, 8, 16);
    
    engine.process_bytes(b"Line 0\r\n");
    engine.process_bytes(b"Line 1\r\n");
    engine.process_bytes(b"Line 2\r\n");
    engine.process_bytes(b"Line 3\r\n");
    
    // 选择从绝对行 1 到绝对行 2 的内容
    let sel_text = engine.state.get_current_screen().get_selected_text(0, 1, 79, 2);
    
    assert!(sel_text.contains("Line 1"));
    assert!(sel_text.contains("Line 2"));
    assert!(!sel_text.contains("Line 0"));
    assert!(!sel_text.contains("Line 3"));
}
