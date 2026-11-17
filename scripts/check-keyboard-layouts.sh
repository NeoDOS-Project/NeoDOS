#!/usr/bin/env bash
# Verify data/keyboard/<tag>.kbd are non-empty and in sync with the XKB
# layouts (issue #702; conversion now via tools/xkbcompile).
#
# Fails if a .kbd is missing, maps no keys, or differs from a fresh compile.
# Run from anywhere; needs the XKB data (package `xkb-data`).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

(cd tools/xkbcompile && cargo build --quiet)
bin=tools/xkbcompile/target/debug/xkbcompile

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

layouts=(
    "us:basic:en-US"
    "es:deadtilde:es-ES"
    "es:cat:ca-ES"
    "fr:basic:fr-FR"
    "de:basic:de-DE"
    "it:basic:it-IT"
    "pt:basic:pt-PT"
    "br:abnt2:pt-BR"
    "nl:basic:nl-NL"
    "se:basic:sv-SE"
    "dk:basic:da-DK"
    "no:basic:nb-NO"
    "fi:kotoistus:fi-FI"
    "pl:basic:pl-PL"
    "cz:basic:cs-CZ"
)

fail=0
for spec in "${layouts[@]}"; do
    IFS=: read -r layout variant tag <<<"$spec"
    kbd="data/keyboard/$tag.kbd"

    if [ ! -f "$kbd" ]; then
        echo "check-keyboard-layouts: missing $kbd" >&2
        fail=1
        continue
    fi

    if ! "$bin" "$layout" "$variant" --name "$tag" --lang "$tag" --out "$tmp/$tag.kbd" 2>"$tmp/$tag.err"; then
        echo "check-keyboard-layouts: xkbcompile failed for $layout($variant)" >&2
        fail=1
        continue
    fi

    if ! grep -qE '\([1-9][0-9]* keys' "$tmp/$tag.err"; then
        echo "check-keyboard-layouts: $layout($variant) produced no keys" >&2
        fail=1
    fi

    if ! cmp -s "$tmp/$tag.kbd" "$kbd"; then
        echo "check-keyboard-layouts: $kbd is stale (run scripts/gen-keyboard-layouts.sh)" >&2
        fail=1
    fi
done

if [ "$fail" -ne 0 ]; then
    echo "check-keyboard-layouts: FAILED" >&2
    exit 1
fi
echo "check-keyboard-layouts: OK"
