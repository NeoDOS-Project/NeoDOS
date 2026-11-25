#!/usr/bin/env python3
"""Fail when a new `dead_code` warning appears.

`dead-code-baseline.txt` records the compiler diagnostics that are known and
triaged (intentional APIs, planned features, diagnostics). Run with `--update`
after adding or removing such an item; CI runs the default check mode.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BASELINE = REPO_ROOT / "dead-code-baseline.txt"


def collect(json_lines) -> set[str]:
    keys: set[str] = set()
    for line in json_lines:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-message":
            continue
        diagnostic = message.get("message", {})
        if diagnostic.get("code", {}).get("code") != "dead_code":
            continue
        spans = diagnostic.get("spans", [])
        primary = next((s for s in spans if s.get("is_primary")), spans[0] if spans else None)
        file_name = primary.get("file_name") if primary else "?"
        keys.add(f"{file_name}\t{diagnostic.get('message', '')}")
    return keys


def load_baseline(path: Path) -> set[str]:
    if not path.exists():
        return set()
    return {
        line.rstrip("\n")
        for line in path.read_text().splitlines()
        if line and not line.startswith("#")
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json-file", type=Path, help="cargo build JSON output to read")
    parser.add_argument("--update", action="store_true", help="rewrite the baseline")
    parser.add_argument("--baseline", type=Path, default=BASELINE)
    args = parser.parse_args()

    if args.json_file:
        lines = args.json_file.read_text().splitlines()
    else:
        result = subprocess.run(
            ["cargo", "build", "--message-format=json"],
            cwd=REPO_ROOT / "neodos-kernel",
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            return result.returncode
        lines = result.stdout.splitlines()

    current = collect(lines)

    if args.update:
        header = (
            "# Dead-code baseline (see docs/investigation/dead-code-triage-2026-11.md).\n"
            "# Format: <source file>\\t<compiler message>. Update with:\n"
            "#   python3 scripts/check-dead-code.py --update\n"
        )
        args.baseline.write_text(header + "\n".join(sorted(current)) + "\n")
        print(f"dead-code: baseline updated with {len(current)} entries")
        return 0

    baseline = load_baseline(args.baseline)
    new = sorted(current - baseline)
    removed = len(baseline - current)
    if new:
        print(f"dead-code: {len(new)} new item(s) not in the baseline:")
        for key in new:
            print(f"  {key}")
        print("Delete the code, wire it up, or update the baseline with a documented reason.")
        return 1
    print(f"dead-code: no new items ({len(current)} known, {removed} baseline entries now fixed)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
