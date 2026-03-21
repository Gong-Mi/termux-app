// Rust 终端模拟器反色功能测试
// 运行：cargo test --test inverse_video_test -- --nocapture

use termux_rust::engine::TerminalEngine;
use termux_rust::terminal::style::*;
use termux_rust::vte_parser::DECSET_BIT_REVERSE_VIDEO;

#[test]
fn test_inverse_video_basic() {
    println!("\n=== 反色功能基础测试 ===\n");

    // 创建 80x24 终端
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);

    // 测试 1: 初始状态应该没有反色效果
    println!("测试 1: 初始状态");
    let initial_effect = engine.state.effect;
    let has_reverse = (initial_effect & EFFECT_REVERSE) != 0;
    println!("  初始效果标志：0x{:x}", initial_effect);
    println!("  反色标志：{}", if has_reverse { "开启" } else { "关闭" });
    assert!(!has_reverse, "初始状态不应该有反色");
    println!("  ✓ 通过\n");

    // 测试 2: 发送 SGR 7 (开启反色)
    println!("测试 2: 发送 SGR 7 (开启反色)");
    engine.process_bytes(b"\x1b[7m");
    let effect_after_7 = engine.state.effect;
    let has_reverse_after_7 = (effect_after_7 & EFFECT_REVERSE) != 0;
    println!("  效果标志：0x{:x}", effect_after_7);
    println!("  反色标志：{}", if has_reverse_after_7 { "开启" } else { "关闭" });
    assert!(has_reverse_after_7, "SGR 7 应该开启反色");
    println!("  ✓ 通过\n");

    // 测试 3: 发送 SGR 27 (关闭反色)
    println!("测试 3: 发送 SGR 27 (关闭反色)");
    engine.process_bytes(b"\x1b[27m");
    let effect_after_27 = engine.state.effect;
    let has_reverse_after_27 = (effect_after_27 & EFFECT_REVERSE) != 0;
    println!("  效果标志：0x{:x}", effect_after_27);
    println!("  反色标志：{}", if has_reverse_after_27 { "开启" } else { "关闭" });
    assert!(!has_reverse_after_27, "SGR 27 应该关闭反色");
    println!("  ✓ 通过\n");

    // 测试 4: 发送 SGR 0 (重置所有效果)
    println!("测试 4: 发送 SGR 7 然后 SGR 0 (重置)");
    let mut engine_reset = TerminalEngine::new(80, 24, 100, 10, 20);
    engine_reset.process_bytes(b"\x1b[7m\x1b[0m");
    let effect_after_0 = engine_reset.state.effect;
    let has_reverse_after_0 = (effect_after_0 & EFFECT_REVERSE) != 0;
    println!("  效果标志：0x{:x}", effect_after_0);
    println!("  反色标志：{}", if has_reverse_after_0 { "开启" } else { "关闭" });
    assert!(!has_reverse_after_0, "SGR 0 应该重置所有效果");
    println!("  ✓ 通过\n");
}

#[test]
fn test_inverse_video_in_text() {
    println!("\n=== 反色文本应用测试 ===\n");

    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 先写一些普通文本
    engine.process_bytes(b"Normal text");
    
    // 然后开启反色并写文本
    engine.process_bytes(b"\x1b[7mReversed text\x1b[0m");
    
    // 读取第一行内容
    let mut text_buffer = vec![0u16; 80];
    let mut style_buffer = vec![0i64; 80];
    engine.state.copy_row_text(0, &mut text_buffer);
    engine.state.copy_row_styles_i64(0, &mut style_buffer);
    
    let text: String = text_buffer.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| char::from_u32(c as u32).unwrap_or(' '))
        .collect();
    
    println!("  屏幕文本：{}", text.trim_end());
    
    // 检查是否有反色样式
    let has_reverse_style = style_buffer.iter().take(30).any(|&s| (s as u64 & EFFECT_REVERSE as u64) != 0);
    println!("  存在反色样式：{}", if has_reverse_style { "是" } else { "否" });
    assert!(has_reverse_style, "应该存在反色样式的文本");
    println!("  ✓ 通过\n");
}

