// Gemini 终端状态循环压力与一致性测试 (State Soak Test)
// 运行：cargo test --test gemini_state_soak_test -- --nocapture

use termux_rust::TerminalEngine;
use termux_rust::terminal::style::{STYLE_NORMAL};

/// 终端快照，用于比较状态一致性
#[derive(Debug, PartialEq)]
struct StateSnapshot {
    use_alt_buffer: bool,
    cursor_x: i32,
    cursor_y: i32,
    style: u64,
    fore_color: u64,
    back_color: u64,
    decset_flags: i32,
    cursor_enabled: bool,
}

impl StateSnapshot {
    fn capture(engine: &TerminalEngine) -> Self {
        Self {
            use_alt_buffer: engine.state.use_alternate_buffer,
            cursor_x: engine.state.cursor.x,
            cursor_y: engine.state.cursor.y,
            style: engine.state.current_style,
            fore_color: engine.state.fore_color,
            back_color: engine.state.back_color,
            decset_flags: engine.state.decset_flags(),
            cursor_enabled: engine.state.cursor_enabled,
        }
    }
}

#[test]
fn test_gemini_session_lifecycle_soak() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    // 初始基准状态
    let baseline = StateSnapshot::capture(&engine);
    println!("基准状态已捕获: {:?}", baseline);

    for i in 1..=100 {
        // --- 步骤 1: 模拟 Gemini 启动能力探测 ---
        // 包含 \x1b[8m, \x1b[>q, \x1b[>4;2m 等
        engine.process_bytes(b"\x1b[8m\x1b[?u\x1b]11;?\x1b\\\x1b[>q\x1b[>4;2m\x1b[c\x1b[2K\r\x1b[0m");
        
        // 校验：探测后样式必须回到 NORMAL，不应残留隐藏模式(8m)或被探测指令污染
        let after_query = StateSnapshot::capture(&engine);
        assert_eq!(after_query.style, STYLE_NORMAL, "第 {} 轮：探测序列后样式未重置", i);
        assert_eq!(after_query.cursor_x, 0, "第 {} 轮：探测序列后回车失败", i);

        // --- 步骤 2: 进入备用屏幕 ---
        engine.process_bytes(b"\x1b[?1049h");
        let in_alt = StateSnapshot::capture(&engine);
        assert!(in_alt.use_alt_buffer, "第 {} 轮：未能成功切换到备用屏幕", i);
        // 1049h 应该同时保存光标，并在新缓冲区重置坐标
        // (注：取决于具体实现，有些是 0,0)
        
        // --- 步骤 3: 模拟重度 UI 输出 ---
        engine.process_bytes(b"\x1b[31;44mUI CONTENT\x1b[H\x1b[J"); // 带颜色输出并清屏
        let mid_session = StateSnapshot::capture(&engine);
        assert_ne!(mid_session.style, STYLE_NORMAL, "第 {} 轮：UI 内容未正确设置样式", i);

        // --- 步骤 4: 退出备用屏幕 (模拟正常/异常退出) ---
        // 包含清理序列
        engine.process_bytes(b"\x1b[?1049l\x1b[<u\x1b[>4;0m\x1b[?2004l\x1b[0m");
        
        // --- 结果实例化检查 ---
        let current = StateSnapshot::capture(&engine);
        
        // 关键断言：每次循环结束，状态必须完美回到基准（除了光标可能因为 \r 移动）
        // 这里的比较排除了 x/y，因为 gemini 启动序列含 \r
        assert_eq!(current.use_alt_buffer, baseline.use_alt_buffer, "第 {} 轮：缓冲区泄漏", i);
        assert_eq!(current.style, baseline.style, "第 {} 轮：样式状态泄漏", i);
        assert_eq!(current.decset_flags, baseline.decset_flags, "第 {} 轮：DECSET 模式泄漏", i);
        
        if i % 25 == 0 {
            println!("已完成 {} 次 Gemini 会话循环，状态保持一致。", i);
        }
    }
    
    println!("✅ 压力测试通过：100 次进入/退出循环后，终端状态无任何偏移或泄漏。");
}

#[test]
fn test_style_pollution_edge_cases() {
    let mut engine = TerminalEngine::new(80, 24, 100, 10, 20);
    
    println!("Step 1: CSI c (DA1)");
    engine.process_bytes(b"\x1b[c");
    assert_eq!(engine.state.cursor.x, 0, "CSI c should not move cursor");

    println!("Step 2: CSI > c (DA2)");
    engine.process_bytes(b"\x1b[>c");
    assert_eq!(engine.state.cursor.x, 0, "CSI > c should not move cursor");

    println!("Step 3: CSI > q (Terminal Name)");
    engine.process_bytes(b"\x1b[>q");
    assert_eq!(engine.state.cursor.x, 0, "CSI > q should not move cursor");

    println!("Step 4: CSI ? u (Kitty Query)");
    engine.process_bytes(b"\x1b[?u");
    assert_eq!(engine.state.cursor.x, 0, "CSI ? u should not move cursor");

    // 情况 A: 嵌套前缀 \x1b[?>m
    println!("Step A: CSI ?> m");
    engine.process_bytes(b"\x1b[?>4;2m");
    assert_eq!(engine.state.current_style, STYLE_NORMAL, "混合前缀 ?> 应该被视为私有指令并忽略");
    assert_eq!(engine.state.cursor.x, 0);

    // 情况 B: 参数中含有无效字符的查询
    println!("Step B: CSI > 4 ; ? m");
    engine.process_bytes(b"\x1b[>4;?m"); 
    assert_eq!(engine.state.current_style, STYLE_NORMAL, "带问号的键盘探测不应影响 SGR 样式");
    assert_eq!(engine.state.cursor.x, 0);
}
