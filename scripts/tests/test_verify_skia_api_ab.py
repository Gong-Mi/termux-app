"""Host-only tests; fake ADB results are not emulator acceptance."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import shlex
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "verify-skia-api-ab.py"
spec = importlib.util.spec_from_file_location("skia_ab", SCRIPT)
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


def log(messages, pid=123):
    return "\n".join(f"09-05 22:30:01.123 {pid:5d}   124 I Rust: {message}" for message in messages)


def good(created=4198400):
    return [f"SKIA_API_CONTRACT: created={created} max={created}", *verifier.MARKERS]


class ParsingTests(unittest.TestCase):
    def test_verifier_contract_exists(self):
        self.assertTrue(SCRIPT.is_file(), "A/B verifier implementation missing")

    def test_success_and_actual_created_generalization(self):
        for version in (4198400, 4202496):
            self.assertTrue(verifier.analyze(log(good(version)), {123}, True)["passed"])

    def test_other_pid_pass_cannot_supply_missing_app_evidence(self):
        raw = log(good(), 999) + "\n" + log(good()[:1])
        self.assertFalse(verifier.analyze(raw, {123}, True)["passed"])
        self.assertFalse(verifier.analyze(log(good()), set(), True)["passed"])

    def test_restart_pids_cannot_combine_markers(self):
        raw = log(good()[:3], 123) + "\n" + log([good()[0], *good()[3:]], 456)
        self.assertFalse(verifier.analyze(raw, {123, 456}, True)["passed"])

    def test_target_fatal_overrides_all_pass_markers_and_baseline_failure(self):
        for fatal in ("FATAL EXCEPTION: main", "Fatal signal 6 (SIGABRT)",
                      "SIGABRT", "SIGSEGV"):
            for capped in (False, True):
                with self.subTest(fatal=fatal, capped=capped):
                    messages = good() if capped else [
                        "SKIA_API_CONTRACT: created=4198400 max=0", verifier.FAILURE]
                    self.assertFalse(verifier.analyze(log([*messages, fatal]), {123}, capped)["passed"])
                    self.assertTrue(verifier.analyze(log(messages) + "\n" + log([fatal], 999), {123}, capped)["passed"])

    def test_only_current_pid_can_supply_complete_evidence(self):
        raw = log(good(), 123) + "\n" + log(good()[:1], 456)
        result = verifier.analyze(raw, {123, 456}, True, {456})
        self.assertFalse(result["passed"])
        self.assertEqual(result["current_pids"], ["456"])
        self.assertEqual(result["observed_pids"], ["123", "456"])
        self.assertFalse(verifier.analyze(raw, {123, 456}, True, set())["passed"])
        raw += "\n" + log(good()[1:], 456)
        self.assertTrue(verifier.analyze(raw, {123, 456}, True, {456})["passed"])
        raw += "\n" + log(["SIGSEGV"], 123)
        self.assertFalse(verifier.analyze(raw, {123, 456}, True, {456})["passed"])

    def test_each_marker_required(self):
        messages = good()
        for i in range(len(messages)):
            with self.subTest(missing=messages[i]):
                self.assertFalse(verifier.analyze(log(messages[:i] + messages[i + 1:]), {123}, True)["passed"])

    def test_wrong_cap_and_conflicting_contract_fail(self):
        for contract in ("created=4198400 max=0", "created=4198400 max=4202496", "created=0 max=0"):
            self.assertFalse(verifier.analyze(log(["SKIA_API_CONTRACT: " + contract, *verifier.MARKERS]), {123}, True)["passed"])
        self.assertFalse(verifier.analyze(log([*good(), "SKIA_API_CONTRACT: created=4198400 max=0"]), {123}, True)["passed"])

    def test_pass_prefix_is_not_a_pass_marker(self):
        messages = [message.replace("READBACK: PASS", "READBACK: PASSIVE") for message in good()]
        self.assertFalse(verifier.analyze(log(messages), {123}, True)["passed"])

    def test_baseline_cannot_use_capped_contract(self):
        self.assertFalse(verifier.analyze(log([*good(), verifier.FAILURE]), {123}, False)["passed"])

    def test_non_threadtime_does_not_count(self):
        self.assertFalse(verifier.analyze("\n".join(good()), {123}, True)["passed"])

    def test_failure_in_capped_rejects_pass(self):
        self.assertFalse(verifier.analyze(log([*good(), verifier.FAILURE]), {123}, True)["passed"])

    def test_baseline_failure_is_pid_scoped(self):
        raw = log(["SKIA_API_CONTRACT: created=4198400 max=0"]) + "\n" + log([verifier.FAILURE], 999)
        self.assertFalse(verifier.analyze(raw, {123}, False)["baseline_failure_reproduced"])
        self.assertTrue(verifier.analyze(log(["SKIA_API_CONTRACT: created=4198400 max=0", verifier.FAILURE]), {123}, False)["passed"])


class FakeRunner:
    """A deterministic process boundary, never executes ADB."""
    def __init__(self, original="", reject=None, restore_reject=False,
                 baseline_failure=True, crash_a=False, timeout_a=False,
                 capture_fail=False, missing_pid=False, initial_read_fail=False):
        self.value = original
        self.original = original
        self.reject = reject
        self.restore_reject = restore_reject
        self.baseline_failure = baseline_failure
        self.crash_a = crash_a
        self.timeout_a = timeout_a
        self.capture_fail = capture_fail
        self.missing_pid = missing_pid
        self.initial_read_fail = initial_read_fail
        self.phase = None
        self.calls = []
        self.sets = 0

    def __call__(self, command, **kwargs):
        self.calls.append(command)
        assert kwargs["timeout"] > 0
        assert command[:3] == ["fake-adb", "-s", "fake-serial"]
        args = command[3:]
        output, error, rc = b"", b"", 0
        if args[:2] == ["shell", "setprop"]:
            self.sets += 1
            values = shlex.split(args[-1])
            assert len(values) == 1, "setprop must receive exactly one quoted value"
            value = values[0]
            if self.sets <= 2:
                self.phase = value
            if value == self.reject or (self.sets == 3 and self.restore_reject):
                rc, error = 1, b"property service denied"
            else:
                self.value = value
        elif args[:2] == ["shell", "getprop"]:
            if self.initial_read_fail and self.sets == 0:
                raise OSError("cannot read original property")
            output = (self.value + "\n").encode()
        elif args[:2] == ["shell", "monkey"] and self.phase == "0":
            if self.crash_a:
                raise OSError("baseline launch exception")
            if self.timeout_a:
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
        elif args[:2] == ["shell", "pidof"]:
            output, rc = (b"", 1) if self.missing_pid else (b"123\n", 0)
        elif args[:2] == ["logcat", "-d"]:
            messages = good() if self.phase == "1" else ["SKIA_API_CONTRACT: created=4198400 max=0"]
            if self.phase == "0" and self.baseline_failure:
                messages += [verifier.FAILURE]
            output = log(messages).encode()
        elif args[:2] == ["exec-out", "screencap"]:
            if self.capture_fail:
                raise OSError("capture failed")
            output = b"\x89PNG\r\n\x1a\nFAKE, NOT DEVICE EVIDENCE"
        elif args[:2] == ["shell", "dumpsys"]:
            output = b"FAKE activities"
        return subprocess.CompletedProcess(command, rc, output, error)


class RunnerTests(unittest.TestCase):
    def run_fake(self, fake, require=False):
        with tempfile.TemporaryDirectory() as temporary:
            args = argparse.Namespace(adb="fake-adb", serial="fake-serial", package="com.termux",
                                      output=Path(temporary), timeout=0.01,
                                      require_baseline_failure=require)
            summary = verifier.run_ab(args, fake, clock=lambda: 0, sleep=lambda _: None)
            saved = json.loads((args.output / "summary.json").read_text())
            self.assertEqual(saved["passed"], summary["passed"])
            self.records = json.loads((args.output / "commands.json").read_text())
            if not fake.initial_read_fail:
                for phase in ("baseline", "capped"):
                    for filename in ("logcat.txt", "pid.txt", "activities.txt", "screenshot.png"):
                        self.assertTrue((args.output / phase / filename).is_file())
            self.assertTrue(all(r["timeout"] == 0.01 for r in self.records))
            return summary

    def test_success_artifacts_and_empty_property_restore(self):
        fake = FakeRunner()
        result = self.run_fake(fake, require=True)
        self.assertTrue(result["passed"])
        self.assertEqual(fake.value, "")
        self.assertEqual(result["restoration"]["readback"], "")
        self.assertEqual(result["baseline_status"], "reproduced")
        self.assertFalse(any("install" in c or "uninstall" in c for c in fake.calls))
        for index, command in enumerate(fake.calls):
            if "setprop" in command:
                self.assertIn("getprop", fake.calls[index + 1])

    def test_nonempty_property_restored(self):
        fake = FakeRunner(original="1")
        self.assertTrue(self.run_fake(fake)["passed"])
        self.assertEqual(fake.value, "1")

    def test_property_shell_quoting_roundtrip(self):
        for value in ("", "  spaced value  ", "semi;$(id)|&<>*", "a'b\"c"):
            with self.subTest(value=value):
                fake = FakeRunner(original=value)
                result = self.run_fake(fake)
                self.assertTrue(result["passed"])
                self.assertEqual(fake.value, value)
                self.assertEqual(result["original_property"], value)
                self.assertEqual([c for c in fake.calls if "setprop" in c][-1][-1], shlex.quote(value))

    def test_pid_death_or_restart_after_capture_rejects_stale_success(self):
        for phase in ("0", "1"):
            for final_pid in (b"", b"456\n"):
                with self.subTest(phase=phase, final_pid=final_pid):
                    class DiesAfterCapture(FakeRunner):
                        def __call__(self, command, **kwargs):
                            result = super().__call__(command, **kwargs)
                            if "pidof" in command and self.phase == phase and any(
                                "screencap" in c for c in self.calls[
                                    max(i for i, c in enumerate(self.calls) if "setprop" in c):]):
                                return subprocess.CompletedProcess(command, 0 if final_pid else 1, final_pid, b"")
                            return result
                    result = self.run_fake(DiesAfterCapture())
                    name = "baseline" if phase == "0" else "capped"
                    self.assertFalse(result[name]["passed"])
                    self.assertEqual(result[name]["current_pids"], ["456"] if final_pid else [])
                    self.assertIn("123", result[name]["observed_pids"])
                    self.assertTrue(result["restoration"]["passed"])
                    if phase == "0":
                        self.assertTrue(result["capped"]["passed"])

    def test_final_pid_query_failure_clears_stale_liveness(self):
        class QueryFails(FakeRunner):
            def __call__(self, command, **kwargs):
                result = super().__call__(command, **kwargs)
                if "pidof" in command and self.phase == "1" and any(
                    "screencap" in c for c in self.calls[
                        max(i for i, c in enumerate(self.calls) if "setprop" in c):]):
                    raise OSError("final pid service unavailable")
                return result
        result = self.run_fake(QueryFails())
        self.assertFalse(result["passed"])
        self.assertEqual(result["capped"]["current_pids"], [])
        self.assertEqual(result["capped"]["observed_pids"], ["123"])
        self.assertTrue(any("final pidof" in error for error in result["capped"]["errors"]))
        self.assertTrue(result["restoration"]["passed"])

    def test_final_capture_fatal_rejects_phase_but_restores(self):
        for phase in ("0", "1"):
            class FatalOnCapture(FakeRunner):
                def __call__(self, command, **kwargs):
                    result = super().__call__(command, **kwargs)
                    reads = sum(c[3:5] == ["logcat", "-d"] for c in self.calls[
                        max(i for i, c in enumerate(self.calls) if "setprop" in c):]) if self.sets else 0
                    if command[3:5] == ["logcat", "-d"] and self.phase == phase and reads > 1:
                        result.stdout += ("\n" + log(["FATAL EXCEPTION: main"])).encode()
                    return result
            result = self.run_fake(FatalOnCapture())
            self.assertFalse(result["baseline" if phase == "0" else "capped"]["passed"])
            self.assertTrue(result["restoration"]["passed"])
            if phase == "0":
                self.assertTrue(result["capped"]["passed"])

    def test_baseline_requirement_and_non_reproduction(self):
        for require in (False, True):
            result = self.run_fake(FakeRunner(baseline_failure=False), require=require)
            self.assertEqual(result["passed"], not require)
            self.assertEqual(result["baseline_status"], "not_reproduced")

    def test_a_exception_or_timeout_does_not_prevent_b_or_restore(self):
        for kwargs in ({"crash_a": True}, {"timeout_a": True}):
            result = self.run_fake(FakeRunner(**kwargs))
            self.assertFalse(result["passed"])
            self.assertTrue(result["capped"]["passed"])
            self.assertTrue(result["restoration"]["passed"])
            self.assertTrue(result["baseline"]["errors"])

    def test_property_denial_still_read_back_and_b_runs(self):
        fake = FakeRunner(reject="0")
        result = self.run_fake(fake)
        self.assertFalse(result["passed"])
        self.assertTrue(result["capped"]["passed"])
        self.assertTrue(result["restoration"]["passed"])
        rejected = next(i for i, c in enumerate(fake.calls) if "setprop" in c)
        self.assertIn("getprop", fake.calls[rejected + 1])

    def test_capped_property_denial_fails(self):
        result = self.run_fake(FakeRunner(reject="1"))
        self.assertFalse(result["passed"])
        self.assertFalse(result["capped"]["passed"])
        self.assertTrue(result["restoration"]["passed"])

    def test_silent_property_refusal_detected(self):
        fake = FakeRunner()
        def runner(command, **kwargs):
            result = fake(command, **kwargs)
            if "setprop" in command and command[-1] == "0":
                fake.value = "ignored"
            return result
        with tempfile.TemporaryDirectory() as temp:
            args = argparse.Namespace(adb="fake-adb", serial="fake-serial", package="com.termux",
                                      output=Path(temp), timeout=0.01, require_baseline_failure=False)
            result = verifier.run_ab(args, runner, sleep=lambda _: None)
            self.assertIn("readback mismatch", result["baseline"]["errors"][0])
            self.assertTrue(result["capped"]["passed"])
            self.assertFalse(result["passed"])

    def test_restore_refusal_makes_entire_run_fail(self):
        result = self.run_fake(FakeRunner(restore_reject=True))
        self.assertTrue(result["capped"]["passed"])
        self.assertFalse(result["restoration"]["passed"])
        self.assertFalse(result["passed"])

    def test_capture_failure_does_not_skip_restoration(self):
        result = self.run_fake(FakeRunner(capture_fail=True))
        self.assertFalse(result["passed"])
        self.assertTrue(result["restoration"]["passed"])
        self.assertTrue(result["capped"]["errors"])

    def test_missing_pid_has_bounded_poll_and_cannot_pass(self):
        fake = FakeRunner(missing_pid=True)
        result = self.run_fake(fake)
        self.assertFalse(result["passed"])
        self.assertLess(len(fake.calls), 40)
        self.assertTrue(result["restoration"]["passed"])

    def test_unknown_original_aborts_without_property_write(self):
        fake = FakeRunner(initial_read_fail=True)
        result = self.run_fake(fake)
        self.assertFalse(result["passed"])
        self.assertFalse(result["restoration"]["attempted"])
        self.assertFalse(any("setprop" in c for c in fake.calls))

    def test_timeout_validation(self):
        for value in ("0", "-1", "nan", "inf"):
            with self.assertRaises(argparse.ArgumentTypeError):
                verifier.positive_timeout(value)
        self.assertEqual(verifier.positive_timeout("45"), 45.0)


if __name__ == "__main__":
    unittest.main()
