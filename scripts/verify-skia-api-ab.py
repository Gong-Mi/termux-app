#!/usr/bin/env python3
"""Verify an already-installed debug APK without installing/changing APKs.

Host tests use fake ADB only; running this CLI requires an authorized target.
Exit 0 means capped evidence, baseline policy, and property restoration passed.
"""
import argparse
import json
import math
from pathlib import Path
import re
import shlex
import subprocess
import time

PROPERTY = "debug.termux.skia_api_cap"
MARKERS = (
    "VulkanContext::new: Skia context created and optimized",
    "VulkanContext::new: SUCCESS",
    "RenderThread: Frame 0 completed",
    "SKIA_BACKEND_READBACK: PASS",
)
FAILURE = "Skia make_vulkan failed"
THREADTIME = re.compile(r"^\s*\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d+\s+(\d+)\s+\d+\s+[VDIWEFAS]\s+[^:]+:\s?(.*)$")
CONTRACT = re.compile(r"SKIA_API_CONTRACT: created=(\d+) max=(\d+)(?!\d)")


FATAL = re.compile(r"\b(?:FATAL EXCEPTION|Fatal signal|SIGABRT|SIGSEGV)\b")


def analyze(raw, pids, capped, current_pids=None):
    """Evaluate each PID independently; callers supply post-capture live PIDs.

    Baseline policy permits make_vulkan failure, never a fatal or dead process.
    Omitting current_pids is for standalone parsing of a known-live PID set.
    """
    wanted = {str(pid) for pid in pids}
    current = wanted if current_pids is None else {str(pid) for pid in current_pids}
    by_pid = {pid: [] for pid in wanted}
    for line in raw.splitlines():
        match = THREADTIME.match(line)
        if match and match.group(1) in wanted:
            by_pid[match.group(1)].append(match.group(2))
    evidence = {}
    fatals = []
    for pid, messages in sorted(by_pid.items()):
        text = "\n".join(messages)
        contracts = [(int(m.group(1)), int(m.group(2)))
                     for m in CONTRACT.finditer(text)]
        contract_ok = bool(contracts) and all(
            created > 0 and maximum == (created if capped else 0)
            for created, maximum in contracts)
        missing = [marker for marker in MARKERS
                   if not re.search(re.escape(marker) + r"(?!\w)", text)] if capped else []
        failed = FAILURE in text
        pid_fatals = [message for message in messages if FATAL.search(message)]
        fatals.extend({"pid": pid, "message": message} for message in pid_fatals)
        evidence[pid] = {"contracts": contracts, "contract_ok": contract_ok,
                         "missing_markers": missing, "baseline_failure_reproduced": failed,
                         "fatal_errors": pid_fatals,
                         "passed": pid in current and contract_ok and not missing and not pid_fatals
                                   and (not capped or not failed)}
    eligible = [pid for pid, item in evidence.items() if item["passed"]]
    # Diagnostics retain per-PID missing markers rather than unioning successes.
    missing = {pid: item["missing_markers"] for pid, item in evidence.items()}
    return {"pids": sorted(wanted), "observed_pids": sorted(wanted),
            "current_pids": sorted(current), "pid_evidence": evidence,
            "eligible_pids": eligible, "fatal_errors": fatals,
            "contracts": [pair for item in evidence.values() for pair in item["contracts"]],
            "contract_ok": any(item["contract_ok"] for item in evidence.values()),
            "missing_markers": missing,
            "baseline_failure_reproduced": any(item["baseline_failure_reproduced"] for item in evidence.values()),
            "passed": bool(eligible) and not fatals
                      and (not capped or not any(item["baseline_failure_reproduced"] for item in evidence.values()))}


class Adb:
    def __init__(self, args, runner=subprocess.run):
        self.prefix = [args.adb] + (["-s", args.serial] if args.serial else [])
        self.timeout = min(args.timeout, 10.0)
        self.runner = runner
        self.records = []

    def call(self, *args, check=True):
        command = self.prefix + list(args)
        record = {"command": command, "timeout": self.timeout}
        self.records.append(record)
        try:
            result = self.runner(command, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, timeout=self.timeout)
            record.update(returncode=result.returncode,
                          stdout=result.stdout.decode("utf-8", "replace") if "screencap" not in args else "<binary screenshot>",
                          stderr=result.stderr.decode("utf-8", "replace"))
            if check and result.returncode:
                raise RuntimeError(f"ADB exit {result.returncode}: {record['stderr']}")
            return result.stdout
        except Exception as exc:
            record["error"] = str(exc)
            raise

    def text(self, *args):
        return self.call(*args).decode("utf-8", "replace")

    def property(self, value):
        # Read back even when setprop itself reports an error.
        error = None
        try:
            self.call("shell", "setprop", PROPERTY, shlex.quote(value))
        except Exception as exc:
            error = exc
        actual = self.text("shell", "getprop", PROPERTY).removesuffix("\n").removesuffix("\r")
        if error:
            raise error
        if actual != value:
            raise RuntimeError(f"property readback mismatch: expected {value!r}, got {actual!r}")
        return actual


