#!/usr/bin/env python3
# Build script for virtio-blk.nem NEM v3 standalone driver.

import subprocess, sys, os

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.abspath(os.path.join(SCRIPT_DIR, '..', '..'))
TOOLS_DIR = os.path.join(PROJECT_ROOT, 'tools')
NEM_PACK = os.path.join(TOOLS_DIR, 'nem-pack.py')

DRIVER_NAME = 'virtio-blk'
TARGET = 'x86_64-unknown-none'

def main():
    output_dir = sys.argv[1] if len(sys.argv) > 1 else SCRIPT_DIR
    os.makedirs(output_dir, exist_ok=True)

    src_file = os.path.join(SCRIPT_DIR, 'src', 'lib.rs')
    obj_file = os.path.join(output_dir, f'{DRIVER_NAME}.o')
    nem_file = os.path.join(output_dir, f'{DRIVER_NAME}.nem')

    rustc = os.environ.get('RUSTC', 'rustc')

    subprocess.run([rustc,
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
    ], check=True)

    subprocess.run([sys.executable, NEM_PACK,
        obj_file, nem_file,
        '--name', DRIVER_NAME,
        '--type', '2',
        '--category', '1',
        '--abi-min', '1',
        '--abi-target', '1',
        '--abi-max', '2',
    ], check=True)

    os.unlink(obj_file)

if __name__ == '__main__':
    main()
