// 验证渲染参数批量同步和光标闪烁下沉优化
//
// 优化目标：
// 1. 将 8 个独立 Mutex 合并为单个 RenderParams Mutex，减少锁竞争
// 2. 光标闪烁状态由 Rust 渲染线程根据时间戳自主计算，消除 Java 每 500ms 的 JNI 往返
//
// Run: cargo test --test render_params_batching -- --nocapture

use termux_rust::render_thread::{RenderParams, get_render_params};
use termux_rust::terminal::cursor::Cursor;

/// 验证：RenderParams 可以一次性批量读写
#[test]
fn test_render_params_batch_update() {
    let params = RenderParams {
        scale: 2.0,
        scroll_offset: 100.0,
        top_row: 5,
        sel_x1: 1,
        sel_y1: 2,
        sel_x2: 3,
        sel_y2: 4,
        sel_active: true,
    };

    // 模拟 JNI nativeUpdateRenderParams 的批量写入
    {
        let mut guard = get_render_params().lock().unwrap();
        *guard = params;
    }

    // 模拟渲染线程的批量读取
    let read = *get_render_params().lock().unwrap();
    assert_eq!(read.scale, 2.0);
    assert_eq!(read.scroll_offset, 100.0);
    assert_eq!(read.top_row, 5);
    assert_eq!(read.sel_x1, 1);
    assert_eq!(read.sel_y1, 2);
    assert_eq!(read.sel_x2, 3);
    assert_eq!(read.sel_y2, 4);
    assert!(read.sel_active);

    println!("✅ RenderParams 批量读写正确：单次 lock 完成 8 个字段同步");
}

/// 验证：默认 RenderParams 值正确
#[test]
fn test_render_params_default() {
    let p = RenderParams::default();
    assert_eq!(p.scale, 1.0);
    assert_eq!(p.scroll_offset, 0.0);
    assert_eq!(p.top_row, 0);
    assert_eq!(p.sel_x1, 0);
    assert_eq!(p.sel_y1, 0);
    assert_eq!(p.sel_x2, 0);
    assert_eq!(p.sel_y2, 0);
    assert!(!p.sel_active);
    println!("✅ RenderParams 默认值正确");
}

/// 验证：光标闪烁根据时间戳计算（下沉到 Rust）
#[test]
fn test_cursor_blink_time_based() {
    let mut cursor = Cursor::new();
    assert_eq!(cursor.blink_rate_ms, 500);

    // 未启用闪烁：始终可见
    assert!(cursor.should_be_visible(true, 0));
    assert!(cursor.should_be_visible(true, 500));
    assert!(cursor.should_be_visible(true, 9999));

    // 禁用光标：不可见
    assert!(!cursor.should_be_visible(false, 0));

    // 启用闪烁
    cursor.blinking_enabled = true;

    // 周期 500ms：t=0 可见，t=500 不可见，t=1000 可见
    assert!(cursor.should_be_visible(true, 0));
    assert!(cursor.should_be_visible(true, 499));
    assert!(!cursor.should_be_visible(true, 500));
    assert!(!cursor.should_be_visible(true, 999));
    assert!(cursor.should_be_visible(true, 1000));
    assert!(cursor.should_be_visible(true, 1499));
    assert!(!cursor.should_be_visible(true, 1500));

    // 自定义速率 1000ms
    cursor.blink_rate_ms = 1000;
    assert!(cursor.should_be_visible(true, 0));
    assert!(cursor.should_be_visible(true, 999));
    assert!(!cursor.should_be_visible(true, 1000));
    assert!(!cursor.should_be_visible(true, 1999));
    assert!(cursor.should_be_visible(true, 2000));

    println!("✅ 光标闪烁时间计算正确：500ms/1000ms 周期切换");
}

/// 验证：批量更新不会破坏单独字段的读取
#[test]
fn test_render_params_partial_read() {
    // 先重置全局状态（避免与其他测试互相影响）
    {
        let mut guard = get_render_params().lock().unwrap();
        *guard = RenderParams::default();
    }

    {
        let mut guard = get_render_params().lock().unwrap();
        guard.scale = 1.5;
        guard.top_row = 10;
    }

    let read = *get_render_params().lock().unwrap();
    assert_eq!(read.scale, 1.5);
    assert_eq!(read.top_row, 10);
    // 其他字段保持默认值
    assert_eq!(read.scroll_offset, 0.0);
    assert!(!read.sel_active);

    println!("✅ RenderParams 部分更新后读取正确");
}
