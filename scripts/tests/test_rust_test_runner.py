#!/usr/bin/env python3
"""Runner process contracts; fake Cargo does not constitute Rust acceptance."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TARGETS = json.loads(Path(__file__).with_name('rust-tier-targets.json').read_text())


class RunnerContract(unittest.TestCase):
    def invoke(self, *args, status=0):
        with tempfile.TemporaryDirectory(prefix='rust runner ') as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            (root / 'terminal-emulator/src/main/rust').mkdir(parents=True)
            shutil.copyfile(ROOT / 'scripts/run-rust-tests.sh', root / 'scripts/run-rust-tests.sh')
            cargo = root / 'cargo'
            cargo.write_text('#!' + shutil.which('python3') + '\n'
                             'import json, os, sys\n'
                             'with open(os.environ["CALL_LOG"], "a") as f:\n'
                             ' f.write(json.dumps({"args":sys.argv[1:],"cwd":os.getcwd()})+"\\n")\n'
                             'sys.exit(int(os.environ["FAKE_STATUS"]))\n')
            cargo.chmod(0o755)
            log = root / 'calls.jsonl'
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       CALL_LOG=str(log), FAKE_STATUS=str(status))
            result = subprocess.run(['bash', str(root / 'scripts/run-rust-tests.sh'), *args],
                                    env=env, cwd='/', text=True, capture_output=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, calls

    def test_each_tier_is_one_locked_batch_with_exact_targets(self):
        for tier, targets in TARGETS.items():
            with self.subTest(tier=tier):
                result, calls = self.invoke(tier)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(calls), 1)
                expected = ['test', '--locked', '--no-fail-fast']
                for name in targets:
                    expected += ['--test', name]
                expected += ['--', '--test-threads=1']
                self.assertEqual(calls[0]['args'], expected)
                self.assertTrue(calls[0]['cwd'].endswith('/terminal-emulator/src/main/rust'))

    def test_default_is_core(self):
        result, calls = self.invoke()
        self.assertEqual(result.returncode, 0)
        selected = [args[i + 1] for c in calls for args in [c['args']]
                    for i, arg in enumerate(args) if arg == '--test']
        self.assertEqual(selected, TARGETS['core'])

    def test_cargo_failure_is_not_hidden(self):
        for status in (1, 101):
            with self.subTest(status=status):
                result, calls = self.invoke('core', status=status)
                self.assertEqual(result.returncode, status)
                self.assertEqual(len(calls), 1)

    def test_unknown_tier_runs_nothing(self):
        result, calls = self.invoke('not-a-tier')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(calls, [])
        self.assertIn('Usage:', result.stderr)

    def test_real_cargo_reports_failure_and_runs_later_binary(self):
        # Tiny independent Rust fixture validates Cargo semantics, not the app.
        with tempfile.TemporaryDirectory(prefix='cargo failure contract ') as directory:
            root = Path(directory)
            (root / 'tests').mkdir()
            (root / 'Cargo.toml').write_text(
                '[package]\nname="runner-contract"\nversion="0.0.0"\nedition="2021"\n')
            (root / 'tests/a_failure.rs').write_text(
                '#[test] fn deliberate_failure() { panic!("expected fixture failure"); }\n')
            (root / 'tests/z_after.rs').write_text(
                '#[test] fn after_failure() { std::fs::write('
                'std::env::var("AFTER_MARKER").unwrap(), "executed").unwrap(); }\n')
            marker = root / 'after.txt'
            env = dict(os.environ, AFTER_MARKER=str(marker), CARGO_TARGET_DIR=str(root / 'target'))
            subprocess.run(['cargo', 'generate-lockfile', '--offline'], cwd=root,
                           env=env, check=True, capture_output=True)
            result = subprocess.run(['cargo', 'test', '--locked', '--offline', '--no-fail-fast',
                                     '--test', 'a_failure', '--test', 'z_after', '--', '--test-threads=1'],
                                    cwd=root, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 101, result.stdout + result.stderr)
            self.assertEqual(marker.read_text(), 'executed')
            self.assertIn('deliberate_failure ... FAILED', result.stdout)
            self.assertIn('after_failure ... ok', result.stdout)

    def test_all_matches_cargo_registration_without_duplicates(self):
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--no-deps', '--format-version=1', '--offline',
             '--manifest-path', str(ROOT / 'terminal-emulator/src/main/rust/Cargo.toml')],
            text=True))
        package = next(p for p in metadata['packages'] if p['name'] == 'termux-rust-new')
        names = [target['name'] for target in package['targets'] if 'test' in target['kind']]
        self.assertEqual(set(TARGETS['all']), set(names))
        self.assertEqual(len(TARGETS['all']), len(set(TARGETS['all'])))
        for tier, targets in TARGETS.items():
            self.assertEqual(len(targets), len(set(targets)), tier)
            self.assertTrue(set(targets) <= set(names), tier)


if __name__ == '__main__':
    unittest.main()