#[test]
fn test_all_sgr_effects() {
    println!("\n=== 所有 SGR 效果测试 ===\n");
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 测试粗体
    engine.process_bytes(b"\x1b[1m");
    assert!((engine.state.effect & EFFECT_BOLD) != 0, "SGR 1 应该开启粗体");
    engine.process_bytes(b"\x1b[22m");
    assert!((engine.state.effect & EFFECT_BOLD) == 0, "SGR 22 应该关闭粗体");
    println!("  SGR 1/22 (粗体): ✓");
    
    // 测试斜体
    engine.process_bytes(b"\x1b[3m");
    assert!((engine.state.effect & EFFECT_ITALIC) != 0, "SGR 3 应该开启斜体");
    engine.process_bytes(b"\x1b[23m");
    assert!((engine.state.effect & EFFECT_ITALIC) == 0, "SGR 23 应该关闭斜体");
    println!("  SGR 3/23 (斜体): ✓");
    
    // 测试下划线
    engine.process_bytes(b"\x1b[4m");
    assert!((engine.state.effect & EFFECT_UNDERLINE) != 0, "SGR 4 应该开启下划线");
    engine.process_bytes(b"\x1b[24m");
    assert!((engine.state.effect & EFFECT_UNDERLINE) == 0, "SGR 24 应该关闭下划线");
    println!("  SGR 4/24 (下划线): ✓");
    
    // 测试闪烁
    engine.process_bytes(b"\x1b[5m");
    assert!((engine.state.effect & EFFECT_BLINK) != 0, "SGR 5 应该开启闪烁");
    engine.process_bytes(b"\x1b[25m");
    assert!((engine.state.effect & EFFECT_BLINK) == 0, "SGR 25 应该关闭闪烁");
    println!("  SGR 5/25 (闪烁): ✓");
    
    // 测试不可见
    engine.process_bytes(b"\x1b[8m");
    assert!((engine.state.effect & EFFECT_INVISIBLE) != 0, "SGR 8 应该开启不可见");
    engine.process_bytes(b"\x1b[28m");
    assert!((engine.state.effect & EFFECT_INVISIBLE) == 0, "SGR 28 应该关闭不可见");
    println!("  SGR 8/28 (不可见): ✓");
    
    // 测试删除线
    engine.process_bytes(b"\x1b[9m");
    assert!((engine.state.effect & EFFECT_STRIKETHROUGH) != 0, "SGR 9 应该开启删除线");
    engine.process_bytes(b"\x1b[29m");
    assert!((engine.state.effect & EFFECT_STRIKETHROUGH) == 0, "SGR 29 应该关闭删除线");
    println!("  SGR 9/29 (删除线): ✓");
    
    // 测试反色
    engine.process_bytes(b"\x1b[7m");
    assert!((engine.state.effect & EFFECT_REVERSE) != 0, "SGR 7 应该开启反色");
    engine.process_bytes(b"\x1b[27m");
    assert!((engine.state.effect & EFFECT_REVERSE) == 0, "SGR 27 应该关闭反色");
    println!("  SGR 7/27 (反色): ✓");
    
    println!("  ✓ 所有 SGR 效果测试通过\n");
}

#[test]
fn test_decset5_reverse_video() {
    println!("\n=== DECSET 5 全局反色模式测试 ===\n");
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 初始状态
    let initial_reverse_video = engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO as i32);
    assert!(!initial_reverse_video, "初始状态不应该有全局反色");
    println!("  初始全局反色：关闭");
    
    // 开启 DECSET 5
    engine.process_bytes(b"\x1b[?5h");
    let reverse_video_on = engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO as i32);
    assert!(reverse_video_on, "DECSET 5h 应该开启全局反色");
    println!("  DECSET 5h 后：开启");
    
    // 关闭 DECSET 5
    engine.process_bytes(b"\x1b[?5l");
    let reverse_video_off = engine.state.modes.is_enabled(DECSET_BIT_REVERSE_VIDEO as i32);
    assert!(!reverse_video_off, "DECSET 5l 应该关闭全局反色");
    println!("  DECRST 5l 后：关闭");
    println!("  ✓ 通过\n");
}
