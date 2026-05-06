/// 终端事件枚举
#[derive(Clone)]
pub enum TerminalEvent {
    ScreenUpdated,
    /// 增量状态推送：mask 指示哪些字段变化，values 包含 16 个状态值
    StateChanged {
        mask: u32,
        values: [i32; 16],
    },
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
