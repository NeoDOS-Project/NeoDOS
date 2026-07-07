#!/bin/bash
PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$PROJECT_ROOT"

OVMF_VARS="$PROJECT_ROOT/OVMF_VARS.fd"
[ -f "$OVMF_VARS" ] || cp /usr/share/OVMF/OVMF_VARS.fd "$OVMF_VARS"

echo "[*] Iniciando DHCP local en tap0..."
sudo dnsmasq --interface=tap0 --bind-interfaces \
  --dhcp-range=10.0.2.2,10.0.2.254,12h \
  --dhcp-option=3,10.0.2.1 \
  --no-daemon --log-dhcp 2>&1 &
sleep 2
echo "[*] DHCP corriendo en tap0"

echo "[*] Lanzando QEMU con TAP (sin sudo)..."
qemu-system-x86_64 \
  -machine q35,accel=tcg \
  -m 512M -smp 2 \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
  -drive if=pflash,format=raw,file="$OVMF_VARS" \
  -device ahci,id=ahci \
  -drive if=none,format=raw,file=disk_image.img,id=mydisk \
  -device ide-hd,drive=mydisk,bus=ahci.0 \
  -netdev tap,id=net0,ifname=tap0,script=no \
  -device e1000,netdev=net0 \
  -serial stdio \
  -no-reboot

kill %1 2>/dev/null
