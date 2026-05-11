#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BUILD_NEODOS_IMAGE=false
BUILD_USERBIN=false

for arg in "$@"; do
    case "$arg" in
        --neodos-image|-n) BUILD_NEODOS_IMAGE=true; BUILD_USERBIN=true ;;
        --userbin|-u)      BUILD_USERBIN=true ;;
    esac
done

echo "[*] NeoDOS Build"
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
# 3. Create ESP partition image (FAT32)
# ============================================
echo "[+] Creating ESP partition image..."

ESP_IMAGE="$PROJECT_ROOT/tmp_esp.img"
ESP_SIZE_MB=100

dd if=/dev/zero of="$ESP_IMAGE" bs=1M count=$ESP_SIZE_MB 2>/dev/null
echo "[✓] Created empty ESP image (${ESP_SIZE_MB}MB)"

if command -v mkfs.fat >/dev/null 2>&1; then
    mkfs.fat -F 32 "$ESP_IMAGE" >/dev/null 2>&1
    echo "[✓] Formatted as FAT32"

    if command -v mmd >/dev/null 2>&1; then
        mmd -i "$ESP_IMAGE" /EFI 2>/dev/null || true
        mmd -i "$ESP_IMAGE" /EFI/BOOT 2>/dev/null || true
        mmd -i "$ESP_IMAGE" /EFI/NeoDOS 2>/dev/null || true
        mcopy -i "$ESP_IMAGE" "$PROJECT_ROOT/bootloader.efi" ::/EFI/BOOT/BOOTX64.EFI
        mcopy -i "$ESP_IMAGE" "$PROJECT_ROOT/bootloader.efi" ::/EFI/NeoDOS/bootloader.efi
        mcopy -i "$ESP_IMAGE" "$PROJECT_ROOT/kernel.elf" ::/EFI/NeoDOS/kernel.elf
        echo "[✓] Copied files to ESP"
    else
        echo "[!] mtools not found; files not copied to image"
        echo "    Install: sudo apt install mtools"
    fi
else
    echo "[!] mkfs.fat not found; ESP image not formatted"
    echo "    Install: sudo apt install dosfstools"
fi

# ============================================
# 3b. Compile user-mode binaries
# ============================================
if [ "$BUILD_USERBIN" = true ]; then
    echo ""
    echo "[+] Compiling user-mode binaries..."
    USERBIN_DIR="$PROJECT_ROOT/userbin"

    # Prefer Python generators (no external dep) — always run them first
    for gen in "$USERBIN_DIR"/generate_*.py; do
        [ -f "$gen" ] || continue
        python3 "$gen" && echo "[\u2713] $(basename "$gen") run OK"
    done

    # Also compile any .asm files if nasm is available
    if command -v nasm >/dev/null 2>&1; then
        for asm_file in "$USERBIN_DIR"/*.asm; do
            [ -f "$asm_file" ] || continue
            bin_file="${asm_file%.asm}.bin"
            nasm -f bin -o "$bin_file" "$asm_file"
            echo "[\u2713] nasm: $(basename "$asm_file") -> $(basename "$bin_file") ($(wc -c < "$bin_file") bytes)"
        done
    fi
fi

# ============================================
# 4. Generate NeoDOS FS image (optional)
# ============================================
NEODOS_IMAGE="$SCRIPT_DIR/neodos_image.img"
if [ "$BUILD_NEODOS_IMAGE" = true ]; then
    echo ""
    echo "[+] Generating NeoDOS FS image..."
    if command -v python3 >/dev/null 2>&1; then
        cd "$SCRIPT_DIR"
        python3 create_neodos_image.py
        cd "$PROJECT_ROOT"
        echo "[✓] NeoDOS FS image: $NEODOS_IMAGE"
    else
        echo "[!] python3 not found; skipping NeoDOS FS image"
    fi
fi

# ============================================
# 5. Create unified GPT disk image
# ============================================
echo ""
echo "[+] Creating unified GPT disk image..."

DISK_IMAGE="$PROJECT_ROOT/disk_image.img"

if [ -f "$NEODOS_IMAGE" ] && command -v python3 >/dev/null 2>&1; then
    python3 "$SCRIPT_DIR/create_gpt_image.py" \
        --esp "$ESP_IMAGE" \
        --neodos "$NEODOS_IMAGE" \
        --output "$DISK_IMAGE"
    echo "[✓] Unified GPT disk image: $DISK_IMAGE"
else
    echo "[!] NeoDOS image missing; creating FAT32-only image"
    mv "$ESP_IMAGE" "$DISK_IMAGE"
fi

# Cleanup temp files
rm -f "$ESP_IMAGE" 2>/dev/null || true

echo ""
echo "[✓] Build Complete!"
echo ""
echo "    Bootloader: $PROJECT_ROOT/bootloader.efi"
echo "    Kernel:     $PROJECT_ROOT/kernel.bin"
echo "    Disk image: $DISK_IMAGE (GPT: ESP + NeoDOS FS)"
echo ""
echo "Next: bash scripts/qemu-debug.sh"
