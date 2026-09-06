#!/usr/bin/env python3
"""Run completion instrumentation on a software emulator; never target a physical device."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TEST_CLASS = 'com.termux.app.SessionCompletionArtTest'
EXPECTED = {
    'earlyExitDeliversOnRealMainLooperAndRetainsTranscript',
    'actualExecutionCommandCapturesBeforeResultCallbackDisposes',
    'serviceOnlyPluginDeliversPendingIntentThenRemovesSession',
}


def verify_output(text, returncode):
    names = set(re.findall(r'INSTRUMENTATION_STATUS: test=(\w+)', text))
    ok = re.findall(r'OK \((\d+) tests?\)', text)
    failed = any(marker in text for marker in ('FAILURES!!!', 'INSTRUMENTATION_FAILED',
                 'INSTRUMENTATION_ABORTED', 'Process crashed', 'INSTRUMENTATION_STATUS_CODE: -2',
                 'INSTRUMENTATION_STATUS_CODE: -1', 'INSTRUMENTATION_STATUS_CODE: -3',
                 'INSTRUMENTATION_STATUS_CODE: -4'))
    return returncode == 0 and ok == [str(len(EXPECTED))] and names == EXPECTED and not failed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    parser.add_argument('--serial')
    args = parser.parse_args()
    out = Path(args.output); out.mkdir(parents=True, exist_ok=True)
    adb = ['adb'] + (['-s', args.serial] if args.serial else [])
    summary = {'passed': False, 'layer': 'ART emulator, actual target APK/Looper/JNI/Service/PendingIntent; not final GPU text present',
               'expected_tests': sorted(EXPECTED), 'steps': {}}

    def run(name, arguments, timeout=60):
        result = subprocess.run(adb + arguments, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                text=True, timeout=timeout)
        (out / (name + '.txt')).write_text(result.stdout)
        summary['steps'][name] = {'returncode': result.returncode}
        return result

    try:
        identity = run('emulator-identity', ['shell', 'getprop', 'ro.kernel.qemu'])
        if identity.returncode or identity.stdout.strip() != '1':
            raise RuntimeError('Refusing non-emulator or ambiguous adb target')
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        summary['head'] = head
        source = (ROOT / 'source-identity.txt').read_text()
        if f'headSha={head}\n' not in source:
            raise RuntimeError('Installed APK build identity does not match checkout')
        (out / 'source-identity.txt').write_text(source)
        apks = sorted((ROOT / 'app/build/outputs/apk/androidTest/debug').glob('*.apk'))
        if len(apks) != 1:
            raise RuntimeError(f'Expected one test APK, found {len(apks)}')
        summary['test_apk_sha256'] = hashlib.sha256(apks[0].read_bytes()).hexdigest()
        if run('install-test', ['install', '-r', str(apks[0])]).returncode:
            raise RuntimeError('Test APK installation failed')
        if run('stop-target', ['shell', 'am', 'force-stop', 'com.termux']).returncode:
            raise RuntimeError('Target stop failed')
        run('clear-logcat', ['logcat', '-c'])
        result = run('instrumentation', ['shell', 'am', 'instrument', '-w', '-r', '-e', 'class', TEST_CLASS,
                     'com.termux.test/androidx.test.runner.AndroidJUnitRunner'], timeout=240)
        summary['passed'] = verify_output(result.stdout, result.returncode)
        print(result.stdout)
        if not summary['passed']:
            raise RuntimeError('Instrumentation output failed strict count/name/failure checks')
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        summary['error'] = str(error)
        print('FAIL:', error)
    finally:
        try:
            run('logcat', ['logcat', '-d', '-v', 'threadtime'])
        except (OSError, subprocess.SubprocessError) as error:
            summary['logcat_error'] = str(error)
        (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