def run_phase(adb, args, name, value, clock=time.monotonic, sleep=time.sleep):
    directory = args.output / name
    directory.mkdir(parents=True, exist_ok=True)
    errors, pids = [], set()
    current_pids = set()
    raw = ""
    result = {"property": value, "errors": errors,
              "liveness_policy": "require_live_evidence_pid_even_for_baseline"}
    # Always leave named evidence files, including on setup failure.
    for filename in ("logcat.txt", "pid.txt", "activities.txt", "screenshot.png"):
        (directory / filename).write_bytes(b"")
    try:
        result["property_readback"] = adb.property(value)
        adb.call("shell", "am", "force-stop", args.package)
        adb.call("logcat", "-c")
        adb.call("shell", "monkey", "-p", args.package,
                 "-c", "android.intent.category.LAUNCHER", "1")
        deadline = clock() + args.timeout
        # Count bound also protects tests/runners whose clock does not advance.
        for _ in range(max(1, math.ceil(args.timeout / 0.25))):
            pid_output = adb.call("shell", "pidof", args.package, check=False).decode("utf-8", "replace")
            with (directory / "pid.txt").open("a") as stream:
                stream.write(pid_output + "\n")
            current_pids = {token for token in pid_output.split() if token.isdecimal()}
            pids.update(current_pids)
            raw = adb.text("logcat", "-d", "-v", "threadtime")
            (directory / "logcat.txt").write_text(raw)
            parsed = analyze(raw, pids, value == "1", current_pids)
            if parsed["passed"] and (value == "1" or parsed["baseline_failure_reproduced"]):
                break
            if clock() >= deadline:
                break
            sleep(min(0.25, max(0, deadline - clock())))
    except Exception as exc:
        errors.append(str(exc))
    finally:
        # Independent captures: one unavailable service must not suppress others.
        for filename, command in (
            ("logcat.txt", ("logcat", "-d", "-v", "threadtime")),
            ("activities.txt", ("shell", "dumpsys", "activity", "activities")),
            ("screenshot.png", ("exec-out", "screencap", "-p")),
        ):
            try:
                data = adb.call(*command)
                (directory / filename).write_bytes(data)
                if filename == "logcat.txt":
                    raw = data.decode("utf-8", "replace")
            except Exception as exc:
                errors.append(f"{filename}: {exc}")
        # Recheck after ALL captures; historical pidof output is not liveness.
        current_pids = set()
        try:
            pid_output = adb.call("shell", "pidof", args.package, check=False).decode("utf-8", "replace")
            current_pids = {token for token in pid_output.split() if token.isdecimal()}
            pids.update(current_pids)
            with (directory / "pid.txt").open("a") as stream:
                stream.write("final: " + pid_output + "\n")
        except Exception as exc:
            errors.append(f"final pidof: {exc}")
    result.update(analyze(raw, pids, value == "1", current_pids))
    if not current_pids:
        errors.append("no live target PID after capture (baseline also requires liveness)")
    elif not result["eligible_pids"]:
        errors.append("no current PID has complete valid phase evidence")
    if result["fatal_errors"]:
        errors.append("target PID fatal crash detected")
    result["passed"] = result["passed"] and not errors
    return result


def run_ab(args, runner=subprocess.run, clock=time.monotonic, sleep=time.sleep):
    args.output = Path(args.output)
    args.output.mkdir(parents=True, exist_ok=True)
    adb = Adb(args, runner)
    summary = {"package": args.package, "serial": args.serial,
               "require_baseline_failure": args.require_baseline_failure,
               "errors": [], "passed": False}
    original = None
    try:
        original = adb.text("shell", "getprop", PROPERTY).removesuffix("\n").removesuffix("\r")
        summary["original_property"] = original
        for name, value in (("baseline", "0"), ("capped", "1")):
            try:
                summary[name] = run_phase(adb, args, name, value, clock, sleep)
            except Exception as exc:
                summary[name] = {"passed": False, "errors": [str(exc)]}
        reproduced = summary["baseline"].get("baseline_failure_reproduced", False)
        summary["baseline_status"] = "reproduced" if reproduced else "not_reproduced"
        summary["passed"] = (summary["baseline"]["passed"] and summary["capped"]["passed"]
                             and (reproduced or not args.require_baseline_failure))
    except Exception as exc:
        summary["errors"].append(str(exc))
    finally:
        summary["restoration"] = {"passed": False, "attempted": original is not None}
        if original is not None:
            try:
                actual = adb.property(original)
                summary["restoration"].update(passed=True, readback=actual)
            except Exception as exc:
                summary["restoration"]["error"] = str(exc)
        summary["passed"] = summary["passed"] and summary["restoration"]["passed"]
        (args.output / "commands.json").write_text(json.dumps(adb.records, indent=2) + "\n")
        (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    return summary


def positive_timeout(value):
    result = float(value)
    if not math.isfinite(result) or result <= 0:
        raise argparse.ArgumentTypeError("timeout must be finite and positive")
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb", default="adb")
    parser.add_argument("--serial")
    parser.add_argument("--package", default="com.termux")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=positive_timeout, default=45.0)
    parser.add_argument("--require-baseline-failure", action="store_true")
    result = run_ab(parser.parse_args(argv))
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
