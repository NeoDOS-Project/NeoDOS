#!/usr/bin/env python3
"""
regression_runner.py — NeoDOS Deterministic Regression Test Runner

Runs `auto_test.py` multiple times (default: 100) and produces a structured
regression report with pass/fail counts, crash frequency, and panic signatures.

Usage:
    python3 scripts/regression_runner.py [--iterations N] [--qemu-accel kvm|tcg]

Output: Full report to stdout + regression_report.log
"""

import subprocess
import sys
import os
import re
import time
import json
from collections import Counter, defaultdict
from datetime import datetime

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
DEFAULT_ITERATIONS = 20  # 100 in production; 20 for quick validation

# Panic signatures we scan for in serial output
PANIC_SIGNATURES = [
    r"KERNEL PANIC \(CLASS: (\w+)\)",
    r"DOUBLE FAULT",
    r"GPF: error=([0-9a-fx]+)",
    r"Page fault @ (0x[0-9a-f]+)",
    r"panic!",
    r"\[ASSERT\]",
    r"\[INVARIANT\]",
]

def run_single_iteration(iteration: int, qemu_accel: str) -> dict:
    """Run one auto_test.py iteration and return structured results."""
    env = os.environ.copy()
    env["QEMU_ACCEL"] = qemu_accel

    start = time.time()
    proc = subprocess.run(
        [sys.executable, os.path.join(SCRIPT_DIR, "auto_test.py")],
        cwd=PROJECT_ROOT,
        capture_output=True,
        text=True,
        timeout=180,
        env=env,
    )
    elapsed = time.time() - start

    stdout = proc.stdout
    stderr = proc.stderr

    # Determine pass/fail
    passed = "OVERALL: SUCCESS" in stdout

    # Extract test counts
    kernel_tests = None
    user_binaries = None
    m = re.search(r"Kernel tests: ([\d]+) tests passed", stdout)
    if m:
        kernel_tests = int(m.group(1))
    m = re.search(r"All ([\d]+) user-mode binaries executed", stdout)
    if m:
        user_binaries = int(m.group(1))

    # Scan for panic signatures
    panics_found = []
    for sig in PANIC_SIGNATURES:
        for m in re.finditer(sig, stdout + stderr):
            panics_found.append(m.group(0))

    # Scan for invariant violations
    invariants_found = []
    for m in re.finditer(r"\[INVARIANT\] (.*?)(?:\n|$)", stdout + stderr):
        invariants_found.append(m.group(1).strip())

    # Check for timeouts
    timeout = "TIMEOUT" in stderr or "timeout" in stderr.lower()

    return {
        "iteration": iteration,
        "passed": passed,
        "elapsed_seconds": round(elapsed, 1),
        "kernel_tests_passed": kernel_tests,
        "user_binaries_executed": user_binaries,
        "panics": panics_found,
        "invariants": invariants_found,
        "timeout": timeout,
        "stdout_snippet": stdout[-500:] if stdout else "",
    }


