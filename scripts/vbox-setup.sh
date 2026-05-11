#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VM_NAME="NeoDOS"
DISK_IMAGE="$PROJECT_ROOT/disk_image.img"
NEODOS_IMAGE="$PROJECT_ROOT/scripts/neodos_image.img"
DISK_VDI="$PROJECT_ROOT/disk_image.vdi"
NEODOS_VDI="$PROJECT_ROOT/scripts/neodos_image.vdi"

echo "[*] NeoDOS VirtualBox VM Setup"
echo ""

# Check required files
for f in "$DISK_IMAGE" "$NEODOS_IMAGE"; do
    if [ ! -f "$f" ]; then
        echo "[!] Missing: $f"
        echo "    Run: bash scripts/build.sh"
        exit 1
    fi
done

# Refresh mode: re-convert VDIs without recreating the VM
if [ "${1:-}" = "--refresh" ]; then
    echo "[*] Refresh mode: re-converting VDIs from raw images..."
    rm -f "$DISK_VDI" "$NEODOS_VDI"
    VBoxManage convertfromraw "$DISK_IMAGE" "$DISK_VDI"
    VBoxManage convertfromraw "$NEODOS_IMAGE" "$NEODOS_VDI"
    echo "[+] VDIs refreshed. Start VM: VBoxManage startvm \"$VM_NAME\""
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

echo "[1/9] Converting disk_image.img to VDI..."
VBoxManage convertfromraw "$DISK_IMAGE" "$DISK_VDI"

echo "[2/9] Converting neodos_image.img to VDI..."
VBoxManage convertfromraw "$NEODOS_IMAGE" "$NEODOS_VDI"

echo "[3/9] Creating VM..."
VBoxManage createvm --name "$VM_NAME" --ostype Linux_64 --register

echo "[4/9] Setting memory (512 MB)..."
VBoxManage modifyvm "$VM_NAME" --memory 512

echo "[5/9] Enabling UEFI..."
VBoxManage modifyvm "$VM_NAME" --firmware efi

echo "[6/9] Setting chipset to ich9 (ACPI poweroff support)..."
VBoxManage modifyvm "$VM_NAME" --chipset ich9

echo "[7/9] Configuring serial port (COM1 -> file)..."
VBoxManage modifyvm "$VM_NAME" --uart1 0x3F8 4
VBoxManage modifyvm "$VM_NAME" --uartmode1 file "$PROJECT_ROOT/vbox_serial.log"

echo "[8/9] Adding IDE controller..."
VBoxManage storagectl "$VM_NAME" --name "IDE" --add ide --controller PIIX4

echo "[9/9] Attaching disk images (master=boot, slave=neodos)..."
VBoxManage storageattach "$VM_NAME" --storagectl "IDE" --port 0 --device 0 --type hdd \
    --medium "$DISK_VDI"
VBoxManage storageattach "$VM_NAME" --storagectl "IDE" --port 0 --device 1 --type hdd \
    --medium "$NEODOS_VDI"

echo ""
echo "[+] VM '$VM_NAME' created successfully!"
echo ""
echo "=========================================="
echo "  Start:    VBoxManage startvm \"$VM_NAME\""
echo "  Serial:   tail -f $PROJECT_ROOT/vbox_serial.log"
echo "  VM dir:   ~/VirtualBox VMs/$VM_NAME/"
echo "=========================================="
