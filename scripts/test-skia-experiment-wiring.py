#!/usr/bin/env python3
"""Static build/CI contract only; not a Gradle or emulator execution test."""
import unittest
from pathlib import Path
import yaml

ROOT = Path(__file__).resolve().parents[1]


class SkiaExperimentWiring(unittest.TestCase):
    def test_experiment_is_explicit_and_invalidates_native_output(self):
        plugin = (ROOT / 'buildSrc/src/main/groovy/com/termux/rust/RustAndroidPlugin.groovy').read_text()
        self.assertIn('findProperty("skiaApiExperiment")', plugin)
        self.assertIn('inputs.property("skiaApiExperiment", skiaApiExperiment)', plugin)
        self.assertIn("args '--features', 'skia-api-experiment'", plugin)
        self.assertIn('if (skiaApiExperiment)', plugin)
        self.assertIn('taskGraph.whenReady', plugin)
        self.assertIn('!variant.buildType.debuggable', plugin)
        self.assertIn('throw new GradleException', plugin)

    def test_exact_head_ab_has_strict_oracle_and_preserves_artifacts(self):
        workflow = yaml.safe_load((ROOT / '.github/workflows/android-emulator-experiment.yml').read_text())
        self.assertIn('fix/engine-handle-lifetime', workflow['on']['pull_request']['branches'])
        self.assertIn('scripts/verify-skia-api-ab.py', workflow['on']['pull_request']['paths'])
        steps = workflow['jobs']['install-startup']['steps']
        checkout = next(s for s in steps if s['name'] == 'Checkout exact source')
        self.assertEqual(checkout['with']['ref'], '${{ github.event.pull_request.head.sha || github.sha }}')
        build = next(s for s in steps if s['name'] == 'Build debug APKs')
        self.assertIn('-PskiaApiExperiment=true', build['run'])
        ab = next(s for s in steps if s['name'] == 'Install exact APK and verify Skia API A/B')
        self.assertIn('scripts/verify-skia-api-ab.py', ab['run'])
        self.assertIn('--require-baseline-failure', ab['run'])
        self.assertNotIn('continue-on-error', ab)
        upload = next(s for s in steps if s['name'] == 'Upload emulator evidence')
        self.assertEqual(upload['if'], 'always()')
        self.assertIn('skia-api-ab/', upload['with']['path'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