def print_report(results: list, elapsed_total: float):
    """Print structured regression report."""
    total = len(results)
    passed_count = sum(1 for r in results if r["passed"])
    failed_count = total - passed_count
    panic_counter = Counter()
    invariant_counter = Counter()
    timeout_count = sum(1 for r in results if r["timeout"])

    for r in results:
        for p in r["panics"]:
            panic_counter[p] += 1
        for inv in r["invariants"]:
            invariant_counter[inv] += 1

    print("=" * 70)
    print("  NeoDOS REGRESSION REPORT")
    print(f"  Generated: {datetime.now().isoformat()}")
    print("=" * 70)
    print(f"\n  Iterations:   {total}")
    print(f"  Passed:       {passed_count}")
    print(f"  Failed:       {failed_count}")
    print(f"  Timeouts:     {timeout_count}")
    print(f"  Pass rate:    {passed_count/total*100:.1f}%")
    print(f"  Total time:   {elapsed_total:.0f}s")
    print(f"  Avg time:     {elapsed_total/total:.1f}s")

    if panic_counter:
        print(f"\n  ── Panic Signatures ({sum(panic_counter.values())} total) ──")
        for signature, count in panic_counter.most_common():
            print(f"    [{count:3d}x] {signature}")
            for r in results:
                if signature in r["panics"]:
                    print(f"           iteration {r['iteration']} (elapsed {r['elapsed_seconds']}s)")
                    break

    if invariant_counter:
        print(f"\n  ── Invariant Violations ({sum(invariant_counter.values())} total) ──")
        for inv, count in invariant_counter.most_common():
            print(f"    [{count:3d}x] {inv}")

    # Per-iteration summary
    print(f"\n  ── Per-Iteration Summary ──")
    print(f"  {'#':>4s}  {'Status':8s}  {'Time':>5s}  {'KTests':>7s}  {'UBins':>6s}  {'Panics':>7s}")
    print(f"  {'─'*4}  {'─'*8}  {'─'*5}  {'─'*7}  {'─'*6}  {'─'*7}")
    for r in results:
        status = "PASS" if r["passed"] else "FAIL"
        kt = str(r["kernel_tests_passed"]) if r["kernel_tests_passed"] is not None else "N/A"
        ub = str(r["user_binaries_executed"]) if r["user_binaries_executed"] is not None else "N/A"
        pn = str(len(r["panics"]))
        print(f"  {r['iteration']:4d}  {status:8s}  {r['elapsed_seconds']:4.0f}s  {kt:>7s}  {ub:>6s}  {pn:>7s}")

    if failed_count > 0:
        print(f"\n  ── Failing Iterations ──")
        for r in results:
            if not r["passed"]:
                print(f"    Iteration {r['iteration']}: {r['elapsed_seconds']}s, panics={r['panics']}")
                snippet = r["stdout_snippet"][-300:]
                print(f"      Last output: {repr(snippet[:200])}")

    overall = "REGRESSION: PASS" if failed_count == 0 else "REGRESSION: FAIL"
    print(f"\n  {'='*50}")
    print(f"  {overall}")
    print(f"  {'='*50}")

    return failed_count == 0


def main():
    iterations = DEFAULT_ITERATIONS
    qemu_accel = os.environ.get("QEMU_ACCEL", "tcg")

    args = sys.argv[1:]
    i = 0
    while i < len(args):
        if args[i] == "--iterations" and i + 1 < len(args):
            iterations = int(args[i + 1])
            i += 2
        elif args[i] == "--qemu-accel" and i + 1 < len(args):
            qemu_accel = args[i + 1]
            i += 2
        else:
            print(f"Unknown arg: {args[i]}")
            sys.exit(1)

    print(f"[*] Regression Runner: {iterations} iterations, qemu={qemu_accel}")
    print(f"[*] Starting at {datetime.now().isoformat()}")

    # Quick build check before lengthy regression
    print("[*] Building kernel...")
    build = subprocess.run(
        ["bash", "scripts/build.sh", "--neodos-image"],
        cwd=PROJECT_ROOT,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if build.returncode != 0:
        print("[FAIL] Build failed!")
        print(build.stderr[-1000:])
        sys.exit(1)
    print("[OK] Build succeeded.")

    results = []
    start_total = time.time()

    for iteration in range(1, iterations + 1):
        print(f"\n[{iteration}/{iterations}] Running iteration {iteration}...", end=" ")
        sys.stdout.flush()
        result = run_single_iteration(iteration, qemu_accel)
        results.append(result)
        status = "PASS" if result["passed"] else "FAIL"
        print(f"{status} ({result['elapsed_seconds']}s)", end="")
        if result["panics"]:
            print(f" panics={len(result['panics'])}", end="")
        print()

        if not result["passed"]:
            # Stop early on first failure to avoid wasting time
            print(f"  [!] Failure detected at iteration {iteration} — stopping.")
            break

    elapsed_total = time.time() - start_total

    # Write full log
    log_path = os.path.join(PROJECT_ROOT, "regression_report.log")
    with open(log_path, "w") as f:
        json.dump({
            "timestamp": datetime.now().isoformat(),
            "iterations": iterations,
            "qemu_accel": qemu_accel,
            "results": results,
            "total_elapsed": round(elapsed_total, 1),
        }, f, indent=2, default=str)

    print(f"\n[*] Full report saved to: {log_path}")

    # Print human-readable report
    passed = print_report(results, elapsed_total)

    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
