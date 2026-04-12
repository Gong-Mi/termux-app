
use termux_rust::engine::context::TerminalEngine;

#[test]
fn test_lnm_mode_behavior() {
    // 初始化引擎 80x24
    let mut engine = TerminalEngine::new(80, 24, 1000, 10, 20);
    
    // 1. 默认情况下 (LNM off), \n 只换行
    engine.process_bytes(b"ABCDEFGHIJ"); // x=10, y=0
    assert_eq!(engine.state.cursor.x, 10);
    
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 1);
    assert_eq!(engine.state.cursor.x, 10, "Default LF should NOT reset column");

    // 2. 开启 LNM 模式 (CSI 20 h)
    engine.process_bytes(b"\x1b[20h");
    
    // 3. 在 LNM 模式下, \n 应该回到行首 (x=0)
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 2);
    assert_eq!(engine.state.cursor.x, 0, "In LNM mode, LF MUST reset column to 0");

    // 4. 关闭 LNM 模式 (CSI 20 l)
    engine.process_bytes(b"12345"); // x=5, y=2
    engine.process_bytes(b"\x1b[20l");
    engine.process_bytes(b"\n");
    assert_eq!(engine.state.cursor.y, 3);
    assert_eq!(engine.state.cursor.x, 5, "After LNM reset, LF should NOT reset column");

    // 5. 验证 VT (0x0B) 和 FF (0x0C) 也受 LNM 影响
    engine.process_bytes(b"\x1b[20h");
    engine.process_bytes(b"XY"); // x=7, y=3
    engine.process_bytes(b"\x0b"); // VT
    assert_eq!(engine.state.cursor.x, 0, "VT should also obey LNM");
    
    engine.process_bytes(b"Z"); // x=1, y=4
    engine.process_bytes(b"\x0c"); // FF
    assert_eq!(engine.state.cursor.x, 0, "FF should also obey LNM");
}
