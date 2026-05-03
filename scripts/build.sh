#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "[*] NeoDOS Fase 1 Build"
echo ""

# Check required tools
command -v cargo >/dev/null 2>&1 || { echo "[!] cargo not found"; exit 1; }
command -v rustup >/dev/null 2>&1 || { echo "[!] rustup not found"; exit 1; }

# Add required targets if not present
echo "[+] Checking Rust targets..."
rustup target add x86_64-unknown-uefi 2>/dev/null || true
rustup target add x86_64-unknown-none 2>/dev/null || true

# ============================================
# 1. Compile bootloader
# ============================================
echo "[+] Building bootloader..."
cd "$PROJECT_ROOT/neodos-bootloader"

cargo build \
    --target x86_64-unknown-uefi \
    --release \
    2>&1 | grep -E "Compiling|Finished|error" || true

if [ ! -f "target/x86_64-unknown-uefi/release/neodos_bootloader.efi" ]; then
    echo "[!] Bootloader build failed!"
    exit 1
fi

cp "target/x86_64-unknown-uefi/release/neodos_bootloader.efi" "$PROJECT_ROOT/bootloader.efi"
echo "[✓] Bootloader: $PROJECT_ROOT/bootloader.efi"
echo ""

# ============================================
# 2. Compile kernel
# ============================================
echo "[+] Building kernel..."
cd "$PROJECT_ROOT/neodos-kernel"

cargo build \
    --target x86_64-unknown-none \
    --release \
    2>&1 | grep -E "Compiling|Finished|error" || true

if [ ! -f "target/x86_64-unknown-none/release/neodos_kernel" ]; then
    echo "[!] Kernel build failed!"
    exit 1
fi

# Use the ELF directly
KERNEL_ELF="target/x86_64-unknown-none/release/neodos_kernel"
KERNEL_BIN="$PROJECT_ROOT/kernel.elf"

cp "$KERNEL_ELF" "$KERNEL_BIN"

echo "[✓] Kernel ELF: $PROJECT_ROOT/kernel.elf"
echo ""

# ============================================
# 3. Create ESP disk image (FAT32)
# ============================================
echo "[+] Creating ESP disk image..."

DISK_IMAGE="$PROJECT_ROOT/disk_image.img"
DISK_SIZE_MB=100

# Create empty disk
dd if=/dev/zero of="$DISK_IMAGE" bs=1M count=$DISK_SIZE_MB 2>/dev/null
echo "[✓] Created empty disk (${DISK_SIZE_MB}MB)"

# Format as FAT32
if command -v mkfs.fat >/dev/null 2>&1; then
    mkfs.fat -F 32 "$DISK_IMAGE" >/dev/null 2>&1
    echo "[✓] Formatted as FAT32"
    
    # Mount and copy files using mtools or direct write
    if command -v mmd >/dev/null 2>&1; then
        mmd -i "$DISK_IMAGE" /EFI 2>/dev/null || true
        mmd -i "$DISK_IMAGE" /EFI/BOOT 2>/dev/null || true
        mmd -i "$DISK_IMAGE" /EFI/NeoDOS 2>/dev/null || true
        mcopy -i "$DISK_IMAGE" "$PROJECT_ROOT/bootloader.efi" ::/EFI/BOOT/BOOTX64.EFI
        mcopy -i "$DISK_IMAGE" "$PROJECT_ROOT/bootloader.efi" ::/EFI/NeoDOS/bootloader.efi
        mcopy -i "$DISK_IMAGE" "$PROJECT_ROOT/kernel.elf" ::/EFI/NeoDOS/kernel.elf
        echo "[✓] Copied files to ESP"
    else
        echo "[!] mtools not found; files not copied to image"
        echo "    Install: sudo apt install mtools"
    fi
else
    echo "[!] mkfs.fat not found; disk image created but not formatted"
    echo "    Install: sudo apt install dosfstools"
fi

echo ""
echo "[✓] Build Complete!"
echo ""
echo "    Bootloader: $PROJECT_ROOT/bootloader.efi"
echo "    Kernel:     $PROJECT_ROOT/kernel.bin"
echo "    Disk image: $DISK_IMAGE"
echo ""
echo "Next: bash scripts/qemu-debug.sh"
