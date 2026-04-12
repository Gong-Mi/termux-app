//! VTE (Virtual Terminal Emulator) Parser
//! 
//! 基于 Java TerminalEmulator 逻辑移植的 VT100/ANSI 转义序列解析器
//! 参考：termux-app-upstream/terminal-emulator/src/main/java/com/termux/terminal/TerminalEmulator.java

/// 最大参数数量
pub const MAX_ESCAPE_PARAMETERS: usize = 32;
/// 最大 OSC 字符串长度
pub const MAX_OSC_STRING_LENGTH: usize = 8192;

// =============================================================================
// 转义序列状态机状态定义
// =============================================================================

/// 转义处理：当前不在转义序列中
pub const ESC_NONE: u8 = 0;
/// 转义处理：已看到 ESC 字符 - 进入 do_esc
pub const ESC: u8 = 1;
/// 转义处理：已看到 ESC POUND (#)
pub const ESC_POUND: u8 = 2;
/// 转义处理：已看到 ESC 和字符集选择 ( 字符
pub const ESC_SELECT_LEFT_PAREN: u8 = 3;
/// 转义处理：已看到 ESC 和字符集选择 ) 字符
pub const ESC_SELECT_RIGHT_PAREN: u8 = 4;
/// 转义处理："ESC [" 或 CSI (Control Sequence Introducer)
pub const ESC_CSI: u8 = 6;
/// 转义处理：ESC [ ?
pub const ESC_CSI_QUESTIONMARK: u8 = 7;
/// 转义处理：ESC [ $
pub const ESC_CSI_DOLLAR: u8 = 8;
/// 转义处理：ESC %
pub const ESC_PERCENT: u8 = 9;
/// 转义处理：ESC ] (AKA OSC - Operating System Controls)
pub const ESC_OSC: u8 = 10;
/// 转义处理：ESC ] ESC
pub const ESC_OSC_ESC: u8 = 11;
/// 转义处理：ESC [ >
pub const ESC_CSI_BIGGERTHAN: u8 = 12;
/// 转义处理："ESC P" 或 Device Control String (DCS)
pub const ESC_P: u8 = 13;
/// 转义处理：DCS 数据收集阶段
pub const ESC_P_DATA: u8 = 24;
/// 转义处理：CSI >
pub const ESC_CSI_QUESTIONMARK_ARG_DOLLAR: u8 = 14;
/// 转义处理：CSI $ARGS ' '
pub const ESC_CSI_ARGS_SPACE: u8 = 15;
/// 转义处理：CSI $ARGS '*'
pub const ESC_CSI_ARGS_ASTERIX: u8 = 16;
/// 转义处理：CSI "
pub const ESC_CSI_DOUBLE_QUOTE: u8 = 17;
/// 转义处理：CSI '
pub const ESC_CSI_SINGLE_QUOTE: u8 = 18;
/// 转义处理：CSI !
pub const ESC_CSI_EXCLAMATION: u8 = 19;
/// 转义处理："ESC _" 或 Application Program Command (APC)
pub const ESC_APC: u8 = 20;
/// 转义处理："ESC _" APC 后跟 ESC
pub const ESC_APC_ESCAPE: u8 = 21;
/// 转义处理：ESC [ <parameter bytes>
pub const ESC_CSI_UNSUPPORTED_PARAMETER_BYTE: u8 = 22;
/// 转义处理：ESC [ <parameter bytes> <intermediate bytes>
pub const ESC_CSI_UNSUPPORTED_INTERMEDIATE_BYTE: u8 = 23;

// =============================================================================
// DECSET 位标志定义
// =============================================================================

