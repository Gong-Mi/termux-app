#!/usr/bin/env python3
"""Static complete production IO handoff gate, not syscall/runtime acceptance."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / 'terminal-emulator/src/main/rust'
JAVA = ROOT / 'terminal-emulator/src/main/java/com/termux/terminal'


def main():
    context = (RUST / 'src/engine/context.rs').read_text()
    native = (RUST / 'src/jni/terminal_emulator.rs').read_text()
    destroy = (RUST / 'src/engine/mod.rs').read_text()
    session = (JAVA / 'TerminalSession.kt').read_text()
    assert 'pty_fd:' not in context, 'context raw fd publication reintroduced'
    assert not re.search(r'libc::(?:read|write|close)\(', context)
    assert 'Arc::downgrade(&context)' in context
    assert 'runtime.cancel()' in context and 'runtime.join()' in context
    assert 'TerminalContext::stop_io(&context)' in destroy
    assert 'libc::close' not in destroy
    assert 'context.pty_fd' not in native
    assert 'libc::write(' not in native and 'libc::dup(' not in native
    assert 'JNI.setPtyWindowSize' not in session
    assert 'RustTerminal.processInput(' not in session
    assert 'RustTerminal.tryProcessInput(' in session
    assert 'INPUT_ACCEPTED' in session and 'input was not queued' in session
    assert 'context.resize_pty(rows, cols, cw, ch)' in native
    for name in ('processInput', 'tryProcessInput', 'startIoThread', 'resize'):
        block = re.search(r'pub extern "system" fn Java_com_termux_terminal_RustTerminal_' + name +
                          r'\(.*?(?=\n#\[unsafe\(no_mangle\)\]|\Z)', native, re.S)
        assert block and 'ENGINE_HANDLES.acquire(ptr)' in block.group(), name
    runner = (ROOT / 'scripts/run-rust-tests.sh').read_text()
    for target in ('pty_io_runtime', 'pty_context_integration'):
        assert runner.count(target) == 2, target
        assert f'name = "{target}"' in (RUST / 'Cargo.toml').read_text(), target
    print('PASS: context/JNI/Kotlin read-write-close-resize handoff and runtime test registration')


if __name__ == '__main__':
    main()
