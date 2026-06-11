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

# Refresh mode: re-convert VDI without recreating the VM
if [ "${1:-}" = "--refresh" ]; then
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
VBoxManage storagectl "$VM_NAME" --name "AHCI" --add ahci --controller AHCI

echo "[8/8] Attaching unified disk image..."
VBoxManage storageattach "$VM_NAME" --storagectl "AHCI" --port 0 --device 0 --type hdd \
    --medium "$DISK_VDI"

echo ""
echo "[+] VM '$VM_NAME' created successfully!"
echo ""
echo "=========================================="
echo "  Start:    VBoxManage startvm \"$VM_NAME\""
echo "  Serial:   tail -f $PROJECT_ROOT/vbox_serial.log"
echo "  VM dir:   ~/VirtualBox VMs/$VM_NAME/"
echo "=========================================="