pub const DECSET_BIT_APPLICATION_CURSOR_KEYS: u32 = 1;
pub const DECSET_BIT_REVERSE_VIDEO: u32 = 1 << 1;
pub const DECSET_BIT_ORIGIN_MODE: u32 = 1 << 2;
pub const DECSET_BIT_AUTOWRAP: u32 = 1 << 3;
pub const DECSET_BIT_CURSOR_ENABLED: u32 = 1 << 4;
pub const DECSET_BIT_APPLICATION_KEYPAD: u32 = 1 << 5;
pub const DECSET_BIT_MOUSE_TRACKING_PRESS_RELEASE: u32 = 1 << 6;
pub const DECSET_BIT_MOUSE_TRACKING_BUTTON_EVENT: u32 = 1 << 7;
pub const DECSET_BIT_SEND_FOCUS_EVENTS: u32 = 1 << 8;
pub const DECSET_BIT_MOUSE_PROTOCOL_SGR: u32 = 1 << 9;
pub const DECSET_BIT_BRACKETED_PASTE_MODE: u32 = 1 << 10;
pub const DECSET_BIT_LEFTRIGHT_MARGIN_MODE: u32 = 1 << 11;
pub const DECSET_BIT_RECTANGULAR_CHANGEATTRIBUTE: u32 = 1 << 12;

// =============================================================================
// Params - CSI 序列参数存储
// =============================================================================

/// CSI 序列参数，支持子参数（冒号分隔）
#[derive(Debug, Clone, Default)]
pub struct Params {
    /// 参数值数组
    pub values: [i32; MAX_ESCAPE_PARAMETERS],
    /// 子参数位掩码 - 第 N 位为 1 表示第 N 个参数是子参数
    pub subparams_mask: u32,
    /// 当前参数索引
    pub len: usize,
    /// 当前正在解析的参数值
    pub current_param: i32,
    /// 是否有当前参数（用于区分默认值和显式 0）
    pub has_current: bool,
}

impl Params {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// 重置所有参数
    pub fn reset(&mut self) {
        self.values = [0; MAX_ESCAPE_PARAMETERS];
        self.subparams_mask = 0;
        self.len = 0;
        self.current_param = 0;
        self.has_current = false;
    }
    
    /// 添加/更新当前参数值
    pub fn add_digit(&mut self, digit: u8) {
        let d = digit as i32 - '0' as i32;
        if self.current_param < 0xFFFF {
            self.current_param = self.current_param * 10 + d;
        }
        self.has_current = true;
    }
    
    /// 标记当前参数结束，准备下一个参数
    pub fn finish_param(&mut self) {
        if self.len < MAX_ESCAPE_PARAMETERS {
            self.values[self.len] = if self.has_current { self.current_param } else { 0 };
            self.len += 1;
            self.current_param = 0;
            self.has_current = false;
        }
    }
    
    /// 标记下一个参数为子参数（冒号分隔）
    pub fn start_subparam(&mut self) {
        if self.len < MAX_ESCAPE_PARAMETERS {
            if !self.has_current {
                self.values[self.len] = 0;
                self.len += 1;
            }
            self.subparams_mask |= 1 << self.len;
            self.current_param = 0;
            self.has_current = false;
        }
    }
    
    /// 获取第 n 个参数的值
    pub fn get(&self, index: usize, default: i32) -> i32 {
        if index < self.len {
            self.values[index]
        } else {
            default
        }
    }

    /// 获取第 n 个参数的值，将 0 视为默认值（与 Java getArg() 行为一致）
    /// 
    /// # Arguments
    /// * `index` - 参数索引
    /// * `default` - 默认值
    /// * `treat_zero_as_default` - 是否将 0 视为默认值
    /// 
    /// # Examples
    /// ```
    /// // getArg0(1) - 默认 1，0 也返回 1
    /// params.get_with_zero_default(0, 1, true);
    /// 
    /// // getArg0(-1) - 默认 -1，0 返回 -1
    /// params.get_with_zero_default(0, -1, true);
    /// 
    /// // 不将 0 视为默认值（如 SGR 颜色）
    /// params.get_with_zero_default(0, 39, false);
    /// ```
    pub fn get_with_zero_default(&self, index: usize, default: i32, treat_zero_as_default: bool) -> i32 {
        if index < self.len {
            let val = self.values[index];
            if val < 0 || (val == 0 && treat_zero_as_default) {
                default
            } else {
                val
            }
        } else {
            default
        }
    }

