#!/usr/bin/env bash
# Verify that data/keyboard/<tag>.kbd are non-empty and in sync with their
# .klc sources (issue #702).
#
# Fails if a .kbd is missing, maps zero scancodes, or differs from a fresh
# kbdcompile run. Run from anywhere.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

(cd tools/kbdcompile && cargo build --quiet)
bin=tools/kbdcompile/target/debug/kbdcompile

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail=0
shopt -s nullglob
found=0
for klc in data/keyboard/*.klc; do
    found=1
    tag="$(basename "$klc" .klc)"
    kbd="data/keyboard/$tag.kbd"

    if [ ! -f "$kbd" ]; then
        echo "check-keyboard-layouts: missing $kbd" >&2
        fail=1
        continue
    fi

    if ! "$bin" "$klc" "$tmp/$tag.kbd" "$tag" "$tag" 2>"$tmp/$tag.err"; then
        echo "check-keyboard-layouts: kbdcompile failed for $klc" >&2
        fail=1
        continue
    fi

    # Reject an empty layout (every scancode 0xFFFF).
    if ! grep -qE '\([1-9][0-9]* scancodes' "$tmp/$tag.err"; then
        echo "check-keyboard-layouts: $klc produces an empty layout" >&2
        fail=1
    fi

    # Reject a stale committed artifact.
    if ! cmp -s "$tmp/$tag.kbd" "$kbd"; then
        echo "check-keyboard-layouts: $kbd is stale (differs from a fresh compile)" >&2
        fail=1
    fi
done

if [ "$found" -eq 0 ]; then
    echo "check-keyboard-layouts: no .klc sources found under data/keyboard/" >&2
    exit 1
fi

if [ "$fail" -ne 0 ]; then
    echo "check-keyboard-layouts: FAILED — regenerate with scripts/gen-keyboard-layouts.sh" >&2
    exit 1
fi
echo "check-keyboard-layouts: OK"
