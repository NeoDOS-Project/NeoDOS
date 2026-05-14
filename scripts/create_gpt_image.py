#!/usr/bin/env python3
"""
create_gpt_image.py — Crea una imagen de disco GPT unificada.

Usa sfdisk (util-linux) para crear la tabla de particiones GPT,
garantizando compatibilidad total con UEFI/OVMF.

Combina una partición FAT32 (ESP) y una partición NeoDOS FS en un solo
disco.

Uso:
    python3 create_gpt_image.py \
        --esp      esp_partition.img \
        --neodos   neodos_image.img \
        --output   disk_image.img
"""

import argparse
import math
import os
import subprocess
import sys
import tempfile

SECTOR_SIZE = 512
ESP_SIZE_MB = 100
NEODOS_SIZE_MB = 10


def build_gpt_disk(
    esp_path: str,
    neodos_path: str,
    output_path: str,
    esp_size_mb: int = ESP_SIZE_MB,
    neodos_size_mb: int = NEODOS_SIZE_MB,
):
    for p in [esp_path, neodos_path]:
        if not os.path.exists(p):
            print(f"[!] Missing: {p}")
            sys.exit(1)

    with open(esp_path, "rb") as f:
        esp_data = f.read()
    with open(neodos_path, "rb") as f:
        neodos_data = f.read()

    esp_mb = max(esp_size_mb, math.ceil(len(esp_data) / (1024 * 1024)))
    neodos_mb = max(neodos_size_mb, math.ceil(len(neodos_data) / (1024 * 1024)))
    total_mb = esp_mb + neodos_mb + 12  # GPT overhead + padding

    print(f"[*] Creating {total_mb} MiB disk image...")
    output_abs = os.path.abspath(output_path)

    # 1. Create blank image
    with open(output_abs, "wb") as f:
        f.seek(total_mb * 1024 * 1024 - 1)
        f.write(b"\x00")

    # 2. Create GPT partitions via sfdisk
    esp_start = 2048
    esp_size = esp_mb * 1024 * 1024 // SECTOR_SIZE
    neodos_start = esp_start + esp_size
    neodos_size = neodos_mb * 1024 * 1024 // SECTOR_SIZE

    sfdisk_input = (
        f"label: gpt\n"
        f"start={esp_start}, size={esp_size}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n"
        f"start={neodos_start}, size={neodos_size}, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7\n"
    )

    print(f"[*] Running sfdisk...")
    result = subprocess.run(
        ["sfdisk", output_abs],
        input=sfdisk_input,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"[!] sfdisk failed: {result.stderr}")
        print(f"    stdout: {result.stdout}")
        sys.exit(1)
    for line in result.stdout.strip().split("\n"):
        print(f"    {line.strip()}")

    # 3. Write partition data at the correct offsets
    esp_offset = esp_start * SECTOR_SIZE
    neodos_offset = neodos_start * SECTOR_SIZE

    print(f"[*] Writing ESP at LBA {esp_start} ({len(esp_data)} bytes)...")
    with open(output_abs, "r+b") as f:
        f.seek(esp_offset)
        f.write(esp_data)

    print(f"[*] Writing NeoDOS FS at LBA {neodos_start} ({len(neodos_data)} bytes)...")
    with open(output_abs, "r+b") as f:
        f.seek(neodos_offset)
        f.write(neodos_data)

    neodos_end = neodos_start + neodos_size - 1
    print(f"[✓] Unified GPT disk image created: {output_path}")
    print(f"    Partition 1 (ESP):     LBA {esp_start} - {esp_start + esp_size - 1}")
    print(f"    Partition 2 (NeoDOS):  LBA {neodos_start} - {neodos_end}")
    print(f"    Kernel will find NeoDOS FS at partition start LBA {neodos_start}")


def build_neodos_only_disk(
    neodos_path: str,
    output_path: str,
    neodos_size_mb: int = NEODOS_SIZE_MB,
):
    if not os.path.exists(neodos_path):
        print(f"[!] Missing: {neodos_path}")
        sys.exit(1)

    with open(neodos_path, "rb") as f:
        neodos_data = f.read()

    neodos_mb = max(neodos_size_mb, math.ceil(len(neodos_data) / (1024 * 1024)))
    total_mb = neodos_mb + 6  # GPT overhead + padding

    print(f"[*] Creating {total_mb} MiB disk image (NeoDOS only)...")
    output_abs = os.path.abspath(output_path)

    with open(output_abs, "wb") as f:
        f.seek(total_mb * 1024 * 1024 - 1)
        f.write(b"\x00")

    neodos_start = 2048
    neodos_size = neodos_mb * 1024 * 1024 // SECTOR_SIZE

    sfdisk_input = (
        f"label: gpt\n"
        f"start={neodos_start}, size={neodos_size}, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7\n"
    )

    print(f"[*] Running sfdisk...")
    result = subprocess.run(
        ["sfdisk", output_abs],
        input=sfdisk_input,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(f"[!] sfdisk failed: {result.stderr}")
        print(f"    stdout: {result.stdout}")
        sys.exit(1)
    for line in result.stdout.strip().split("\n"):
        print(f"    {line.strip()}")

    neodos_offset = neodos_start * SECTOR_SIZE
    print(f"[*] Writing NeoDOS FS at LBA {neodos_start} ({len(neodos_data)} bytes)...")
    with open(output_abs, "r+b") as f:
        f.seek(neodos_offset)
        f.write(neodos_data)

    neodos_end = neodos_start + neodos_size - 1
    print(f"[✓] NeoDOS-only disk image created: {output_path}")
    print(f"    Partition 1 (NeoDOS): LBA {neodos_start} - {neodos_end}")


def main():
    parser = argparse.ArgumentParser(
        description="Create unified GPT disk image with ESP + NeoDOS FS"
    )
    parser.add_argument("--esp", required=False)
    parser.add_argument("--neodos", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--esp-size", type=int, default=ESP_SIZE_MB)
    parser.add_argument("--neodos-size", type=int, default=NEODOS_SIZE_MB)
    parser.add_argument("--neodos-only", action="store_true",
                        help="Create disk with only a NeoDOS partition (no ESP)")
    args = parser.parse_args()

    if args.neodos_only:
        build_neodos_only_disk(
            neodos_path=args.neodos,
            output_path=args.output,
            neodos_size_mb=args.neodos_size,
        )
    else:
        if not args.esp:
            parser.error("--esp is required unless --neodos-only is set")
        build_gpt_disk(
            esp_path=args.esp,
            neodos_path=args.neodos,
            output_path=args.output,
            esp_size_mb=args.esp_size,
            neodos_size_mb=args.neodos_size,
        )


if __name__ == "__main__":
    main()
