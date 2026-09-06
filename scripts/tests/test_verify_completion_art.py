"""Synthetic instrumentation-output contracts, not Android execution evidence."""
import importlib.util
from pathlib import Path
import re
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('completion_art', ROOT / 'scripts/verify-completion-art.py')
art = importlib.util.module_from_spec(spec)
spec.loader.exec_module(art)


class CompletionArtContracts(unittest.TestCase):
    def output(self):
        return '\n'.join('INSTRUMENTATION_STATUS: test=' + name for name in sorted(art.EXPECTED)) + f'\nOK ({len(art.EXPECTED)} tests)\nINSTRUMENTATION_CODE: -1\n'

    def test_complete_output_passes_and_runner_code_is_not_test_failure(self):
        self.assertTrue(art.verify_output(self.output(), 0))

    def test_missing_count_wrong_count_or_missing_test_fails(self):
        text = self.output()
        for bad in (text.replace(f'OK ({len(art.EXPECTED)} tests)', ''), text.replace(f'{len(art.EXPECTED)} tests', '0 tests'),
                    text.replace(sorted(art.EXPECTED)[0], 'unexpectedTest'), ''):
            self.assertFalse(art.verify_output(bad, 0))
        self.assertFalse(art.verify_output(text, 1))

    def test_failure_and_skipped_status_cannot_hide_behind_ok(self):
        for marker in ('FAILURES!!!', 'INSTRUMENTATION_FAILED', 'Process crashed',
                       'INSTRUMENTATION_STATUS_CODE: -1', 'INSTRUMENTATION_STATUS_CODE: -2',
                       'INSTRUMENTATION_STATUS_CODE: -3', 'INSTRUMENTATION_STATUS_CODE: -4'):
            self.assertFalse(art.verify_output(self.output() + marker, 0))

    def test_art_is_not_skipped_by_independent_skia_failure(self):
        flow = yaml.safe_load((ROOT / '.github/workflows/android-emulator-experiment.yml').read_text())
        steps = flow['jobs']['install-startup']['steps']
        install = next(step for step in steps if step.get('id') == 'install-target')
        self.assertIn('adb install -r', install['run'])
        art_step = next(step for step in steps if step['name'] == 'Verify real ART completion and plugin result delivery')
        self.assertEqual(art_step['if'], "${{ !cancelled() && steps.install-target.outcome == 'success' }}")
        self.assertFalse(art_step.get('continue-on-error', False))

    def test_art_output_producers_are_system_tools_and_fail_closed(self):
        source = (ROOT / 'app/src/androidTest/java/com/termux/app/SessionCompletionArtTest.java').read_text()
        for marker in ('art-tail', 'result-tail', 'plugin-art-tail'):
            self.assertIn("/system/bin/toybox printf '" + marker, source)
        self.assertEqual(source.count('|| exit 91;'), 3)  # original completion fixtures
        self.assertIn('actual transcript=', source)
        self.assertIn('actual plugin stdout=', source)

    def test_app_suite_explicitly_defers_package_mutation(self):
        expected = art.expected_for_suite('app')
        self.assertNotIn('aptInstallsPythonAndPythonSubprocessRunsInArt', expected)
        self.assertIn('existingPrefixDirectoryRepairPreservesUserFilesInArt', expected)
        self.assertEqual(art.EXPECTED - expected, {'aptInstallsPythonAndPythonSubprocessRunsInArt'})
        output = '\n'.join('INSTRUMENTATION_STATUS: test=' + name for name in sorted(expected)) + f'\nOK ({len(expected)} tests)\n'
        self.assertTrue(art.verify_output(output, 0, expected))
        self.assertFalse(art.verify_output(output, 0))
        flow = yaml.safe_load((ROOT / '.github/workflows/android-emulator-experiment.yml').read_text())
        commands = '\n'.join(step.get('run', '') for step in flow['jobs']['install-startup']['steps'])
        self.assertIn('--output completion-art --suite app', commands)

    def test_zero_exit_install_log_with_traceback_is_not_clean(self):
        self.assertEqual(art.package_script_errors('Setting up python ...\n'), [])
        errors = art.package_script_errors('Setting up python ...\nTraceback (most recent call last):\nPermissionError: denied\nSetting up pip ...\n')
        self.assertEqual(errors, ['Traceback (most recent call last):', 'PermissionError: denied'])

    def test_package_python_probe_is_real_and_failure_checked(self):
        import ast
        import subprocess
        assets = ROOT / 'app/src/androidTest/assets'
        script = (assets / 'package-python-art.sh').read_text()
        subprocess.run(['bash', '-n', str(assets / 'package-python-art.sh')], check=True)
        self.assertIn('APT::Update::Error-Mode=any update', script)
        self.assertIn('install python', script)
        self.assertNotIn('--allow-unauthenticated', script)
        self.assertIn('dpkg-query', script)
        self.assertIn('python-result.json', script)
        probe = (assets / 'python-subprocess-probe.py').read_text()
        ast.parse(probe)
        self.assertIn('subprocess.check_output', probe)
        self.assertIn("[sys.executable, '-c'", probe)
        self.assertIn("prefix + '/bin/printf'", probe)
        self.assertIn('aptInstallsPythonAndPythonSubprocessRunsInArt', art.EXPECTED)

    def test_every_named_art_test_is_registered_and_ci_keeps_ab(self):
        source = (ROOT / 'app/src/androidTest/java/com/termux/app/SessionCompletionArtTest.java').read_text()
        self.assertEqual(set(re.findall(r'@Test public void (\w+)\(', source)), art.EXPECTED)
        flow = yaml.safe_load((ROOT / '.github/workflows/android-emulator-experiment.yml').read_text())
        self.assertIn('feat/session-completion-ui', flow['on']['pull_request']['branches'])
        steps = flow['jobs']['install-startup']['steps']
        commands = '\n'.join(step.get('run', '') for step in steps)
        self.assertIn(':app:assembleDebugAndroidTest', commands)
        self.assertIn('scripts/verify-completion-art.py --output completion-art', commands)
        self.assertIn('--require-baseline-failure', commands)
        self.assertTrue(any('completion-art/' in step.get('with', {}).get('path', '') for step in steps))


if __name__ == '__main__':
    unittest.main()
