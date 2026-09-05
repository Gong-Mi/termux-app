#!/usr/bin/env python3
"""Static JNI ownership/registration gate; runtime leases are tested separately."""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / 'terminal-emulator/src/main/rust'

def main():
    java = (ROOT / 'terminal-emulator/src/main/java/com/termux/terminal/RustTerminal.kt').read_text()
    native = (RUST / 'src/jni/terminal_emulator.rs').read_text()
    renderer = (RUST / 'src/render_thread.rs').read_text()
    names = re.findall(r'external fun (\w+)\(enginePtr: Long', java)
    assert names and len(names) == len(set(names)), 'duplicate/empty native handle declaration inventory'
    for name in names:
        match = re.search(r'pub extern "system" fn Java_com_termux_terminal_RustTerminal_' + name + r'\(.*?(?=\n#\[unsafe\(no_mangle\)\]|\Z)', native, re.S)
        assert match, name
        block = match.group()
        if name == 'destroyEngine':
            assert 'destroy_engine(ptr)' in block, name
        else:
            assert 'ENGINE_HANDLES.acquire(ptr)' in block, name
    for text in (native, renderer):
        assert not re.search(r'Arc::(?:from_raw|into_raw)|as \*const TerminalContext', text), 'raw engine ownership reintroduced'
    assert 'ENGINE_HANDLES.current()' in renderer
    assert 'ENGINE_HANDLES.publish(ptr)' in (RUST / 'src/jni/terminal_view.rs').read_text()
    runner = (ROOT / 'scripts/run-rust-tests.sh').read_text()
    manifest = (RUST / 'Cargo.toml').read_text()
    for name in ('engine_handle_lifecycle', 'engine_handle_integration'):
        assert runner.count(name) == 2, name
        assert 'name = "' + name + '"' in manifest, name
    tracked = subprocess.check_output(['git', 'ls-files', 'terminal-emulator/src/main/rust/src'], cwd=ROOT, text=True).splitlines()
    for relative in tracked:
        path = ROOT / relative
        if path.suffix == '.rs':
            assert not re.search(r'Arc::from_raw\([^\n]*TerminalContext', path.read_text()), relative
    print(f'PASS: {len(names)} declared handle consumers, both creation paths raw-free, renderer lease and test registration')

if __name__ == '__main__':
    main()
