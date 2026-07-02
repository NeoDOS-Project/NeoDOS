#!/bin/bash
# NeoDOS QEMU Debug Session (v0.10.3)

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OVMF_CODE="/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TMPL="/usr/share/OVMF/OVMF_VARS.fd"
DISK_IMAGE="$PROJECT_ROOT/disk_image.img"

if [ ! -f "$DISK_IMAGE" ]; then
    echo "[!] Disk image not found: $DISK_IMAGE"
    echo "[!] Run bash scripts/build.sh first."
    exit 1
fi

echo "[*] NeoDOS QEMU Debug Session"
echo ""

USE_STORAGE="ahci"

for arg in "$@"; do
    case "$arg" in
        --ata) USE_STORAGE="ata" ;;
        --ahci) USE_STORAGE="ahci" ;;
        --nvme) USE_STORAGE="nvme" ;;
        --virtio) USE_STORAGE="virtio" ;;
    esac
done

# Create a temporary copy of OVMF_VARS to avoid permission issues and keep state clean
OVMF_VARS="/tmp/OVMF_VARS_$RANDOM.fd"
cp "$OVMF_VARS_TMPL" "$OVMF_VARS"
echo "[+] Created temporary OVMF_VARS: $OVMF_VARS"

echo ""
echo "=========================================="
echo "Launching QEMU (GUI)..."
echo "=========================================="
echo "QEMU Monitor:  localhost:4444 (use 'telnet localhost 4444')"
echo "GDB:           localhost:1234 (use 'gdb -x .gdbinit')"
echo ""
echo "Close the QEMU window to exit"
echo "=========================================="
echo ""

ACCEL="${QEMU_ACCEL:-tcg}"
if [ "$ACCEL" = "kvm" ] && [ ! -c /dev/kvm ]; then
    echo "[!] KVM requested but /dev/kvm not available; falling back to TCG"
    ACCEL="tcg"
fi
echo "[+] QEMU accelerator: $ACCEL"

if [ "$USE_STORAGE" = "ahci" ]; then
  DRIVE_OPTS="-device ahci,id=ahci -drive if=none,format=raw,file=$DISK_IMAGE,id=mydisk -device ide-hd,drive=mydisk,bus=ahci.0"
  echo "[+] Storage: AHCI Mode"
elif [ "$USE_STORAGE" = "nvme" ]; then
  DRIVE_OPTS="-drive if=none,format=raw,file=$DISK_IMAGE,id=nvm -device nvme,serial=deadbeef,drive=nvm"
  echo "[+] Storage: NVMe Mode"
elif [ "$USE_STORAGE" = "virtio" ]; then
  DRIVE_OPTS="-drive if=none,format=raw,file=$DISK_IMAGE,id=virtioblk -device virtio-blk-pci,disable-legacy=on,drive=virtioblk"
  echo "[+] Storage: VirtIO Block Mode"
else
  DRIVE_OPTS="-drive format=raw,file=$DISK_IMAGE,index=0,media=disk"
  echo "[+] Storage: ATA/IDE Mode"
fi
echo ""

# ── Network: TAP (host reachable) o user-mode (SLiRP) ──
NET_OPTS=""
if [ -c /dev/net/tun ] && ip link show tap0 >/dev/null 2>&1; then
    echo "[+] Network: TAP (tap0, 10.0.1.0/24)"
    NET_OPTS="-netdev tap,id=net0,ifname=tap0,script=no -device e1000,netdev=net0"
else
    echo "[+] Network: user-mode (SLiRP), host→guest ICMP NO funciona"
    echo "[!] Para TAP: sudo ip tuntap add tap0 mode tap user $(whoami) && sudo ip addr add 10.0.1.1/24 dev tap0 && sudo ip link set tap0 up"
    NET_OPTS="-netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 -device e1000,netdev=net0"
fi

qemu-system-x86_64 \
  $NET_OPTS \
  -machine q35,accel=$ACCEL \
  -no-reboot \
  -monitor telnet:127.0.0.1:4444,server,nowait \
  -gdb tcp::1234 \
  -drive if=pflash,format=raw,readonly=on,file=$OVMF_CODE \
  -drive if=pflash,format=raw,file=$OVMF_VARS \
  $DRIVE_OPTS \
  -m 512M \
  -serial stdio | tee "$PROJECT_ROOT/qemu_output.log"

EXIT_CODE=$?

echo ""
echo "[*] QEMU stopped (exit code: $EXIT_CODE)"
echo "[*] Output saved to: $PROJECT_ROOT/qemu_output.log"
echo "[*] OVMF_VARS: $OVMF_VARS (kept for inspection)"
echo ""

# Cleanup
rm -f "$OVMF_VARS" 2>/dev/null || true