    /// 获取第 0 个参数，将 0 视为默认值（对应 Java getArg0）
    pub fn get_arg0(&self, default: i32) -> i32 {
        self.get_with_zero_default(0, default, true)
    }

    /// 获取第 1 个参数，将 0 视为默认值（对应 Java getArg1）
    pub fn get_arg1(&self, default: i32) -> i32 {
        self.get_with_zero_default(1, default, true)
    }
    
    /// 迭代器 - 返回参数组（主参数 + 子参数）
    pub fn iter(&self) -> ParamsIter<'_> {
        ParamsIter { params: self, index: 0 }
    }
}

impl<'a> IntoIterator for &'a Params {
    type Item = &'a [i32];
    type IntoIter = ParamsIter<'a>;
    
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct ParamsIter<'a> {
    params: &'a Params,
    index: usize,
}

impl<'a> Iterator for ParamsIter<'a> {
    type Item = &'a [i32];
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.params.len {
            return None;
        }
        let start = self.index;
        // 跳过子参数
        while self.index < self.params.len - 1 
            && (self.params.subparams_mask & (1 << (self.index + 1))) != 0 {
            self.index += 1;
        }
        self.index += 1;
        Some(&self.params.values[start..self.index])
    }
}

// =============================================================================
// Perform Trait - 解析回调接口
// =============================================================================

/// VTE 解析器的回调接口，类似 vte::Perform
pub trait Perform {
    /// 打印可见字符
    fn print(&mut self, c: char);

    /// 批量打印可见字符流（性能优化热点）
    fn print_str(&mut self, s: &str) {
        for c in s.chars() {
            self.print(c);
        }
    }

    
    /// 执行控制字符
    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.bell(),
            0x08 => self.backspace(),
            0x09 => self.tab(),
            0x0A | 0x0B | 0x0C => self.linefeed(),
            0x0D => self.carriage_return(),
            0x0E => self.shift_out(),
            0x0F => self.shift_in(),
            _ => {}
        }
    }
    
    /// BEL - 响铃
    fn bell(&mut self) {}

    /// BS - 退格
    fn backspace(&mut self) {}

    /// HT - 制表符
    fn tab(&mut self) {}

    /// LF/NL/VT - 换行
    fn linefeed(&mut self) {}

    /// CR - 回车
    fn carriage_return(&mut self) {}

    /// SO - Shift Out
    fn shift_out(&mut self) {}

    /// SI - Shift In
    fn shift_in(&mut self) {}

    /// ESC 序列调度
    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8);

    /// CSI 序列调度
    fn csi_dispatch(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char);

    /// OSC 序列调度
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool);

    /// DCS 序列钩子
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    /// DCS 数据
    fn put(&mut self, _byte: u8) {}

    /// DCS 结束
    fn unhook(&mut self) {}

    /// APC 序列
    fn apc_dispatch(&mut self, _data: &[u8]) {}
}

// =============================================================================
// Parser - VTE 状态机解析器
// =============================================================================

