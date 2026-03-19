//! Termux 独立终端模拟器
//! 基于 Rust 终端引擎的完整 TUI 应用

use std::io::{self, Write};
use std::time::{Duration, Instant};
use crossterm::{
    cursor::{Hide, Show, MoveTo},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    execute,
    style::{Color, SetForegroundColor, SetBackgroundColor, ResetColor, SetAttribute, Attribute},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

mod vte_parser;
mod terminal_engine;

use terminal_engine::TerminalEngine;

// ============================================================================
// 主程序
// ============================================================================

fn main() -> io::Result<()> {
    // 初始化终端
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;

    // 获取终端尺寸
    let (cols, rows) = terminal::size()?;

    // 创建终端引擎
    let mut engine = TerminalEngine::new(cols as i32, rows as i32);

    // 启动 PTY 子进程（优先使用 Termux 环境下的 shell）
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        let prefix = std::env::var("PREFIX").unwrap_or_else(|_| "/data/data/com.termux/files/usr".to_string());
        format!("{}/bin/sh", prefix)
    });
    let pty_fd = match spawn_pty(&shell, cols as i32, rows as i32) {
        Ok(fd) => {
            // 设置 FD 为非阻塞
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
            fd
        },
        Err(e) => {
            execute!(stdout, Show, LeaveAlternateScreen)?;
            terminal::disable_raw_mode()?;
            eprintln!("无法启动 PTY ({}): {}", shell, e);
            std::process::exit(1);
        }
    };

    // 主循环
    let result = run_main_loop(&mut stdout, &mut engine, pty_fd);

    // 清理
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_main_loop<W: Write>(
    stdout: &mut W,
    engine: &mut TerminalEngine,
    pty_fd: i32,
) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    let mut last_render = Instant::now();
    let render_interval = Duration::from_millis(16); // ~60 FPS

    loop {
        // 1. 从 PTY 读取数据
        let n = unsafe {
            libc::read(pty_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };

        if n > 0 {
            let data = &buf[..n as usize];
            // 解析 ANSI 序列
            engine.parse_bytes(data);
        } else if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::WouldBlock {
                // 真正的读取错误
                break;
            }
        }

        // 2. 渲染屏幕
        if last_render.elapsed() >= render_interval {
            render_screen(stdout, engine)?;
            last_render = Instant::now();
        }

        // 3. 处理键盘/鼠标事件
        if event::poll(Duration::from_millis(1))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key_event(pty_fd, key, engine.application_cursor_keys)? {
                        break; // 退出
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(pty_fd, mouse)?;
                }
                Event::Resize(cols, rows) => {
                    engine.resize(cols as i32, rows as i32);
                    set_pty_winsize(pty_fd, cols as i32, rows as i32);
                }
                _ => {} // 忽略其他事件（FocusGained, FocusLost, Paste）
            }
        }
    }

    Ok(())
}

// ============================================================================
// 渲染
// ============================================================================

fn render_screen<W: Write>(stdout: &mut W, engine: &TerminalEngine) -> io::Result<()> {
    for row in 0..engine.rows {
        // 移动光标到行首
        execute!(stdout, MoveTo(0, row as u16))?;

        for col in 0..engine.cols {
            let cell = engine.get_cell(col as usize, row as usize);
            
            // 应用样式
            if cell.bold {
                execute!(stdout, SetAttribute(Attribute::Bold))?;
            }
            if cell.underline {
                execute!(stdout, SetAttribute(Attribute::Underlined))?;
            }
            
            // 应用颜色
            if let Some(fg) = cell.fg_color {
                execute!(stdout, SetForegroundColor(Color::Rgb { r: fg.0, g: fg.1, b: fg.2 }))?;
            }
            if let Some(bg) = cell.bg_color {
                execute!(stdout, SetBackgroundColor(Color::Rgb { r: bg.0, g: bg.1, b: bg.2 }))?;
            }

            // 打印字符
            let c = if cell.char == '\0' { ' ' } else { cell.char };
            stdout.write_all(c.to_string().as_bytes())?;
        }
    }

    execute!(stdout, ResetColor)?;

    // 同步物理光标位置（至关重要！）
    execute!(stdout, MoveTo(engine.cursor_x as u16, engine.cursor_y as u16))?;
    
    stdout.flush()?;

    Ok(())
}

// ============================================================================
// 输入处理
// ============================================================================

fn handle_key_event(
    pty_fd: i32,
    key: KeyEvent,
    app_cursor: bool,
) -> io::Result<bool> {
    // Ctrl+C 退出
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    // 转换按键为转义序列
    let seq = key_to_escape_sequence(key, app_cursor);
    if !seq.is_empty() {
        unsafe {
            libc::write(pty_fd, seq.as_ptr() as *const libc::c_void, seq.len());
        }
    }

    Ok(false)
}

fn handle_mouse_event(
    pty_fd: i32,
    mouse: MouseEvent,
) -> io::Result<()> {
    // 发送鼠标事件到 PTY
    let seq = match mouse.kind {
        MouseEventKind::Down(button) => {
            format!("\x1b[<{};{};{}M", button as u8 + 32, mouse.column + 1, mouse.row + 1)
        }
        MouseEventKind::Up(button) => {
            format!("\x1b[<{};{};{}m", button as u8 + 32, mouse.column + 1, mouse.row + 1)
        }
        MouseEventKind::Drag(button) => {
            format!("\x1b[<{};{};{}M", button as u8 + 36, mouse.column + 1, mouse.row + 1)
        }
        _ => return Ok(()),
    };

    unsafe {
        libc::write(pty_fd, seq.as_ptr() as *const libc::c_void, seq.len());
    }

    Ok(())
}

