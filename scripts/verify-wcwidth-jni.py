#!/usr/bin/env python3
"""Isolated production WcWidth JNI and missing-library fallback JUnit gates.

Not Gradle/full-suite acceptance. Compile unmodified production Kotlin; supply
an existing platform-compatible native library (never build or substitute JNI).
Four fresh JVMs check both positive paths and both wrong-environment failures.
Classpath arguments must use absolute paths because JVM working directories differ.
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
    parser.add_argument('--compiler-classpath', required=True)
    parser.add_argument('--compile-classpath', required=True)
    parser.add_argument('--runtime-classpath', required=True)
    parser.add_argument('--native-library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    args.output.mkdir(parents=True, exist_ok=True)
    out = Path(tempfile.mkdtemp(prefix='wcwidth-', dir=args.output.resolve()))
    summary = {'scope': 'isolated production Kotlin/JNI and fallback; NOT Gradle',
               'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
               'gradle': 'NOT RUN', 'steps': {},
               'LD_LIBRARY_PATH': os.environ.get('LD_LIBRARY_PATH')}

    def save():
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')

    def run(name, command, cwd=repo):
        command = list(map(str, command))
        print(f'{name}: {shlex.join(command)}', flush=True)
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(command, cwd=cwd, stdout=log, stderr=subprocess.STDOUT)
        summary['steps'][name] = {'exit_code': result.returncode, 'command': command, 'cwd': str(cwd)}
        save()
        print(f'{name}: exit {result.returncode}', flush=True)
        return result.returncode

    print(f'Evidence: {out}', flush=True)
    save()  # Leave a summary even if later setup/compilation fails.
    lib = args.native_library.resolve(strict=True)
    summary['native_library'] = {'path': str(lib), 'sha256': hashlib.sha256(lib.read_bytes()).hexdigest()}
    src = repo / 'terminal-emulator/src/main/java'
    tests = repo / 'terminal-emulator/src/test/java'
    # The new missing-library test is intentionally outside the default source
    # set: its JVM environment is incompatible with the existing native tests.
    targets = [tests / 'com/termux/terminal/WcWidthTest.java',
               repo / 'scripts/java/com/termux/terminal/WcWidthFallbackTest.java']
    sources = sorted(src.rglob('*.kt'))
    summary['source_sha256'] = {str(p.relative_to(repo)): hashlib.sha256(p.read_bytes()).hexdigest()
                                for p in sources + targets + [Path(__file__).resolve()]}
    save()
    classes = out / 'classes'
    classes.mkdir()
    if run('production-kotlin', ['java', '-cp', args.compiler_classpath,
           'org.jetbrains.kotlin.cli.jvm.K2JVMCompiler', '-no-stdlib', '-no-reflect',
           '-jvm-target', '21', '-classpath', args.compile_classpath, '-d', classes, *sources]):
        return 1
    cp = os.pathsep.join([str(classes), args.compile_classpath, args.runtime_classpath])
    all_classes = out / 'all-test-classes'
    all_classes.mkdir()
    all_code = run('all-java-tests-compile', ['javac', '-Xmaxerrs', '20', '-cp', cp,
                   '-d', all_classes, *sorted(tests.rglob('*.java'))])
    summary['all_java_tests_compile_passed'] = all_code == 0
    save()
    if run('wcwidth-compile', ['javac', '-cp', cp, '-d', classes, *targets]):
        return 1
    present = out / 'present'
    missing = out / 'missing'
    native_dir = present / 'build/libs'
    native_dir.mkdir(parents=True)
    missing.mkdir()
    (native_dir / lib.name).symlink_to(lib)
    runtime_cp = os.pathsep.join([str(classes), args.runtime_classpath])
    # Production JNI.kt searches build/libs relative to cwd on host JVMs.
    # No java.vendor changes, no loaded-flag mocks, no shared JVM state.
    def junit(name, target, cwd):
        code = run(name, ['java', '-cp', runtime_cp, 'org.junit.runner.JUnitCore',
                         'com.termux.terminal.' + target], cwd)
        return code, (out / (name + '.log')).read_text()

    positive, positive_text = junit('jni-present', 'WcWidthTest', present)
    negative, negative_text = junit('jni-missing', 'WcWidthTest', missing)
    fallback, fallback_text = junit('fallback-missing', 'WcWidthFallbackTest', missing)
    wrong, wrong_text = junit('fallback-present', 'WcWidthFallbackTest', present)
    checks = {
        'original_nine_jni_tests_passed': positive == 0 and 'OK (9 tests)' in positive_text,
        'missing_jni_fails_closed': negative != 0 and 'Tests run: 9,  Failures: 9' in negative_text
            and negative_text.count('WcWidth tests require the real termux_rust JNI library') == 9,
        'fallback_three_tests_passed': fallback == 0 and 'OK (3 tests)' in fallback_text,
        'fallback_rejects_loaded_jni': wrong != 0 and 'Tests run: 3,  Failures: 3' in wrong_text
            and wrong_text.count('WcWidth fallback tests require an isolated JVM without JNI') == 3,
    }
    summary.update(checks)
    summary['isolated_gates_passed'] = all(checks.values())
    save()
    print(json.dumps(checks, indent=2), flush=True)
    print(f'Full Java compilation passed: {all_code == 0}; Gradle NOT RUN', flush=True)
    return 0 if all(checks.values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
