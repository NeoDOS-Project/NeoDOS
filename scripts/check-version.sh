#!/usr/bin/env bash
# Guard against docs drifting from the kernel version (#820).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
ver="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/neodos-kernel/Cargo.toml" | head -n1)"
if [[ -z "$ver" ]]; then
    echo "check-version: cannot read neodos-kernel version" >&2
    exit 1
fi
short="${ver%.*}"

fail=0
grep -q "version-v${ver}-" "$root/README.md" || {
    echo "check-version: README badge is not v${ver}" >&2
    fail=1
}
grep -q "v${ver}" "$root/README.md" || {
    echo "check-version: README status table is not v${ver}" >&2
    fail=1
}
grep -q "Version:\*\* v${short}" "$root/docs/README.md" || {
    echo "check-version: docs/README header is not ${short}" >&2
    fail=1
}
grep -q "Version:\*\* v${ver}" "$root/AGENTS.md" || {
    echo "check-version: AGENTS.md header is not v${ver}" >&2
    fail=1
}

if [[ "$fail" -ne 0 ]]; then
    exit 1
fi
echo "check-version: OK (${ver})"