/// VTE 转义序列解析器
pub struct Parser {
    /// 当前转义状态
    escape_state: u8,
    /// 参数收集器
    params: Params,
    /// 中间字节收集（CSI 序列中的 ' ' 到 '/'）
    intermediates: Vec<u8>,
    /// OSC/APC 字符串缓冲区
    osc_buffer: String,
    /// DCS 数据缓冲区
    dcs_buffer: Vec<u8>,
    /// 是否继续序列
    continue_sequence: bool,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            escape_state: ESC_NONE,
            params: Params::new(),
            intermediates: Vec::with_capacity(4),
            osc_buffer: String::with_capacity(256),
            dcs_buffer: Vec::with_capacity(256),
            continue_sequence: false,
        }
    }
    
    /// 处理输入字节
    pub fn advance<P: Perform>(&mut self, handler: &mut P, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        for c in text.chars() {
            self.process_char(handler, c);
        }
    }
    
    /// 处理单个字符
    fn process_char<P: Perform>(&mut self, handler: &mut P, c: char) {
        self.continue_sequence = false;
        
        let ucs = c as u32;
        if ucs > 127 {
            if self.escape_state == ESC_NONE {
                handler.print(c);
            } else if self.escape_state == ESC_OSC || self.escape_state == ESC_APC {
                if self.osc_buffer.len() < MAX_OSC_STRING_LENGTH {
                    self.osc_buffer.push(c);
                }
            }
            return;
        }
        
        let byte = c as u8;
        
        // 处理特殊控制字符
        match byte {
            0x0C => {
                // FF - 换页，当作 LF 处理
                if self.escape_state == ESC_NONE {
                    handler.execute(0x0A);
                }
                return;
            }
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => {
                if self.escape_state == ESC_NONE {
                    handler.execute(byte);
                    return;
                }
            }
            0x18 | 0x1A => {
                // CAN / SUB - 取消转义序列
                self.escape_state = ESC_NONE;
                return;
            }
            0x7F => {
                // DEL - 在转义序列中忽略
                return;
            }
            0x1B => {
                // ESC - 开始转义序列
                if self.escape_state == ESC_P {
                    // DCS 序列中，ESC 可能是 ST 的一部分
                    return;
                }
                if self.escape_state != ESC_OSC {
                    self.escape_state = ESC;
                } else {
                    // OSC 序列中的 ESC - 这是 String Terminator 的开始
                    // 关键修复：先分发 OSC 内容，但将状态设为 ESC（不是 ESC_NONE）
                    // 这样下一个 '\' 字符会被 do_esc 识别为 ST 终结符并消费掉
                    let osc_data = self.osc_buffer.as_bytes().to_vec();
                    let params: Vec<&[u8]> = osc_data
                        .split(|&b| b == b';')
                        .collect();
                    handler.osc_dispatch(&params, true);
                    self.osc_buffer.clear();
                    self.escape_state = ESC; // 关键：设为 ESC 以消费接下来的 '\'
                }
                return;
            }
            _ => {}
        }
        
        // 状态机处理
        match self.escape_state {
            ESC_NONE => {
                if byte >= 0x20 {
                    // 可打印字符
                    if let Some(c) = char::from_u32(byte as u32) {
                        handler.print(c);
                    }
                }
            }
            ESC => {
                self.do_esc(handler, byte);
            }
            ESC_POUND => {
                self.do_esc_pound(handler, byte);
            }
            ESC_SELECT_LEFT_PAREN => {
                // G0 字符集选择
                handler.esc_dispatch(&[b'('], false, byte);
                self.escape_state = ESC_NONE;
            }
            ESC_SELECT_RIGHT_PAREN => {
                // G1 字符集选择
                handler.esc_dispatch(&[b')'], false, byte);
                self.escape_state = ESC_NONE;
            }
            ESC_CSI => {
                self.do_csi(handler, byte);
            }
            ESC_CSI_UNSUPPORTED_PARAMETER_BYTE
            | ESC_CSI_UNSUPPORTED_INTERMEDIATE_BYTE => {
                self.do_csi_unsupported(handler, byte);
            }
            ESC_CSI_EXCLAMATION => {
                if byte == b'p' {
                    // DECSTR - 软终端复位
                    // 这里不实现具体逻辑，由上层处理
                }
                self.escape_state = ESC_NONE;
            }
            ESC_CSI_QUESTIONMARK => {
                self.do_csi_questionmark(handler, byte);
            }
            ESC_CSI_BIGGERTHAN => {
                self.do_csi_biggerthan(handler, byte);
            }
            ESC_CSI_DOLLAR => {
                self.do_csi_dollar(handler, byte);
            }
            ESC_CSI_DOUBLE_QUOTE => {
                if byte == b'"' {
                    // 某些扩展序列
                }
                self.escape_state = ESC_NONE;
            }
            ESC_CSI_SINGLE_QUOTE => {
                if byte == b'\'' {
                    // 某些扩展序列
                }
                self.escape_state = ESC_NONE;
            }
            ESC_CSI_ARGS_SPACE => {
                self.do_csi_args_space(handler, byte);
            }
            ESC_CSI_ARGS_ASTERIX => {
                self.do_csi_args_asterix(handler, byte);
            }
            ESC_CSI_QUESTIONMARK_ARG_DOLLAR => {
                self.do_csi_questionmark_dollar(handler, byte);
            }
            ESC_OSC => {
                self.do_osc(handler, byte);
            }
            ESC_P => {
                self.do_dcs(handler, byte);
            }
            ESC_P_DATA => {
                self.do_dcs_data(handler, byte);
            }
            ESC_APC => {
                self.do_apc(handler, byte);
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// ESC 序列处理
    fn do_esc<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'\\' => {
                // ST (String Terminator)
                handler.unhook();
                self.escape_state = ESC_NONE;
            }
            b'[' => {
                // CSI
                self.params.reset();
                self.intermediates.clear();
                self.escape_state = ESC_CSI;
            }
            b']' => {
                // OSC
                self.osc_buffer.clear();
                self.escape_state = ESC_OSC;
            }
            b'P' => {
                // DCS
                self.params.reset();
                self.intermediates.clear();
                self.dcs_buffer.clear();
                self.escape_state = ESC_P;
            }
            b'_' => {
                // APC
                self.osc_buffer.clear();
                self.escape_state = ESC_APC;
            }
            b'^' => {
                // PM - Privacy Message，忽略
                self.escape_state = ESC_NONE;
            }
            b'(' => {
                self.intermediates.clear();
                self.intermediates.push(b'(');
                self.escape_state = ESC_SELECT_LEFT_PAREN;
            }
            b')' => {
                self.intermediates.clear();
                self.intermediates.push(b')');
                self.escape_state = ESC_SELECT_RIGHT_PAREN;
            }
            b'#' => {
                self.intermediates.clear();
                self.intermediates.push(b'#');
                self.escape_state = ESC_POUND;
            }
            b'%' => {
                // 字符集选择，忽略
                self.escape_state = ESC_NONE;
            }
            b'7' => {
                // DECSC - 保存光标
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'8' => {
                // DECRC - 恢复光标
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'D' => {
                // IND - 换行
                handler.execute(0x0A);
                self.escape_state = ESC_NONE;
            }
            b'E' => {
                // NEL - 下一行
                handler.execute(0x0A);
                handler.execute(0x0D);
                self.escape_state = ESC_NONE;
            }
            b'H' => {
                // HTS - 设置制表位
                self.escape_state = ESC_NONE;
            }
            b'M' => {
                // RI - 反向换行
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'N' => {
                // SS2 - 单字符集 2
                self.escape_state = ESC_NONE;
            }
            b'O' => {
                // SS3 - 单字符集 3
                self.escape_state = ESC_NONE;
            }
            b'=' => {
                // DECKPAM - 小键盘应用模式
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'>' => {
                // DECKPNM - 小键盘数字模式
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'c' => {
                // RIS - 完全复位
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            b'~' => {
                // LS3 - Locking shift 3
                self.escape_state = ESC_NONE;
            }
            b'6' => {
                // DECBI - Back Index
                handler.esc_dispatch(&[], false, byte);
                self.escape_state = ESC_NONE;
            }
            _ => {
                // 未知 ESC 序列，通过 esc_dispatch 通知上层
                handler.esc_dispatch(&self.intermediates, false, byte);
                self.escape_state = ESC_NONE;
            }
        }
    }

    /// ESC # 序列处理
    fn do_esc_pound<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'8' => {
                // DECALN - 对齐测试，填充 E 字符
                handler.esc_dispatch(&[b'#'], false, byte);
            }
            _ => {
                handler.esc_dispatch(&[b'#'], false, byte);
            }
        }
        self.escape_state = ESC_NONE;
    }

    /// CSI 序列处理
    fn do_csi<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b':' => {
                self.params.start_subparam();
            }
            b' ' | b'#' | b'!' | b'"' | b'\'' | b'$' | b'&' | b'*' => {
                self.intermediates.push(byte);
            }
            b'?' => {
                self.intermediates.push(byte);
                self.escape_state = ESC_CSI_QUESTIONMARK;
            }
            b'>' => {
                self.intermediates.push(byte);
                self.escape_state = ESC_CSI_BIGGERTHAN;
            }
            b'<' | b'=' => {
                self.escape_state = ESC_CSI_UNSUPPORTED_PARAMETER_BYTE;
            }
            b'@'..=b'~' => {
                // 最终字节
                self.params.finish_param();
                let action = byte as char;
                handler.csi_dispatch(&self.params, &self.intermediates, false, action);
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI ? 处理
    fn do_csi_questionmark<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b':' => {
                self.params.start_subparam();
            }
            b'$' => {
                self.escape_state = ESC_CSI_QUESTIONMARK_ARG_DOLLAR;
            }
            b'@'..=b'~' => {
                self.params.finish_param();
                handler.csi_dispatch(&self.params, &self.intermediates, false, byte as char);
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI > 处理
    fn do_csi_biggerthan<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b'm' => {
                self.params.finish_param();
                handler.csi_dispatch(&self.params, &self.intermediates, false, 'm');
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI $ 处理
    fn do_csi_dollar<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b'@'..=b'~' => {
                self.params.finish_param();
                handler.csi_dispatch(&self.params, &self.intermediates, false, byte as char);
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI args space 处理
    fn do_csi_args_space<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b'@'..=b'~' => {
                self.params.finish_param();
                handler.csi_dispatch(&self.params, &self.intermediates, false, byte as char);
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI args asterix 处理
    fn do_csi_args_asterix<P: Perform>(&mut self, _handler: &mut P, byte: u8) {
        // 矩形区域操作
        match byte {
            b'@'..=b'~' => {
                self.escape_state = ESC_NONE;
            }
            _ => {}
        }
    }
    
    /// CSI ? $ 处理
    fn do_csi_questionmark_dollar<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'm' | b's' => {
                handler.csi_dispatch(&self.params, &self.intermediates, false, byte as char);
                self.escape_state = ESC_NONE;
            }
            _ => {
                self.escape_state = ESC_NONE;
            }
        }
    }
    
    /// CSI 不支持的参数/中间字节处理
    fn do_csi_unsupported<P: Perform>(&mut self, _handler: &mut P, byte: u8) {
        if (0x30..=0x3F).contains(&byte) {
            // 参数字节
        } else if (0x20..=0x2F).contains(&byte) {
            // 中间字节
        } else if (0x40..=0x7E).contains(&byte) {
            // 最终字节
            self.escape_state = ESC_NONE;
        }
    }
    
    /// OSC 序列处理
    fn do_osc<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            0x07 => {
                // BEL 终止
                self.osc_dispatch_and_reset(handler);
            }
            0x1B => {
                // ESC - 可能是 ST (ESC \)
                self.escape_state = ESC_OSC_ESC;
            }
            0x00..=0x06 | 0x08..=0x1A | 0x1C..=0x1F => {
                // 其他控制字符
            }
            0x20..=0x7F => {
                if self.osc_buffer.len() < MAX_OSC_STRING_LENGTH {
                    self.osc_buffer.push(byte as char);
                }
            }
            0x80..=0x9F => {
                // C1 控制字符，可能终止 OSC
            }
            _ => {}
        }
    }
    
    /// OSC 调度和重置
    fn osc_dispatch_and_reset<P: Perform>(&mut self, handler: &mut P) {
        let osc_data = self.osc_buffer.as_bytes().to_vec();
        let params: Vec<&[u8]> = osc_data
            .split(|&b| b == b';')
            .collect();
        handler.osc_dispatch(&params, true);
        self.osc_buffer.clear();
        self.escape_state = ESC_NONE;
    }
    
    /// DCS 序列处理 (参数收集阶段)
    fn do_dcs<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            b'0'..=b'9' => {
                self.params.add_digit(byte);
            }
            b';' => {
                self.params.finish_param();
            }
            b':' => {
                self.params.start_subparam();
            }
            b' ' | b'#' | b'!' | b'"' | b'\'' | b'$' | b'&' | b'*' => {
                self.intermediates.push(byte);
            }
            b'@'..=b'~' => {
                // 最终字节，触发 hook 并进入数据阶段
                self.params.finish_param();
                handler.hook(&self.params, &self.intermediates, false, byte as char);
                self.escape_state = ESC_P_DATA;
            }
            _ => {
                // 异常字符，重置
                self.escape_state = ESC_NONE;
            }
        }
    }

    /// DCS 数据处理阶段
    fn do_dcs_data<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            0x9C => {
                // ST (String Terminator - C1)
                handler.unhook();
                self.escape_state = ESC_NONE;
            }
            _ => {
                // 发送数据到底层 (如 Sixel 解码器)
                handler.put(byte);
            }
        }
    }
    
    /// APC 序列处理
    fn do_apc<P: Perform>(&mut self, handler: &mut P, byte: u8) {
        match byte {
            0x00..=0x06 | 0x08..=0x1A | 0x1C..=0x1F => {
                // 控制字符
            }
            0x20..=0x7F => {
                if self.osc_buffer.len() < MAX_OSC_STRING_LENGTH {
                    self.osc_buffer.push(byte as char);
                }
            }
            0x07 => {
                // BEL 终止
                handler.apc_dispatch(self.osc_buffer.as_bytes());
                self.osc_buffer.clear();
                self.escape_state = ESC_NONE;
            }
            0x1B => {
                // ESC - 可能是 ST
                handler.apc_dispatch(self.osc_buffer.as_bytes());
                self.osc_buffer.clear();
                self.escape_state = ESC_NONE;
            }
            0x9C => {
                // ST
                handler.apc_dispatch(self.osc_buffer.as_bytes());
                self.osc_buffer.clear();
                self.escape_state = ESC_NONE;
            }
            _ => {}
        }
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct TestHandler {
        printed: Vec<char>,
        executed: Vec<u8>,
        csi_calls: Vec<(String, Vec<i32>)>,
    }
    
    impl TestHandler {
        fn new() -> Self {
            Self {
                printed: Vec::new(),
                executed: Vec::new(),
                csi_calls: Vec::new(),
            }
        }
    }
    
    impl Perform for TestHandler {
        fn print(&mut self, c: char) {
            self.printed.push(c);
        }
        
        fn execute(&mut self, byte: u8) {
            self.executed.push(byte);
        }
        
        fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
        
        fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
            self.csi_calls.push((action.to_string(), params.values[..params.len].to_vec()));
        }
        
        fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    }
    
    #[test]
    fn test_plain_text() {
        let mut parser = Parser::new();
        let mut handler = TestHandler::new();
        parser.advance(&mut handler, b"Hello");
        assert_eq!(handler.printed, vec!['H', 'e', 'l', 'l', 'o']);
    }
    
    #[test]
    fn test_cursor_up() {
        let mut parser = Parser::new();
        let mut handler = TestHandler::new();
        parser.advance(&mut handler, b"\x1b[2A");
        assert_eq!(handler.csi_calls.len(), 1);
        assert_eq!(handler.csi_calls[0], ("A".to_string(), vec![2]));
    }
    
    #[test]
    fn test_multiple_params() {
        let mut parser = Parser::new();
        let mut handler = TestHandler::new();
        parser.advance(&mut handler, b"\x1b[10;20H");
        assert_eq!(handler.csi_calls.len(), 1);
        assert_eq!(handler.csi_calls[0], ("H".to_string(), vec![10, 20]));
    }
}
