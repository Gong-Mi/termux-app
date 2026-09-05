#!/usr/bin/env python3
"""Rust CI configuration contract (requires PyYAML; no Android build).

Run: python3 scripts/verify-rust-android-ci.py
Executes the workflow's build shell with a recording cargo function, for every
ABI and for a failing cargo. This proves command/API routing and failure
propagation, NOT cargo-ndk, Skia compilation, linking or device compatibility.
"""

import re
import subprocess
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
ABIS = {
    "aarch64-linux-android": "arm64-v8a",
    "armv7-linux-androideabi": "armeabi-v7a",
    "i686-linux-android": "x86",
    "x86_64-linux-android": "x86_64",
}


def render(script, target):
    """Interpret only the workflow's restricted matrix ternary-chain syntax."""
    def substitute(match):
        parts = match[1].strip().split(" || ")
        for part in parts[:-1]:
            condition = re.fullmatch(r"matrix.target == '([^']+)' && '([^']+)'", part)
            if condition is None:
                raise AssertionError(f"Unsupported matrix expression: {part}")
            if condition[1] == target:
                return condition[2]
        fallback = re.fullmatch(r"'([^']+)'", parts[-1])
        if fallback is None:
            raise AssertionError(f"Unsupported fallback: {parts[-1]}")
        return fallback[1]

    return re.sub(r"\$\{\{(.*?)\}\}", substitute, script)


class AndroidBuildContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        workflow = yaml.safe_load((ROOT / ".github/workflows/rust-ci.yml").read_text())
        cls.job = workflow["jobs"]["android-build"]
        cls.step = next(s for s in cls.job["steps"] if s.get("name") == "Build for ${{ matrix.target }}")
        properties = (ROOT / "gradle.properties").read_text()
        levels = re.findall(r"^minSdkVersion=(\d+)$", properties, re.M)
        if len(levels) != 1:
            raise AssertionError("Expected exactly one project minSdkVersion")
        cls.api = levels[0]

    def test_gradle_api_is_forwarded_to_cargo_ndk(self):
        module = (ROOT / "terminal-emulator/build.gradle").read_text()
        self.assertRegex(module, r"rustAndroid\s*\{[^}]*minSdkVersion = project.properties.minSdkVersion.toInteger\(\)")
        plugin = (ROOT / "buildSrc/src/main/groovy/com/termux/rust/RustAndroidPlugin.groovy").read_text()
        self.assertIn("'ndk', '-t', abi, '-p', extension.minSdkVersion.toString(), 'build', '--release'", plugin)

    def test_all_abis_remain_required(self):
        self.assertCountEqual(self.job["strategy"]["matrix"]["target"], ABIS)
        self.assertIs(self.job["strategy"]["fail-fast"], False)
        for node in (self.job, self.step):
            self.assertNotIn("if", node)
            self.assertFalse(node.get("continue-on-error", False))

    def test_build_command_matches_gradle_and_preserves_failure(self):
        for target, abi in ABIS.items():
            script = render(self.step["run"], target)
            syntax = subprocess.run(["bash", "-n"], input=script, text=True, capture_output=True)
            self.assertEqual(syntax.returncode, 0, syntax.stderr)
            for status in (0, 37):
                with self.subTest(target=target, cargo_status=status):
                    # Deliberately a shell-boundary test double, not a compiler.
                    recording_cargo = 'cargo() { printf "%s\\n" "$PWD" "$@"; return ' + str(status) + '; }\n'
                    result = subprocess.run(
                        ["bash", "-e", "-c", recording_cargo + script],
                        cwd=ROOT, text=True, capture_output=True,
                    )
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(result.stdout.splitlines(), [
                        str(ROOT / "terminal-emulator/src/main/rust"),
                        "ndk", "-t", abi, "-p", self.api, "build", "--release",
                    ])


if __name__ == "__main__":
    unittest.main(verbosity=2)
