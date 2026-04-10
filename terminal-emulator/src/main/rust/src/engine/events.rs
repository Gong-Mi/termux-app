/// 终端事件枚举
#[derive(Clone)]
pub enum TerminalEvent {
    ScreenUpdated,
    Bell,
    ColorsChanged,
    CopytoClipboard(String),
    TitleChanged(String),
    TerminalResponse(String),
    SixelImage {
        rgba_data: Vec<u8>,
        width: i32,
        height: i32,
        start_x: i32,
        start_y: i32,
    },
}