fn key_to_escape_sequence(key: KeyEvent, app_cursor: bool) -> String {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl 键
                match c {
                    'a'..='z' => format!("{}", (c as u8 - b'a' + 1) as char),
                    '@' => "\x00".to_string(),
                    '[' => "\x1b".to_string(),
                    '\\' => "\x1c".to_string(),
                    ']' => "\x1d".to_string(),
                    '^' => "\x1e".to_string(),
                    '_' => "\x1f".to_string(),
                    _ => c.to_string(),
                }
            } else if key.modifiers.contains(KeyModifiers::ALT) {
                // Alt 键
                format!("\x1b{}", c)
            } else {
                c.to_string()
            }
        }
        KeyCode::Enter => "\r".to_string(),
        KeyCode::Backspace => "\x7f".to_string(),
        KeyCode::Tab => "\t".to_string(),
        KeyCode::Esc => "\x1b".to_string(),
        KeyCode::Up => if app_cursor { "\x1bOA".to_string() } else { "\x1b[A".to_string() },
        KeyCode::Down => if app_cursor { "\x1bOB".to_string() } else { "\x1b[B".to_string() },
        KeyCode::Right => if app_cursor { "\x1bOC".to_string() } else { "\x1b[C".to_string() },
        KeyCode::Left => if app_cursor { "\x1bOD".to_string() } else { "\x1b[D".to_string() },
        KeyCode::Home => "\x1b[H".to_string(),
        KeyCode::End => "\x1b[F".to_string(),
        KeyCode::PageUp => "\x1b[5~".to_string(),
        KeyCode::PageDown => "\x1b[6~".to_string(),
        KeyCode::Delete => "\x1b[3~".to_string(),
        KeyCode::F(n) => format!("\x1b[{}~", n + 10),
        _ => String::new(),
    }
}

// ============================================================================
// PTY 支持
// ============================================================================

use std::ffi::CString;

unsafe extern "C" {
    fn ptsname_r(fd: i32, buf: *mut libc::c_char, buflen: usize) -> i32;
}

fn set_pty_winsize(fd: i32, cols: i32, rows: i32) {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    let mut ws = Winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ as _, &mut ws);
    }
}

fn spawn_pty(shell: &str, cols: i32, rows: i32) -> io::Result<i32> {
    // 打开 master PTY
    let master_fd = unsafe { libc::posix_openpt(libc::O_RDWR) };
    if master_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    unsafe {
        libc::grantpt(master_fd);
        libc::unlockpt(master_fd);
    }

    // 获取 slave PTY 名称
    let mut buf = [0u8; 256];
    unsafe {
        ptsname_r(master_fd, buf.as_mut_ptr() as *mut libc::c_char, buf.len());
    }
    let pts_name = CString::new(
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char) }
            .to_str()
            .unwrap_or("/dev/pts/0")
    ).unwrap();

    // Fork 子进程
    match unsafe { libc::fork() } {
        0 => {
            // 子进程
            unsafe {
                // 开启新会话
                libc::setsid();

                let slave_fd = libc::open(pts_name.as_ptr(), libc::O_RDWR);
                if slave_fd < 0 {
                    libc::_exit(1);
                }

                // 重定向标准输入输出
                libc::dup2(slave_fd, libc::STDIN_FILENO);
                libc::dup2(slave_fd, libc::STDOUT_FILENO);
                libc::dup2(slave_fd, libc::STDERR_FILENO);

                // 设置控制终端
                libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0);

                // 设置 termios 属性 (关键：启用 ONLCR 将 \n 转换为 \r\n)
                let mut tio: libc::termios = std::mem::zeroed();
                libc::tcgetattr(slave_fd, &mut tio);
                tio.c_oflag |= libc::OPOST | libc::ONLCR;
                libc::tcsetattr(slave_fd, libc::TCSANOW, &tio);

                if slave_fd > 2 {
                    libc::close(slave_fd);
                }

                // 设置窗口大小
                set_pty_winsize(libc::STDIN_FILENO, cols, rows);

                // 设置环境变量
                let prefix = "/data/data/com.termux/files/usr";
                let home = "/data/data/com.termux/files/home";
                let path = format!("{}/bin:{}/bin/applets", prefix, prefix);

                let env_map = [
                    ("HOME", home),
                    ("PREFIX", prefix),
                    ("TERM", "xterm-256color"),
                    ("PATH", &path),
                    ("USER", "termux"),
                    ("SHELL", &shell),
                    ("ANDROID_ROOT", "/system"),
                    ("ANDROID_DATA", "/data"),
                    ("EXTERNAL_STORAGE", "/sdcard"),
                    ("LANG", "en_US.UTF-8"),
                ];

                let mut env_strings = Vec::new();
                for (k, v) in env_map.iter() {
                    env_strings.push(CString::new(format!("{}={}", k, v)).unwrap());
                }

                let mut envs_ptr: Vec<*const libc::c_char> = env_strings.iter().map(|s| s.as_ptr()).collect();
                envs_ptr.push(std::ptr::null());

                // 执行 shell (模拟登录 shell)
                let shell_c = CString::new(shell).unwrap();
                let shell_filename = shell.split('/').last().unwrap_or("sh");
                let arg0 = CString::new(format!("-{}", shell_filename)).unwrap();
                
                let args = [arg0.as_ptr(), std::ptr::null()];
                libc::execve(shell_c.as_ptr(), args.as_ptr(), envs_ptr.as_ptr());

                libc::_exit(1);
            }
        }
        pid if pid > 0 => {
            // 父进程，返回 master FD
            Ok(master_fd)
        }
        _ => Err(io::Error::last_os_error()),
    }
}
