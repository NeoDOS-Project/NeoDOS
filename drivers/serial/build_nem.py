#!/usr/bin/env python3
"""
Build script for serial.nem NEM v3 standalone driver.

Usage:
  python3 build_nem.py [output_dir]
"""

import subprocess
import sys
import os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, '..', '..'))
TOOLS_DIR = os.path.join(PROJECT_ROOT, 'tools')
NEM_PACK = os.path.join(TOOLS_DIR, 'nem-pack.py')

DRIVER_NAME = 'serial'
TARGET = 'x86_64-unknown-none'


def find_rustc():
    for path in os.environ.get('PATH', '').split(os.pathsep):
        rustc = os.path.join(path, 'rustc')
        if os.path.exists(rustc) or os.path.exists(rustc + '.exe'):
            return rustc
    return 'rustc'


def main():
    output_dir = sys.argv[1] if len(sys.argv) > 1 else SCRIPT_DIR
    os.makedirs(output_dir, exist_ok=True)

    src_file = os.path.join(SCRIPT_DIR, 'src', 'lib.rs')
    obj_file = os.path.join(output_dir, f'{DRIVER_NAME}.o')
    nem_file = os.path.join(output_dir, f'{DRIVER_NAME}.nem')
    rustc = find_rustc()

    print(f"[DRV] Compiling {src_file} -> {obj_file}...")
    result = subprocess.run([
        rustc,
        '--target', TARGET,
        '--crate-type', 'lib',
        '--emit', 'obj',
        '-C', 'panic=abort',
        '-C', 'debuginfo=0',
        '-C', 'opt-level=z',
        '-C', 'codegen-units=1',
        '-C', 'relocation-model=static',
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

    print(f"[DRV] Packaging {obj_file} -> {nem_file}...")
    result = subprocess.run([
        sys.executable, NEM_PACK,
        obj_file, nem_file,
        '--name', DRIVER_NAME,
        '--type', '2',
        '--category', '0',
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
        print(f"[DRV] [OK] {nem_file}: {nem_size} bytes")
    else:
        print(f"[!] NEM file not produced: {nem_file}")
        sys.exit(1)

    os.unlink(obj_file)


if __name__ == '__main__':
    main()
