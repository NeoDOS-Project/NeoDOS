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
echo ""

qemu-system-x86_64 \
  -machine q35,accel=$ACCEL \
  -monitor telnet:127.0.0.1:4444,server,nowait \
  -gdb tcp::1234 \
  -drive if=pflash,format=raw,readonly=on,file=$OVMF_CODE \
  -drive if=pflash,format=raw,file=$OVMF_VARS \
  -drive format=raw,file="$DISK_IMAGE",index=0,media=disk \
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
