#!/usr/bin/env python3
"""Static topology/cache contracts; not a measured CI speedup."""
from pathlib import Path
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[1]


class BuildContracts(unittest.TestCase):
    def test_process_owner_stack_keeps_all_acceptance_routes(self):
        for name in ('rust-ci.yml', 'rust-quality.yml', 'engine-construction.yml',
                     'android-emulator-experiment.yml'):
            with self.subTest(workflow=name):
                workflow = yaml.safe_load((ROOT / '.github/workflows' / name).read_text())
                self.assertIn('fix/pty-io-lifecycle', workflow.get('on', workflow.get(True, {}))['pull_request']['branches'])

    def test_completion_stack_keeps_all_acceptance_routes(self):
        for name in ('rust-ci.yml', 'rust-quality.yml', 'engine-construction.yml',
                     'android-emulator-experiment.yml'):
            with self.subTest(workflow=name):
                workflow = yaml.safe_load((ROOT / '.github/workflows' / name).read_text())
                self.assertIn('fix/session-process-owner', workflow.get('on', workflow.get(True, {}))['pull_request']['branches'])

    def test_delivery_stack_keeps_all_acceptance_routes_and_real_jni_mode(self):
        for name in ('rust-ci.yml', 'rust-quality.yml', 'engine-construction.yml',
                     'android-emulator-experiment.yml'):
            workflow = yaml.safe_load((ROOT / '.github/workflows' / name).read_text())
            self.assertIn('fix/session-completion-observation', workflow.get('on', workflow.get(True, {}))['pull_request']['branches'])
        workflow = yaml.safe_load((ROOT / '.github/workflows/engine-construction.yml').read_text())
        commands = '\n'.join(step.get('run', '') for step in workflow['jobs']['constructor-contract']['steps'])
        self.assertIn("('contract', 'native', 'handles', 'delivery')", commands)
        self.assertIn('scripts/java/delivery-stubs/**', workflow['on']['pull_request']['paths'])

    def test_candidate_stack_keeps_all_acceptance_routes(self):
        for name in ('rust-ci.yml', 'rust-quality.yml', 'engine-construction.yml',
                     'android-emulator-experiment.yml'):
            with self.subTest(workflow=name):
                workflow = yaml.safe_load((ROOT / '.github/workflows' / name).read_text())
                self.assertIn('fix/session-delivery-claim',
                              workflow.get('on', workflow.get(True, {}))['pull_request']['branches'])

    def test_host_tiers_keep_coverage_and_isolate_dependency_cache(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/rust-quality.yml').read_text())
        self.assertEqual(workflow['jobs']['correctness']['strategy']['matrix']['tier'],
                         ['core', 'regressions', 'terminal', 'lifecycle'])
        self.assertFalse(workflow['jobs']['correctness']['strategy']['fail-fast'])
        for event in ('push', 'pull_request'):
            self.assertIn('scripts/tests/test_rust_test_runner.py', workflow['on'][event]['paths'])
            self.assertIn('scripts/tests/rust-tier-targets.json', workflow['on'][event]['paths'])
        for job_id in ('correctness', 'manual'):
            steps = workflow['jobs'][job_id]['steps']
            cache = next(s for s in steps if s.get('uses') == 'Swatinem/rust-cache@v2')
            config = cache['with']
            self.assertEqual(config['cache-bin'], 'false')
            self.assertEqual(config['cache-on-failure'], 'true')
            self.assertIn('tier', config['key'])
            self.assertIn('ImageVersion', config['env-vars'])
            run = next(s for s in steps if 'bash ./scripts/run-rust-tests.sh' in s.get('run', ''))
            self.assertNotIn('if', run)
            self.assertNotIn('continue-on-error', run)
            self.assertTrue(any('python3 scripts/tests/test_rust_test_runner.py' in s.get('run', '') for s in steps))

    def test_emulator_cache_is_not_android_default_feature_cache(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/android-emulator-experiment.yml').read_text())
        steps = workflow['jobs']['install-startup']['steps']
        cache = next(s for s in steps if s.get('name') == 'Cache emulator native dependencies')
        config = cache['with']
        self.assertEqual(cache['uses'], 'Swatinem/rust-cache@v2')
        self.assertIn('skia-api-experiment', config['key'])
        self.assertIn('x86_64', config['key'])
        self.assertIn('steps.ndk.outputs.identity', config['key'])
        self.assertEqual(config['cache-bin'], 'false')
        for prefix in ('SKIA', 'FORCE_SKIA', 'ANDROID_NDK', 'CXX', 'ImageVersion'):
            self.assertIn(prefix, config['env-vars'])
        identity = next(s for s in steps if s.get('id') == 'ndk')
        self.assertIn('source.properties', identity['run'])
        self.assertIn('GITHUB_ENV', identity['run'])
        build = next(s for s in steps if s.get('name') == 'Build debug APKs')
        self.assertLess(steps.index(identity), steps.index(cache))
        self.assertLess(steps.index(cache), steps.index(build))
        self.assertNotIn('if', build)
        self.assertIn('assembleDebug -PskiaApiExperiment=true', build['run'])
        self.assertTrue(any('--require-baseline-failure' in s.get('run', '') for s in steps))


if __name__ == '__main__':
    unittest.main()
