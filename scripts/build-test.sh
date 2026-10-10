#!/usr/bin/env bash
# Rebuild-safe build + test.
#
# `neodev build --quick` does NOT reliably rebuild the kernel (NeoDev#36), so
# `neodev build --image` / `neodev test` can silently use a STALE kernel.elf.
# This wrapper always compiles the kernel with cargo first, then regenerates the
# image and runs the suite. Prefer it over calling `neodev test` directly.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "[build-test] cargo build (kernel, x86_64-unknown-none)..."
cargo build --manifest-path neodos-kernel/Cargo.toml --target x86_64-unknown-none

echo "[build-test] neodev build --quick --image..."
neodev build --quick --image

echo "[build-test] neodev test..."
neodev test "$@"
