#!/bin/bash
# NeoDOS QEMU Debug Session (v0.11.0)
#
# Modes:
#   (default)  SLiRP user-mode networking — works without any setup
#   --bridge   Bridge mode via qemu-bridge-helper — run setup-network.sh first
#   --tap      Raw TAP device — needs manual tap0 setup
#
# Bridge setup (one-time):
#   sudo bash scripts/setup-network.sh

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OVMF_CODE="/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TMPL="/usr/share/OVMF/OVMF_VARS.fd"
DISK_IMAGE="$PROJECT_ROOT/disk_image.img"
BRIDGE_NAME="${NEODOS_BRIDGE:-neodos0}"

if [ ! -f "$DISK_IMAGE" ]; then
    echo "[!] Disk image not found: $DISK_IMAGE"
    echo "[!] Run bash scripts/build.sh first."
    exit 1
fi

echo "[*] NeoDOS QEMU Debug Session"
echo ""

USE_STORAGE="ahci"
USE_TAP=false
USE_BRIDGE=false
BDM_ONLY=false

for arg in "$@"; do
    case "$arg" in
        --ata)       USE_STORAGE="ata" ;;
        --ahci)      USE_STORAGE="ahci" ;;
        --nvme)      USE_STORAGE="nvme" ;;
        --virtio)    USE_STORAGE="virtio" ;;
        --tap)       USE_TAP=true ;;
        --bridge)    USE_BRIDGE=true ;;
        --bdm)       BDM_ONLY=true ;;
    esac
done

# ── OVMF VARS (permanent for BDM, ephemeral otherwise) ──
if [ "$BDM_ONLY" = true ]; then
    OVMF_VARS="$PROJECT_ROOT/OVMF_VARS.fd"
    if [ ! -f "$OVMF_VARS" ]; then
        cp "$OVMF_VARS_TMPL" "$OVMF_VARS"
        echo "[+] Created persistent OVMF_VARS: $OVMF_VARS"
    fi
    echo "[+] BDM mode: preserving OVMF_VARS"
else
    OVMF_VARS="/tmp/OVMF_VARS_$RANDOM.fd"
    cp "$OVMF_VARS_TMPL" "$OVMF_VARS"
    echo "[+] Created temporary OVMF_VARS: $OVMF_VARS"
fi

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

# ── Accelerator ──
ACCEL="${QEMU_ACCEL:-tcg}"
if [ "$ACCEL" = "kvm" ] && [ ! -c /dev/kvm ]; then
    echo "[!] KVM requested but /dev/kvm not available; falling back to TCG"
    ACCEL="tcg"
fi
echo "[+] QEMU accelerator: $ACCEL"

# ── Storage ──
case "$USE_STORAGE" in
    ahci)
        DRIVE_OPTS="-device ahci,id=ahci -drive if=none,format=raw,file=$DISK_IMAGE,id=mydisk -device ide-hd,drive=mydisk,bus=ahci.0"
        echo "[+] Storage: AHCI Mode"
        ;;
    nvme)
        DRIVE_OPTS="-drive if=none,format=raw,file=$DISK_IMAGE,id=nvm -device nvme,serial=deadbeef,drive=nvm"
        echo "[+] Storage: NVMe Mode"
        ;;
    virtio)
        DRIVE_OPTS="-drive if=none,format=raw,file=$DISK_IMAGE,id=virtioblk -device virtio-blk-pci,disable-legacy=on,drive=virtioblk"
        echo "[+] Storage: VirtIO Block Mode"
        ;;
    *)
        DRIVE_OPTS="-drive format=raw,file=$DISK_IMAGE,index=0,media=disk"
        echo "[+] Storage: ATA/IDE Mode"
        ;;
esac

# ── Network ──
if [ "$USE_BRIDGE" = true ] && [ -x /usr/libexec/qemu-bridge-helper ]; then
    if ip link show "$BRIDGE_NAME" &>/dev/null; then
        NET_OPTS="-netdev bridge,id=net0,br=$BRIDGE_NAME"
        echo "[+] Network: bridge ($BRIDGE_NAME) via qemu-bridge-helper"
        echo "[!] Guest will use DHCP (QEMU built-in). IP range: 10.0.2.x"
    else
        echo "[!] Bridge $BRIDGE_NAME does not exist."
        echo "[!] Run: sudo bash scripts/setup-network.sh"
        echo "[!] Falling back to SLiRP..."
        NET_OPTS="-netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1"
    fi
elif [ "$USE_TAP" = true ] && [ -c /dev/net/tun ]; then
    echo "[+] Network: TAP (tap0, 10.0.1.0/24)"
    echo "[!] If setup needed:"
    echo "    sudo ip tuntap del tap0 2>/dev/null"
    echo "    sudo ip tuntap add tap0 mode tap user $(whoami)"
    echo "    sudo ip addr add 10.0.1.1/24 dev tap0"
    echo "    sudo ip link set tap0 up"
    NET_OPTS="-netdev tap,id=net0,ifname=tap0,script=no"
else
    echo "[+] Network: user-mode (SLiRP) — DHCP virtual 10.0.1.x"
    echo "[!] Use --bridge for real networking (run setup-network.sh first)"
    NET_OPTS="-netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1"
fi

# Attach e1000 NIC to the selected network backend
NET_OPTS="$NET_OPTS -device e1000,netdev=net0"

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

# Cleanup
if [ "$BDM_ONLY" = false ]; then
    rm -f "$OVMF_VARS" 2>/dev/null || true
    echo "[*] OVMF_VARS cleaned up"
else
    echo "[*] OVMF_VARS preserved at: $OVMF_VARS"
fi
echo ""
