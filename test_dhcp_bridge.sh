#!/bin/bash
set -e

PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
echo "[*] Conectando enp0s31f6 al bridge neodos0..."
sudo ip link set enp0s31f6 master neodos0
sudo ip link set neodos0 up

echo "[*] Lanzando QEMU con bridge. Espera ~30s para boot..."
cd "$PROJECT_ROOT"
OVMF_VARS="$PROJECT_ROOT/OVMF_VARS.fd"
[ -f "$OVMF_VARS" ] || cp /usr/share/OVMF/OVMF_VARS.fd "$OVMF_VARS"

qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -m 512M -smp 2 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -device ahci,id=ahci \
  -drive if=none,format=raw,file=disk_image.img,id=mydisk \
  -device ide-hd,drive=mydisk,bus=ahci.0 \
  -netdev bridge,id=net0,br=neodos0 \
  -device e1000,netdev=net0 \
  -serial stdio \
  -no-reboot
