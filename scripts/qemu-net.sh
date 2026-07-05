#!/bin/bash
# qemu-net.sh — Lanza QEMU con red (TAP si disponible, SLiRP si no)
# Uso: bash scripts/qemu-net.sh
# Para TAP (red real + DHCP de tu router):
#   sudo bash scripts/qemu-net.sh --setup-tap
#   bash scripts/qemu-net.sh

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR/.."
DISK_IMAGE="$PROJECT_ROOT/disk_image.img"
OVMF_CODE="/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TEMPLATE="/usr/share/OVMF/OVMF_VARS.fd"
OVMF_VARS="/tmp/OVMF_VARS_net.fd"

if [ "$1" = "--setup-tap" ]; then
    echo "[*] Setting up TAP interface for NeoDOS..."
    ip tuntap add tap0 mode tap user $(whoami)
    ip addr add 10.0.1.1/24 dev tap0
    ip link set tap0 up
    echo 1 > /proc/sys/net/ipv4/ip_forward
    # NAT para que la VM salga a internet
    iptables -t nat -A POSTROUTING -o $(ip route get 8.8.8.8 | awk '{print $5}') -j MASQUERADE 2>/dev/null || true
    echo "[✓] TAP ready. Run without --setup-tap to start QEMU."
    exit 0
fi

if [ ! -f "$DISK_IMAGE" ]; then
    echo "[!] disk_image.img not found. Run 'bash scripts/build.sh --neodos-image' first."
    exit 1
fi

cp "$OVMF_VARS_TEMPLATE" "$OVMF_VARS"

# Detectar aceleración
if kvm-ok 2>/dev/null; then
    ACCEL="kvm"
else
    ACCEL="tcg"
fi

# Detectar TAP
if ip link show tap0 >/dev/null 2>&1; then
    echo "[+] Network: TAP (tap0) — DHCP de tu red real"
    NET_OPTS="-netdev tap,id=net0,ifname=tap0,script=no -device e1000,netdev=net0"
else
    echo "[+] Network: user-mode (SLiRP) — DHCP virtual 10.0.1.0/24"
    echo "[!] Para red real: sudo bash $0 --setup-tap"
    NET_OPTS="-netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 -device e1000,netdev=net0"
fi

echo "[+] Starting QEMU (ACCEL=$ACCEL)..."
qemu-system-x86_64 \
  $NET_OPTS \
  -machine q35,accel=$ACCEL \
  -no-reboot \
  -drive if=pflash,format=raw,readonly=on,file=$OVMF_CODE \
  -drive if=pflash,format=raw,file=$OVMF_VARS \
  -device ahci,id=ahci \
  -drive if=none,format=raw,file=$DISK_IMAGE,id=mydisk \
  -device ide-hd,drive=mydisk,bus=ahci.0 \
  -m 512M \
  -serial stdio

rm -f "$OVMF_VARS"
