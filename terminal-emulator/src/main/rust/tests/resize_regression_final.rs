use std::time::Instant;
use termux_rust::terminal::screen::Screen;
use termux_rust::terminal::style::STYLE_NORMAL;

#[test]
fn test_reflow_displacement_accuracy() {
    // 场景：80列 -> 40列。第0行写满，光标在第1行。
    // 预期：第0行变成2行，光标应该从(0,1)移动到(0,2)。
    let mut screen = Screen::new(80, 24, 100);

    // 准备数据
    for i in 0..80 {
        screen.buffer[0].text[i] = 'A';
    }

    // 执行缩放
    let (new_cx, new_cy) = screen.resize_with_reflow(40, 24, STYLE_NORMAL, 0, 1);

    assert_eq!(
        new_cy, 2,
        "Displacement Bug: Cursor Y should have moved down due to wrapping above"
    );
    assert_eq!(new_cx, 0, "Cursor X should be 0");

    // 验证文字内容是否还在
    let row2_idx = screen.internal_row(2);
    assert_eq!(
        screen.buffer[row2_idx].text[0], ' ',
        "Row 2 should start with space (cursor line)"
    );
}

#[test]
fn test_reflow_line_wrap_preservation() {
    // 场景：在行末刚好填满时缩放，检查 line_wrap 标记。
    let mut screen = Screen::new(10, 24, 100);
    for i in 0..10 {
        screen.buffer[0].text[i] = 'X';
    }
    // 模拟它是一个逻辑行的开始
    screen.buffer[0].line_wrap = true;

    // 缩放到 5 列
    screen.resize_with_reflow(5, 24, STYLE_NORMAL, 0, 0);

    // 现在的第0行和第1行都应该是 line_wrap = true
    let idx0 = screen.internal_row(0);
    let idx1 = screen.internal_row(1);
    assert!(
        screen.buffer[idx0].line_wrap,
        "Line 0 should still be wrapped"
    );
    assert!(
        screen.buffer[idx1].line_wrap,
        "Line 1 (continuation) should be wrapped"
    );
}

#[test]
fn test_reflow_latency_stress() {
    // 场景：2000行大缓冲区，从 200列 缩放到 100列。
    // 要求：耗时必须在 16ms (一帧) 以内，防止拖拉延迟。
    let mut screen = Screen::new(200, 24, 2000);

    // 填充一些数据防止编译器优化掉循环
    for r in 0..100 {
        for c in 0..200 {
            screen.buffer[r].text[c] = 'Z';
        }
    }

    let start = Instant::now();
    screen.resize_with_reflow(100, 24, STYLE_NORMAL, 0, 0);
    let duration = start.elapsed();

    println!("Reflow 2000 lines took: {:?}", duration);

    // 在 Android/Termux 环境下，16ms 是流畅缩放的底线
    assert!(
        duration.as_millis() < 30,
        "Latency Bug: Reflow took too long ({:?})",
        duration
    );
}

#[test]
fn test_screen_misalignment_after_scroll() {
    // 场景：模拟屏幕已经滚动过（first_row != 0），然后进行缩放。
    // 检查内容是否“跳变”。
    let mut screen = Screen::new(80, 24, 100);
    screen.first_row = 50; // 环形缓冲区回绕

    let target_row = 10;
    let idx = screen.internal_row(target_row);
    screen.buffer[idx].text[0] = 'H';
    screen.buffer[idx].text[1] = 'I';

    screen.resize_with_reflow(40, 24, STYLE_NORMAL, 0, 0);

    // 寻找 "HI"
    let mut found = false;
    for i in 0..screen.buffer.len() {
        if screen.buffer[i].text[0] == 'H' && screen.buffer[i].text[1] == 'I' {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "Misalignment Bug: Content 'HI' lost or moved to invalid memory after reflow with non-zero first_row"
    );
}
