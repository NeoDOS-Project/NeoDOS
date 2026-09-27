#!/usr/bin/env bash
#
# vbox-vdi-sync-check.sh — verify NeoDev regenerates a stale VirtualBox VDI.
#
# Contract (see docs/development/virtualbox.md):
#   mtime(disk_image.img) > mtime(disk_image.vdi)  =>  regenerate VDI
#
# NeoDOS wrapper around the canonical NeoDev harness. It defaults the project
# root to this repository, forces a stale VDI, runs
# `neodev test --backend virtualbox` and asserts the VDI is at least as new as
# the raw image afterwards.
#
# Usage:
#   scripts/vbox-vdi-sync-check.sh [--neodos-root DIR] [--neodev PATH]
#                                  [--timeout SECS] [--build] [--keep-log]
#
# Exit status: 0 on PASS, 1 on FAIL.
# Requires NeoDev >= 0.3.0.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

NEODOS_ROOT="${NEODOS_ROOT:-$DEFAULT_ROOT}"
NEODEV_BIN="${NEODEV_BIN:-}"
TIMEOUT=240
DO_BUILD=0
KEEP_LOG=0

usage() { grep '^#' "$0" | tail -n +2 | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --neodos-root) NEODOS_ROOT="$2"; shift 2 ;;
    --neodev)      NEODEV_BIN="$2";  shift 2 ;;
    --timeout)     TIMEOUT="$2";     shift 2 ;;
    --build)       DO_BUILD=1;       shift ;;
    --keep-log)    KEEP_LOG=1;       shift ;;
    -h|--help)     usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

[ -d "$NEODOS_ROOT" ] || { echo "error: not a directory: $NEODOS_ROOT" >&2; exit 2; }

if [ -z "$NEODEV_BIN" ]; then
  if command -v neodev >/dev/null 2>&1; then
    NEODEV_BIN="$(command -v neodev)"
  elif [ -x "$DEFAULT_ROOT/../neodev/target/debug/neodev" ]; then
    NEODEV_BIN="$DEFAULT_ROOT/../neodev/target/debug/neodev"
  else
    echo "error: neodev binary not found (use --neodev)" >&2
    exit 2
  fi
fi

IMG="$NEODOS_ROOT/disk_image.img"
VDI="$NEODOS_ROOT/disk_image.vdi"
LOG="$(mktemp -t neodev-vdi-sync.XXXXXX.log)"

mt_epoch() { stat -c %Y "$1" 2>/dev/null || echo 0; }
mt_iso()   { stat -c %y "$1" 2>/dev/null || echo "missing"; }

echo "==================================================================="
echo " VirtualBox stale-VDI sync check"
echo "==================================================================="
echo "  neodev:      $NEODEV_BIN"
echo "  NEODOS_ROOT: $NEODOS_ROOT"
echo "  log:         $LOG"
echo

if [ "$DO_BUILD" -eq 1 ]; then
  echo ">> neodev build --quick --image"
  ( cd "$NEODOS_ROOT" && "$NEODEV_BIN" build --quick --image )
  echo
else
  [ -f "$IMG" ] || { echo "FAIL: $IMG missing (run with --build)" >&2; exit 1; }
  echo ">> forcing stale VDI: touching $IMG"
  touch "$IMG"
fi

[ -f "$IMG" ] || { echo "FAIL: $IMG still missing after build" >&2; exit 1; }

IMG_BEFORE="$(mt_epoch "$IMG")"
VDI_BEFORE="$(mt_epoch "$VDI")"
echo "IMG mtime before: $IMG_BEFORE  ($(mt_iso "$IMG"))"
echo "VDI mtime before: $VDI_BEFORE  ($(mt_iso "$VDI"))"

if [ "$VDI_BEFORE" -ne 0 ] && [ "$IMG_BEFORE" -le "$VDI_BEFORE" ]; then
  echo "WARN: IMG is not newer than VDI; the reconversion path may not trigger." >&2
fi

echo
echo ">> neodev test --backend virtualbox --timeout $TIMEOUT"
set +e
( cd "$NEODOS_ROOT" && "$NEODEV_BIN" test --backend virtualbox --timeout "$TIMEOUT" ) 2>&1 | tee "$LOG"
TEST_RC=${PIPESTATUS[0]}
set -e

IMG_AFTER="$(mt_epoch "$IMG")"
VDI_AFTER="$(mt_epoch "$VDI")"
echo
echo "IMG mtime after:  $IMG_AFTER  ($(mt_iso "$IMG"))"
echo "VDI mtime after:  $VDI_AFTER  ($(mt_iso "$VDI"))"

CONVERTED=0
if [ "$VDI_BEFORE" -eq 0 ] || [ "$VDI_AFTER" -gt "$VDI_BEFORE" ]; then
  CONVERTED=1
fi

echo
echo "-------------------------------------------------------------------"
echo " conversion decision : $(grep -aE 're-converting|not found, creating|Converting' "$LOG" | head -1 || echo 'n/a')"
echo " VDI regenerated     : $([ "$CONVERTED" -eq 1 ] && echo yes || echo no)"
echo " VDI >= IMG          : $([ "$VDI_AFTER" -ge "$IMG_AFTER" ] && echo yes || echo no)"
echo " test exit code      : $TEST_RC"
echo "-------------------------------------------------------------------"

RESULT=0
[ "$CONVERTED" -eq 1 ]  || { echo "FAIL: VDI was not regenerated"; RESULT=1; }
[ "$VDI_AFTER" -ge "$IMG_AFTER" ] || { echo "FAIL: VDI is still older than IMG"; RESULT=1; }

if [ "$RESULT" -eq 0 ]; then
  echo "PASS: stale VDI was detected and regenerated automatically."
else
  echo "FAIL: stale-VDI contract not satisfied."
fi

if [ "$KEEP_LOG" -eq 1 ]; then
  echo "log kept at $LOG"
else
  rm -f "$LOG"
fi
exit "$RESULT"
