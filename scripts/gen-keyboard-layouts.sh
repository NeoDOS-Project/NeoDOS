#!/usr/bin/env bash
# Regenerate data/keyboard/<tag>.kbd from data/keyboard/<tag>.klc with kbdcompile.
#
# The layout files are committed artifacts, so this is run by hand when a .klc
# changes (see issue #702). `check-keyboard-layouts.sh` guards against drift.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

(cd tools/kbdcompile && cargo build --quiet)
bin=tools/kbdcompile/target/debug/kbdcompile

shopt -s nullglob
found=0
for klc in data/keyboard/*.klc; do
    found=1
    tag="$(basename "$klc" .klc)"
    "$bin" "$klc" "data/keyboard/$tag.kbd" "$tag" "$tag"
done

if [ "$found" -eq 0 ]; then
    echo "gen-keyboard-layouts: no .klc sources found under data/keyboard/" >&2
    exit 1
fi
echo "gen-keyboard-layouts: done"
