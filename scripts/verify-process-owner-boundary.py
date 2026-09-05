#!/usr/bin/env python3
"""Static C1 wiring gate; actual child/JNI tests provide runtime evidence."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / 'terminal-emulator/src/main/rust'
JAVA = ROOT / 'terminal-emulator/src/main/java/com/termux/terminal'


def main():
    coordinator = (RUST / 'src/coordinator.rs').read_text()
    native = (RUST / 'src/jni/terminal_emulator.rs').read_text()
    pty = (RUST / 'src/pty.rs').read_text()
    context = (RUST / 'src/engine/context.rs').read_text()
    owner = (RUST / 'src/process_owner.rs').read_text()
    session = (JAVA / 'TerminalSession.kt').read_text()
    jni = (JAVA / 'JNI.kt').read_text()
    assert not re.search(r'waitpid\s*\(\s*-1\b', coordinator)
    assert 'libc::kill(' not in coordinator
    assert 'Os.kill(' not in session
    assert 'JNI.terminateSession(sessionId)' in session
    assert 'mNativeSessionId = sessionId' in session
    assert 'coordinator.bind_pty_child(session_id, pid)' in native
    assert 'TerminalContext::with_process(engine, process)' in native
    assert 'managed_process_for_pid(pid)' in pty
    assert 'owner.wait()' in pty
    assert 'record_managed_child_exit()' in coordinator
    assert 'self.process.as_ref().is_some_and(|owner| !owner.is_running())' in context
    assert 'libc::P_PIDFD' in owner and 'libc::WNOHANG' in owner
    assert 'libc::SYS_pidfd_send_signal' in owner
    for method in ('terminateSession', 'getSessionProcessStatus'):
        assert f'external fun {method}(' in jni
        assert f'Java_com_termux_terminal_JNI_{method}(' in coordinator
    runner = (ROOT / 'scripts/run-rust-tests.sh').read_text()
    manifest = (RUST / 'Cargo.toml').read_text()
    for target in ('process_owner', 'session_process_lifecycle'):
        assert runner.count(target) == 2, target
        assert f'name = "{target}"' in manifest, target
    print('PASS C1 process-owner wiring/known-child reaper/session termination/test registration; not drain/UI/ART acceptance')


if __name__ == '__main__':
    main()
