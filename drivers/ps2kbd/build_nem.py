#!/usr/bin/env python3
"""
Build script for ps2kbd.nem standalone driver.

Compiles the Rust source to an ELF object file (.o)
and packages it into a NEM v3 binary (.nem).

Usage:
  python3 build_nem.py [output_dir]

Output:
  <output_dir>/ps2kbd.nem
"""

import subprocess
import sys
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, '..', '..'))
KERNEL_DIR = os.path.join(PROJECT_ROOT, 'neodos-kernel')
TOOLS_DIR = os.path.join(PROJECT_ROOT, 'tools')
NEM_PACK = os.path.join(TOOLS_DIR, 'nem-pack.py')

DRIVER_NAME = 'ps2kbd'
TARGET = 'x86_64-unknown-none'


def find_rustc():
    """Find rustc in PATH."""
    for path in os.environ.get('PATH', '').split(os.pathsep):
        rustc = os.path.join(path, 'rustc')
        if os.path.exists(rustc) or os.path.exists(rustc + '.exe'):
            return rustc
    return 'rustc'


def find_rustup():
    """Find rustup in PATH."""
    for path in os.environ.get('PATH', '').split(os.pathsep):
        rustup = os.path.join(path, 'rustup')
        if os.path.exists(rustup) or os.path.exists(rustup + '.exe'):
            return rustup
    return 'rustup'


def main():
    output_dir = sys.argv[1] if len(sys.argv) > 1 else SCRIPT_DIR
    os.makedirs(output_dir, exist_ok=True)

    src_file = os.path.join(SCRIPT_DIR, 'src', 'lib.rs')
    obj_file = os.path.join(output_dir, f'{DRIVER_NAME}.o')
    nem_file = os.path.join(output_dir, f'{DRIVER_NAME}.nem')

    rustc = find_rustc()
    rustup = find_rustup()

    # Ensure target is installed
    print(f"[DRV] Checking target {TARGET}...")
    subprocess.run([rustup, 'target', 'add', TARGET], check=False,
                   capture_output=True)

    # Compile to relocatable object file
    print(f"[DRV] Compiling {src_file} -> {obj_file}...")
    result = subprocess.run([
        rustc,
        '--target', TARGET,
        '--crate-type', 'lib',
        '--emit', 'obj',
        '-C', 'panic=abort',
        '-C', 'debuginfo=0',
        '-C', 'opt-level=3',
        '-C', 'codegen-units=1',
        '-C', 'relocation-model=pic',
        '--edition', '2021',
        '-o', obj_file,
        src_file,
    ], capture_output=True, text=True)

    if result.returncode != 0:
        print(f"[!] Compilation failed:")
        print(result.stderr)
        sys.exit(1)

    if not os.path.exists(obj_file):
        print(f"[!] Object file not produced: {obj_file}")
        sys.exit(1)

    obj_size = os.path.getsize(obj_file)
    print(f"[DRV] Object file: {obj_file} ({obj_size} bytes)")

    # Package into NEM v3
    print(f"[DRV] Packaging {obj_file} -> {nem_file}...")
    result = subprocess.run([
        sys.executable, NEM_PACK,
        obj_file, nem_file,
        '--name', DRIVER_NAME,
        '--type', '2',     # Lifecycle
        '--category', '0',  # Boot
        '--abi-min', '1',
        '--abi-target', '1',
        '--abi-max', '2',
    ], capture_output=True, text=True)

    if result.returncode != 0:
        print(f"[!] nem-pack failed:")
        print(result.stderr)
        sys.exit(1)

    print(result.stdout)

    if os.path.exists(nem_file):
        nem_size = os.path.getsize(nem_file)
        print(f"[DRV] ✓ {nem_file}: {nem_size} bytes")
    else:
        print(f"[!] NEM file not produced: {nem_file}")
        sys.exit(1)

    # Clean up .o file
    os.unlink(obj_file)

    print(f"[DRV] Build complete: {nem_file}")


if __name__ == '__main__':
    main()
