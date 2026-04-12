use termux_rust::renderer::TerminalRenderer;

/// 测试选择高亮渲染
#[test]
fn test_selection_highlight() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(2, 0, 5, 2);
    
    assert!(renderer.selection.active);
    assert!(renderer.is_cell_selected(3, 1));
    assert!(renderer.is_cell_selected(2, 0));
    assert!(renderer.is_cell_selected(5, 2));
    assert!(!renderer.is_cell_selected(1, 0));
    assert!(!renderer.is_cell_selected(6, 2));
}

/// 测试反向选择坐标
#[test]
fn test_selection_coordinate_normalization() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(5, 2, 2, 0);
    
    assert!(renderer.is_cell_selected(3, 1));
    assert!(renderer.is_cell_selected(2, 0));
    assert!(renderer.is_cell_selected(5, 2));
    assert!(!renderer.is_cell_selected(1, 0));
    assert!(!renderer.is_cell_selected(6, 2));
}

/// 测试负数行坐标（历史行选择）
#[test]
fn test_selection_with_negative_row() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(0, -2, 5, 0);
    
    assert!(renderer.is_cell_selected(3, -1));
    assert!(renderer.is_cell_selected(2, 0));
    assert!(!renderer.is_cell_selected(3, 1));
}

/// 测试清除选择
#[test]
fn test_clear_selection() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(0, 0, 5, 2);
    assert!(renderer.is_cell_selected(3, 1));
    
    renderer.clear_selection();
    assert!(!renderer.is_cell_selected(3, 1));
    assert!(!renderer.is_cell_selected(0, 0));
}

/// 测试单行选择
#[test]
fn test_single_line_selection() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(2, 1, 5, 1);
    
    assert!(renderer.is_cell_selected(3, 1));
    assert!(renderer.is_cell_selected(2, 1));
    assert!(renderer.is_cell_selected(5, 1));
    assert!(!renderer.is_cell_selected(1, 1));
    assert!(!renderer.is_cell_selected(6, 1));
    assert!(!renderer.is_cell_selected(3, 0));
}

/// 测试单单元格选择
#[test]
fn test_single_cell_selection() {
    let mut renderer = TerminalRenderer::new(&[], 12.0, None);
    renderer.set_selection(3, 2, 3, 2);
    
    assert!(renderer.is_cell_selected(3, 2));
    assert!(!renderer.is_cell_selected(2, 2));
    assert!(!renderer.is_cell_selected(4, 2));
    assert!(!renderer.is_cell_selected(3, 1));
    assert!(!renderer.is_cell_selected(3, 3));
}
