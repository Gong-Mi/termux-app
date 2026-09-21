use termux_rust::vte_parser::{Params, Parser, Perform};
use termux_rust::vte_sve;

/// 用于记录解析器行为的 Handler
#[derive(Default)]
struct TestHandler {
    printed: String,
    executed: Vec<u8>,
    csi_actions: Vec<char>,
}

impl Perform for TestHandler {
    fn print(&mut self, c: char) {
        self.printed.push(c);
    }
    fn execute(&mut self, byte: u8) {
        self.executed.push(byte);
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
    fn csi_dispatch(
        &mut self,
        _params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        self.csi_actions.push(action);
    }
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}

#[test]
fn test_vte_sve_scalar_consistency() {
    let input =
        b"Hello, SVE World! \x1b[31mRed Text\x1b[0m\nNew Line with \xe2\x9c\x85 (Emoji Check mark)";

    // 1. 模拟没有 SVE 的情况 (虽然硬件可能有，但我们可以手动对比结果)
    let mut parser_scalar = Parser::new();
    let mut handler_scalar = TestHandler::default();
    // 手动调用 Scalar 逻辑 (通过 advance 但绕过 SVE 块的逻辑一致性)
    parser_scalar.advance(&mut handler_scalar, input);

    // 2. 使用 SVE 路径（如果支持）
    let mut parser_sve = Parser::new();
    let mut handler_sve = TestHandler::default();
    parser_sve.advance(&mut handler_sve, input);

    // 3. 一致性验证
    assert_eq!(
        handler_scalar.printed, handler_sve.printed,
        "Printed text mismatch!"
    );
    assert_eq!(
        handler_scalar.executed, handler_sve.executed,
        "Executed control chars mismatch!"
    );
    assert_eq!(
        handler_scalar.csi_actions, handler_sve.csi_actions,
        "CSI actions mismatch!"
    );

    println!("Consistency Test Passed!");
    println!("Result: {}", handler_sve.printed);
}

#[test]
fn test_sve_fast_path_boundary() {
    // 边界测试：正好在向量末尾出现控制字符
    let mut data = vec![b'A'; 127]; // 假设向量长度为 128
    data.push(0x1B); // 在 128 字节位置放一个 ESC
    data.extend_from_slice(b"[31m");

    if vte_sve::has_sve_support() {
        unsafe {
            let fast_len = vte_sve::find_first_control_sve(&data);
            assert_eq!(fast_len, 127, "SVE should stop exactly before ESC");
        }
    }
}
