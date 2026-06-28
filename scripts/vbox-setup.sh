#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VM_NAME="NeoDOS"
DISK_IMAGE="$PROJECT_ROOT/disk_image.img"
DISK_VDI="$PROJECT_ROOT/disk_image.vdi"

echo "[*] NeoDOS VirtualBox VM Setup"
echo ""

# Check required files
if [ ! -f "$DISK_IMAGE" ]; then
    echo "[!] Missing: $DISK_IMAGE"
    echo "    Run: bash scripts/build.sh"
    exit 1
fi

# Refresh mode: re-build disk image + re-convert VDI without recreating VM
if [ "${1:-}" = "--refresh" ] || [ "${1:-}" = "--build" ]; then
    if [ "${1:-}" = "--build" ]; then
        echo "[*] Build mode: rebuilding kernel + disk image..."
        (cd "$PROJECT_ROOT" && bash scripts/build.sh --neodos-image)
        echo "[*] Build complete."
    fi
    echo "[*] Refresh mode: re-converting VDI from raw image..."
    rm -f "$DISK_VDI"
    VBoxManage convertfromraw "$DISK_IMAGE" "$DISK_VDI"
    echo "[+] VDI refreshed. Start VM: VBoxManage startvm \"$VM_NAME\""
    exit 0
fi

# Delete existing VM if present
if VBoxManage showvminfo "$VM_NAME" &>/dev/null; then
    echo "[!] VM '$VM_NAME' already exists."
    read -rp "    Delete and recreate? (y/N): " confirm
    if [ "$confirm" = "y" ] || [ "$confirm" = "Y" ]; then
        VBoxManage unregistervm "$VM_NAME" --delete 2>/dev/null || true
        echo "[*] Old VM deleted."
    else
        echo "[*] Aborted."
        exit 1
    fi
fi

echo "[1/8] Converting disk_image.img to VDI..."
rm -f "$DISK_VDI"
VBoxManage convertfromraw "$DISK_IMAGE" "$DISK_VDI"

echo "[2/8] Creating VM..."
VBoxManage createvm --name "$VM_NAME" --ostype Linux_64 --register

echo "[3/8] Setting memory (512 MB)..."
VBoxManage modifyvm "$VM_NAME" --memory 512

echo "[4/8] Enabling UEFI..."
VBoxManage modifyvm "$VM_NAME" --firmware efi

echo "[5/8] Setting chipset to ich9 (ACPI poweroff support)..."
VBoxManage modifyvm "$VM_NAME" --chipset ich9

echo "[6/8] Configuring serial port (COM1 -> file)..."
VBoxManage modifyvm "$VM_NAME" --uart1 0x3F8 4
VBoxManage modifyvm "$VM_NAME" --uartmode1 file "$PROJECT_ROOT/vbox_serial.log"

echo "[7/8] Adding AHCI controller..."
VBoxManage storagectl "$VM_NAME" --name "AHCI" --add sata --controller IntelAhci

echo "[8/8] Attaching unified disk image..."
VBoxManage storageattach "$VM_NAME" --storagectl "AHCI" --port 0 --device 0 --type hdd \
    --medium "$DISK_VDI"

# ── Bridged Networking ──
echo ""
echo "[+] Configuring bridged networking..."

# Detect the first active physical interface
BRIDGE_IFACE=""
for iface in $(ip -o link show | awk -F': ' '{print $2}' | grep -v lo | grep -v tap | grep -v docker | grep -v vbox); do
    state=$(cat "/sys/class/net/$iface/operstate" 2>/dev/null || echo "unknown")
    if [ "$state" = "up" ]; then
        BRIDGE_IFACE="$iface"
        break
    fi
done

if [ -z "$BRIDGE_IFACE" ]; then
    # Fallback: pick first non-loopback interface
    BRIDGE_IFACE=$(ip -o link show | awk -F': ' '{print $2}' | grep -v lo | grep -v tap | grep -v docker | head -1)
fi

echo "    Host interface: $BRIDGE_IFACE"

VBoxManage modifyvm "$VM_NAME" \
    --nic1 bridged \
    --bridgeadapter1 "$BRIDGE_IFACE" \
    --nictype1 82540EM \
    --macaddress1 525400123456 \
    --cableconnected1 on

echo ""
echo "[+] VM '$VM_NAME' created successfully!"
echo ""
echo "=========================================="
echo "  Start:    VBoxManage startvm \"$VM_NAME\""
echo "  Serial:   tail -f $PROJECT_ROOT/vbox_serial.log"
echo "  VM dir:   ~/VirtualBox VMs/$VM_NAME/"
echo "  Network:  bridged ($BRIDGE_IFACE)"
echo "=========================================="
