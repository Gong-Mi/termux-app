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
        return '\n'.join('INSTRUMENTATION_STATUS: test=' + name for name in sorted(art.EXPECTED)) + '\nOK (3 tests)\nINSTRUMENTATION_CODE: -1\n'

    def test_complete_output_passes_and_runner_code_is_not_test_failure(self):
        self.assertTrue(art.verify_output(self.output(), 0))

    def test_missing_count_wrong_count_or_missing_test_fails(self):
        text = self.output()
        for bad in (text.replace('OK (3 tests)', ''), text.replace('3 tests', '2 tests'),
                    text.replace(sorted(art.EXPECTED)[0], 'unexpectedTest'), ''):
            self.assertFalse(art.verify_output(bad, 0))
        self.assertFalse(art.verify_output(text, 1))

    def test_failure_and_skipped_status_cannot_hide_behind_ok(self):
        for marker in ('FAILURES!!!', 'INSTRUMENTATION_FAILED', 'Process crashed',
                       'INSTRUMENTATION_STATUS_CODE: -1', 'INSTRUMENTATION_STATUS_CODE: -2',
                       'INSTRUMENTATION_STATUS_CODE: -3', 'INSTRUMENTATION_STATUS_CODE: -4'):
            self.assertFalse(art.verify_output(self.output() + marker, 0))

    def test_art_output_producers_are_system_tools_and_fail_closed(self):
        source = (ROOT / 'app/src/androidTest/java/com/termux/app/SessionCompletionArtTest.java').read_text()
        self.assertEqual(source.count('/system/bin/toybox printf'), len(art.EXPECTED))
        self.assertEqual(source.count('|| exit 91;'), len(art.EXPECTED))
        self.assertIn('actual transcript=', source)
        self.assertIn('actual plugin stdout=', source)

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
