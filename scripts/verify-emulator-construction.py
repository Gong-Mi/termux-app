#!/usr/bin/env python3
"""Constructor contract (recording JNI boundary) and real Kotlin/JNI smoke.

Same classpaths as verify-keyhandler-jni.py. No Gradle source exclusions.
Contract mode compiles production Kotlin with ONLY RustTerminal substituted by
an explicitly recording boundary; native mode compiles all production unchanged.
All JVMs have isolated /dev/null stdin; never run the old reader in the caller.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('compiler-classpath', 'compile-classpath', 'runtime-classpath', 'output'):
        p.add_argument('--' + name, required=True)
    p.add_argument('--native-library', required=True)
    p.add_argument('--mode', choices=('contract', 'native'), required=True)
    a = p.parse_args()
    repo = Path(__file__).resolve().parents[1]
    Path(a.output).mkdir(parents=True, exist_ok=True)
    out = Path(tempfile.mkdtemp(prefix=a.mode + '-', dir=a.output))
    classes = out / 'classes'
    classes.mkdir()
    src = repo / 'terminal-emulator/src/main/java'
    sources = sorted(src.rglob('*.kt'))
    summary = {'mode': a.mode, 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
               'sources': {str(s.relative_to(repo)): hashlib.sha256(s.read_bytes()).hexdigest() for s in sources}, 'steps': {}}

    def run(name, cmd, cwd=repo):
        r = subprocess.run(list(map(str, cmd)), cwd=cwd, stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=120)
        (out / (name + '.log')).write_text(r.stdout)
        summary['steps'][name] = {'exit_code': r.returncode, 'command': list(map(str, cmd))}
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
        print(name, r.returncode, r.stdout[-4000:], flush=True)
        return r.returncode

    print('Evidence:', out, flush=True)
    if a.mode == 'contract':
        original = src / 'com/termux/terminal/RustTerminal.kt'
        text = original.read_text()
        def replacement(m):
            name, params, result = m.group(1), m.group(2), m.group(3) or 'Unit'
            if name == 'createEngine':
                body = 'createCalls++; createArgs = listOf(columns, rows, cellWidthPixels, cellHeightPixels, transcriptRows); lastCallback = callback; return nextPtr'
            elif name == 'startIoThread':
                body = 'ioCalls.add(enginePtr to ptyFd)'
            else:
                body = 'error("Unexpected boundary call: ' + name + '")'
            return 'fun ' + name + '(' + params + '): ' + result + ' { ' + body + ' }'
        text, count = re.subn(r'external fun (\w+)\((.*?)\)(?:\s*:\s*([\w?.<>]+))?', replacement, text, flags=re.S)
        assert count > 2
        text = text.replace('object RustTerminal {', '''object RustTerminal {
    var createCalls = 0
    var nextPtr = 0x123456789ABCDEFL
    var createArgs = listOf<Int>()
    var lastCallback: RustEngineCallback? = null
    val ioCalls = mutableListOf<Pair<Long, Int>>()
    fun clearRecording() { createCalls = 0; ioCalls.clear() }
''')
        stub = out / 'RustTerminal.kt'
        stub.write_text(text)
        sources = [s for s in sources if s != original] + [stub]
    harness = repo / 'scripts/kotlin' / ('EmulatorConstructionContract.kt' if a.mode == 'contract' else 'EmulatorConstructionNative.kt')
    if run('compile', ['java', '-cp', a.compiler_classpath, 'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler',
                      '-no-stdlib', '-no-reflect', '-jvm-target', '21', '-classpath', a.compile_classpath,
                      '-d', classes, *sources, harness]):
        return 1
    if a.mode == 'native':
        lib = Path(a.native_library).resolve(strict=True)
        summary['native_library'] = {'path': str(lib), 'sha256': hashlib.sha256(lib.read_bytes()).hexdigest()}
        native = out / 'build/libs'
        native.mkdir(parents=True)
        (native / lib.name).symlink_to(lib)
    return run('run', ['java', '-cp', os.pathsep.join([str(classes), a.runtime_classpath]),
                       'com.termux.terminal.' + harness.stem + 'Kt'], out)


if __name__ == '__main__':
    raise SystemExit(main())
