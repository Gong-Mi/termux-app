#!/usr/bin/env python3
"""Isolated KeyHandler JNI acceptance, NOT the Gradle/full Java test gate.

Compile unmodified production Kotlin sources, then the actual JUnit test. Supply
an independently built, platform-compatible libtermux_rust (no substitute JNI).
Each run creates a fresh evidence directory below --output. Also compile ALL
terminal-emulator Java tests and record that gate separately; never exclude old
tests from Gradle or report the isolated result as full-suite success.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler-classpath', required=True,
                        help='Classpath containing Kotlin compiler and its dependencies')
    parser.add_argument('--compile-classpath', required=True,
                        help='Android SDK, AndroidX annotations and Kotlin stdlib jars')
    parser.add_argument('--runtime-classpath', required=True,
                        help='JUnit 4, Hamcrest and Kotlin stdlib jars')
    parser.add_argument('--native-library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    args.output.mkdir(parents=True, exist_ok=True)
    out = Path(tempfile.mkdtemp(prefix='keyhandler-', dir=args.output.resolve()))
    classes = out / 'classes'
    classes.mkdir()
    summary = {'scope': 'isolated real-production Kotlin/JNI JUnit test; not Gradle',
               'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
               'steps': {}}
    print(f'Evidence: {out}', flush=True)

    def save():
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')

    def run(name, command, cwd=repo):
        print(f'{name}: {shlex.join(map(str, command))}', flush=True)
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=cwd,
                                    stdout=log, stderr=subprocess.STDOUT)
        summary['steps'][name] = {'exit_code': result.returncode,
                                  'command': list(map(str, command)), 'cwd': str(cwd)}
        save()
        print(f'{name}: exit {result.returncode}', flush=True)
        return result.returncode

    lib = args.native_library.resolve(strict=True)
    summary['native_library'] = {'path': str(lib), 'sha256': hashlib.sha256(lib.read_bytes()).hexdigest()}
    src = repo / 'terminal-emulator/src/main/java'
    tests = repo / 'terminal-emulator/src/test/java'
    target = tests / 'com/termux/terminal/KeyHandlerRustTest.java'
    sources = sorted(src.rglob('*.kt'))
    summary['source_sha256'] = {str(p.relative_to(repo)): hashlib.sha256(p.read_bytes()).hexdigest()
                                for p in sources + [target]}
    if run('production-kotlin', ['java', '-cp', args.compiler_classpath,
           'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '-no-stdlib', '-no-reflect',
           '-jvm-target', '21', '-classpath', args.compile_classpath,
           '-d', classes, *sources]):
        return 1
    cp = os.pathsep.join([str(classes), args.compile_classpath, args.runtime_classpath])
    all_classes = out / 'all-test-classes'
    all_classes.mkdir()
    run('all-java-tests-compile', ['javac', '-Xmaxerrs', '20', '-cp', cp,
                                 '-d', all_classes, *sorted(tests.rglob('*.java'))])
    if run('keyhandler-compile', ['javac', '-cp', cp, '-d', classes, target]):
        return 1
    # Use an existing production JNI.kt host search location in a fresh cwd.
    # Do not change java.vendor, production source, or any native method.
    present = out / 'present'
    missing = out / 'missing'
    native_dir = present / 'build/libs'
    native_dir.mkdir(parents=True)
    missing.mkdir()
    (native_dir / lib.name).symlink_to(lib)
    runtime_cp = os.pathsep.join([str(classes), args.runtime_classpath])
    junit = ['java', '-cp', runtime_cp, 'org.junit.runner.JUnitCore',
             'com.termux.terminal.KeyHandlerRustTest']
    positive = run('jni-present', junit, present)
    negative = run('jni-missing', junit, missing)
    positive_text = (out / 'jni-present.log').read_text()
    negative_text = (out / 'jni-missing.log').read_text()
    passed = positive == 0 and 'OK (6 tests)' in positive_text
    fail_closed = negative != 0 and 'KeyHandler tests require the real termux_rust JNI library' in negative_text
    summary['isolated_six_tests_passed'] = passed
    summary['missing_jni_fails_closed'] = fail_closed
    summary['all_java_tests_compile_passed'] = summary['steps']['all-java-tests-compile']['exit_code'] == 0
    save()
    print(json.dumps(summary, indent=2), flush=True)
    return 0 if passed and fail_closed else 1


if __name__ == '__main__':
    raise SystemExit(main())
