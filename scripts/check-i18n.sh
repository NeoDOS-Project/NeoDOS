#!/usr/bin/env bash
# verify i18n consistency: locales, frozen ids, code<->catalog bindings, keymap.
#
# Part of the i18n hardening work (#572/#573/#578). Run from anywhere.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ -n "${NLTC:-}" ]; then
    BIN="$NLTC"
else
    # Always build so the binary matches the working tree (fresh modes).
    (cd tools/nltc && cargo build --quiet)
    BIN=tools/nltc/target/debug/nltc
fi

fail=0

# 1. every locale defines the same keys/ids as the base.
"$BIN" --check-coverage data/locale en-US || fail=1

# 2. shipping catalogs must freeze their entry ids.
shopt -s nullglob
for f in data/locale/en-US/*.toml; do
    "$BIN" --require-ids "$f" >/dev/null || fail=1
done

# 3. code constants must match the catalog (name and id).
#    neocfg keeps its ids in the logic crate; other apps put them in main.rs.
#    Warnings (unused translations) are hidden unless the check fails.
verify_bindings() {
    local out
    if ! out="$("$BIN" --verify-bindings "$1" "$2" 2>&1)"; then
        echo "$out" >&2
        fail=1
    fi
}
for f in data/locale/en-US/*.toml; do
    app="$(basename "$f" .toml)"
    if [ "$app" = "neocfg" ] && [ -f libneocfg/src/i18n_keys.rs ]; then
        verify_bindings libneocfg/src/i18n_keys.rs "$f"
    elif [ -f "userbin/$app/src/main.rs" ]; then
        verify_bindings "userbin/$app/src/main.rs" "$f"
    fi
done

# 4. the compile-time keymap (tr!) must be up to date.
if [ -f libneodos/src/i18n_keymap.rs ]; then
    tmp="$(mktemp)"
    "$BIN" --generate-keymap data/locale/en-US "$tmp" >/dev/null
    if ! diff -q "$tmp" libneodos/src/i18n_keymap.rs >/dev/null; then
        echo "check-i18n: keymap drift — regenerate with:" >&2
        echo "  nltc --generate-keymap data/locale/en-US libneodos/src/i18n_keymap.rs" >&2
        fail=1
    fi
    rm -f "$tmp"
fi

if [ "$fail" -ne 0 ]; then
    echo "check-i18n: FAILED" >&2
    exit 1
fi
echo "check-i18n: OK"
