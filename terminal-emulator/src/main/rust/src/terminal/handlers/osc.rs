use crate::engine::ScreenState;
use crate::terminal::colors::TerminalColors;
use crate::terminal::style::{COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND, COLOR_INDEX_CURSOR};

pub fn handle_osc(state: &mut ScreenState, opcode: &str, params: &[&[u8]]) {
    // 将除 opcode 外的所有参数拼接成字符串
    let param_text = params[1..]
        .iter()
        .filter_map(|p| std::str::from_utf8(p).ok())
        .collect::<Vec<&str>>()
        .join(";");

    match opcode {
        "0" | "2" => {
            if params.len() > 1 {
                let title = std::str::from_utf8(params[1]).unwrap_or("");
                let clean_title = title.trim_end_matches(|c| c == '\x07' || c == '\x1b' || c == '\\');
                state.set_title(clean_title);
            }
        }
        "4" => {
            handle_osc4(state, &param_text);
        }
        "10" => {
            handle_osc10(state, &param_text);
        }
        "11" => {
            handle_osc11(state, &param_text);
        }
        "12" => {
            if let Some(color) = TerminalColors::parse_color(&param_text) {
                state.colors.current_colors[COLOR_INDEX_CURSOR as usize] = color;
                state.report_colors_changed();
            }
        }
        "13" => { state.handle_osc13(); }
        "14" => { state.handle_osc14(); }
        "18" => { state.handle_osc18(); }
        "19" => { state.handle_osc19(); }
        "22" => { state.push_title(opcode); }
        "23" => { state.pop_title(opcode); }
        "52" => {
            if params.len() > 2 {
                if let Ok(base64_data) = std::str::from_utf8(params[2]) {
                    state.handle_osc52(base64_data);
                }
            }
        }
        "104" => {
            handle_osc104(state, &param_text);
        }
        "110" => {
            state.colors.reset_index(COLOR_INDEX_FOREGROUND as usize);
            state.report_colors_changed();
        }
        "111" => {
            state.colors.reset_index(COLOR_INDEX_BACKGROUND as usize);
            state.report_colors_changed();
        }
        "112" => {
            state.colors.reset_index(COLOR_INDEX_CURSOR as usize);
            state.report_colors_changed();
        }
        _ => {}
    }
}

fn handle_osc4(state: &mut ScreenState, param_text: &str) {
    let parts: Vec<&str> = param_text.split(';').collect();
    let mut i = 0;
    while i + 1 < parts.len() {
        if let Ok(color_index) = parts[i].parse::<usize>() {
            let color_spec = parts[i + 1];
            if color_spec == "?" {
                let report = state.colors.generate_color_report(color_index);
                state.report_color_response(&format!("4;{}", report));
            } else {
                if state.colors.try_parse_color(color_index, color_spec) {
                    state.report_colors_changed();
                }
            }
        }
        i += 2;
    }
}

fn handle_osc10(state: &mut ScreenState, param_text: &str) {
    if param_text == "?" {
        let report = state.colors.generate_color_report(COLOR_INDEX_FOREGROUND as usize);
        state.report_color_response(&format!("10;{}", report));
    } else {
        if let Some(color) = TerminalColors::parse_color(param_text) {
            state.colors.current_colors[COLOR_INDEX_FOREGROUND as usize] = color;
            state.report_colors_changed();
        }
    }
}

fn handle_osc11(state: &mut ScreenState, param_text: &str) {
    if param_text == "?" {
        let report = state.colors.generate_color_report(COLOR_INDEX_BACKGROUND as usize);
        state.report_color_response(&format!("11;{}", report));
    } else {
        if let Some(color) = TerminalColors::parse_color(param_text) {
            state.colors.current_colors[COLOR_INDEX_BACKGROUND as usize] = color;
            state.report_colors_changed();
        }
    }
}

fn handle_osc104(state: &mut ScreenState, param_text: &str) {
    if param_text.is_empty() {
        state.colors.reset();
        state.report_colors_changed();
    } else {
        for part in param_text.split(';') {
            if let Ok(index) = part.parse::<usize>() {
                state.colors.reset_index(index);
            }
        }
        state.report_colors_changed();
    }
}
