#!/usr/bin/env python3
"""Static D1 wiring and test-scope gate; not ART/Service runtime acceptance."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
RUST = ROOT / 'terminal-emulator/src/main/rust'
JAVA = ROOT / 'terminal-emulator/src/main/java/com/termux/terminal'


def main():
    native = (RUST / 'src/jni/terminal_emulator.rs').read_text()
    coordinator = (RUST / 'src/coordinator.rs').read_text()
    engine = (RUST / 'src/engine/mod.rs').read_text()
    session = (JAVA / 'TerminalSession.kt').read_text()
    jni = (JAVA / 'JNI.kt').read_text()
    service = (ROOT / 'app/src/main/java/com/termux/app/TermuxService.java').read_text()
    start = native.index('pub unsafe extern "system" fn Java_com_termux_terminal_JNI_createSessionAsync')
    body = native[start:]
    assert body.index('coordinator.set_engine_data(') < body.index('"onEngineInitialized"')
    assert 'pending.cleanup_transferred = true' in body
    assert 'pending.cleanup_transferred = result.is_ok()' not in body
    for method in ('claimEngineData', 'ackEngineData', 'rejectEngineData'):
        assert f'external fun {method}(' in jni
        assert f'Java_com_termux_terminal_JNI_{method}(' in coordinator
    adopt = session[session.index('fun onEngineInitialized('):session.index('fun dispose()')]
    assert adopt.index('JNI.claimEngineData(') < adopt.index('JNI.ackEngineData(') < adopt.index('mSessionState = SessionState.READY')
    no_claim = adopt[adopt.index('if (data == null)'):adopt.index('} else {', adopt.index('if (data == null)'))]
    assert 'rejectEngineData' not in no_claim, 'a failed claim has no authority to reject another claimant'
    assert 'mLifecycleLock' in adopt and 'SessionState.INITIALIZING' in adopt
    assert 'mEmulator!!' not in session, 'repeated nullable field reads race final disposal'
    assert 'mRustCallback.updateClient(client)' in session
    assert '.updateTerminalSessionClient(client)' not in session, 'do not install raw Activity client as native callback'
    assert service.count('.getTerminalSession().dispose();') == 2
    callback = service[service.index('public void onTermuxSessionExited('):]
    assert callback.index('processPluginExecutionCommandResult(') < callback.index('.getTerminalSession().dispose();')
    normal = re.search(r'pub fn destroy_engine\(.*?\n}', engine, re.S).group()
    assert 'terminate_unadopted_process' not in normal
    assert 'terminate_unadopted_process' in engine
    assert 'destroy_unadopted_engine' in coordinator and 'destroy_unadopted_engine(self.handle)' in native
    shims = {str(p.relative_to(ROOT / 'scripts/java/delivery-stubs')) for p in (ROOT / 'scripts/java/delivery-stubs').rglob('*.java')}
    assert shims == {'android/os/Handler.java', 'android/os/Message.java', 'android/util/Log.java'}
    verifier = (ROOT / 'scripts/verify-emulator-construction.py').read_text()
    assert "if a.mode == 'delivery':" in verifier and "summary['scheduling_boundary']" in verifier
    runner = (ROOT / 'scripts/run-rust-tests.sh').read_text()
    assert runner.count('engine_delivery_claim') == 2
    assert 'name = "engine_delivery_claim"' in (RUST / 'Cargo.toml').read_text()
    print('PASS D1 native offer/claim/ack, Kotlin adoption, two Service removal hooks, queue-only shim and registration; not ART')


if __name__ == '__main__':
    main()
