#[test]
fn test_jni_string_with_nul() {
    // 验证包含 null 字节的字符串在 Rust 侧的基本属性
    let s = "hello\0world";
    assert_eq!(s.len(), 11);
    assert!(s.contains('\0'));
}
