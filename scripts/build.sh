#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BUILD_NEODOS_IMAGE=false
BUILD_USERBIN=false

NEODOS_IMAGE="$SCRIPT_DIR/neodos_image.img"
NEODOS_IMAGE2="$SCRIPT_DIR/neodos_image2.img"

for arg in "$@"; do
    case "$arg" in
        --neodos-image|-n) BUILD_NEODOS_IMAGE=true; BUILD_USERBIN=true ;;
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
    --release

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
    --release

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
# 2.5. Generate binaries needed by NeoDOS FS image (user binaries + NEM drivers)
# ============================================
USERBIN_DIR="$PROJECT_ROOT/userbin"
NEM_DIR="/tmp/nem_drivers_$$"
cd "$PROJECT_ROOT"
if [ "$BUILD_NEODOS_IMAGE" = true ] && command -v python3 >/dev/null 2>&1; then
    echo "[+] Building Rust user-mode binaries for FS image..."
    for pkg in neoshell neoinit coredir cd corehelp cpuinfo datetime ver mem vol echo kobj coretype tree corecls corecopy coredel coreren coremd corerd; do
    #for pkg in hello systest filetest alltest cputest test cpuinfo neoshell neoinit coredir corehelp; do
        echo "    Building $pkg..."
        cd "$USERBIN_DIR/$pkg"
        cargo build --release 2>&1 || { echo "[!] Failed to build $pkg"; exit 1; }
        cp "target/x86_64-unknown-none/release/$pkg" "$USERBIN_DIR/$pkg.nxe"
        echo "    -> $USERBIN_DIR/$pkg.nxe"
    done
    cd "$PROJECT_ROOT"
    # Also produce hello.elf (same binary as hello.nxe, but with .elf extension for ELF loader tests)
    #cp "$USERBIN_DIR/hello.nxe" "$USERBIN_DIR/hello.elf"
    #echo "    -> $USERBIN_DIR/hello.elf"

    echo "[+] Building libneodos NXL (shared library)..."
    cd "$PROJECT_ROOT/libneodos-nxl"
    cargo build --release 2>&1 || { echo "[!] Failed to build libneodos-nxl"; exit 1; }
    NXL_BIN="$PROJECT_ROOT/libneodos.nxl"
    cp "target/x86_64-unknown-none/release/libneodos-nxl" "$NXL_BIN"
    echo "[✓] libneodos NXL: $NXL_BIN ($(stat -c%s "$NXL_BIN") bytes)"

    echo "[+] Building libmath NXL (math library)..."
    cd "$PROJECT_ROOT/libmath-nxl"
    cargo build --release 2>&1 || { echo "[!] Failed to build libmath-nxl"; exit 1; }
    MATH_NXL_BIN="$PROJECT_ROOT/libmath.nxl"
    cp "target/x86_64-unknown-none/release/libmath-nxl" "$MATH_NXL_BIN"
    echo "[✓] libmath NXL: $MATH_NXL_BIN ($(stat -c%s "$MATH_NXL_BIN") bytes)"

    echo "[+] Building cpuinfo NXL (CPU information library)..."
    cd "$PROJECT_ROOT/libcpu-nxl"
    cargo build --release 2>&1 || { echo "[!] Failed to build cpuinfo NXL"; exit 1; }
    CPUINFO_NXL_BIN="$PROJECT_ROOT/cpuinfo.nxl"
    cp "target/x86_64-unknown-none/release/libcpu-nxl" "$CPUINFO_NXL_BIN"
    echo "[✓] cpuinfo NXL: $CPUINFO_NXL_BIN ($(stat -c%s "$CPUINFO_NXL_BIN") bytes)"

    echo "[+] Compiling NEM v3 standalone driver (ps2kbd)..."
    DRV_DIR="$PROJECT_ROOT/drivers/ps2kbd"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/BOOT"
        echo "[✓] ps2kbd.nem compiled"
    else
        echo "[!] ps2kbd build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (serial)..."
    DRV_DIR="$PROJECT_ROOT/drivers/serial"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/BOOT"
        echo "[✓] serial.nem compiled"
    else
        echo "[!] serial build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (rtc)..."
    DRV_DIR="$PROJECT_ROOT/drivers/rtc"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/BOOT"
        echo "[✓] rtc.nem compiled"
    else
        echo "[!] rtc build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (acpi)..."
    DRV_DIR="$PROJECT_ROOT/drivers/acpi"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        mkdir -p "$NEM_DIR/SYSTEM"
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/SYSTEM"
        echo "[✓] acpi.nem compiled"
    else
        echo "[!] acpi build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (pci)..."
    DRV_DIR="$PROJECT_ROOT/drivers/pci"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        mkdir -p "$NEM_DIR/SYSTEM"
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/SYSTEM"
        echo "[✓] pci.nem compiled"
    else
        echo "[!] pci build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (ata)..."
    DRV_DIR="$PROJECT_ROOT/drivers/ata"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        mkdir -p "$NEM_DIR/SYSTEM"
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/SYSTEM"
        echo "[✓] ata.nem compiled"
    else
        echo "[!] ata build script not found"
    fi

    echo "[+] Compiling NEM v3 standalone driver (ahci)..."
    DRV_DIR="$PROJECT_ROOT/drivers/ahci"
    if [ -f "$DRV_DIR/build_nem.py" ]; then
        mkdir -p "$NEM_DIR/SYSTEM"
        python3 "$DRV_DIR/build_nem.py" "$NEM_DIR/SYSTEM"
        echo "[✓] ahci.nem compiled"
    else
        echo "[!] ahci build script not found"
    fi
    export NEM_DIR
fi

# ============================================
# 3. Generate NeoDOS FS image (optional, before ESP)
# ============================================
if [ "$BUILD_NEODOS_IMAGE" = true ]; then
    echo "[+] Generating NeoDOS FS images..."
    if command -v python3 >/dev/null 2>&1; then
        cd "$SCRIPT_DIR"
        python3 create_neodos_image.py \
            --label "NEODOS" \
            --output "neodos_image.img" \
            --readme "Welcome to NeoDOS!
This is the PRIMARY disk (C:).
Built with NeoDOS FS v1.0.
"
        python3 create_neodos_image.py \
            --label "NEODOS2" \
            --output "neodos_image2.img" \
            --minimal
        cd "$PROJECT_ROOT"
        echo "[✓] NeoDOS FS images: $NEODOS_IMAGE, $NEODOS_IMAGE2"
    else
        echo "[!] python3 not found; skipping NeoDOS FS image"
    fi
fi
echo ""

# ============================================
# 4. Create ESP partition image (FAT32)
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
        if [ -f "$NEODOS_IMAGE" ]; then
            mcopy -i "$ESP_IMAGE" "$NEODOS_IMAGE" ::/EFI/NeoDOS/neodos.fs
            echo "[✓] Copied NeoDOS FS to ESP"
        fi
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
# 6. Create unified GPT disk image
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
echo "    Kernel:     $PROJECT_ROOT/kernel.elf"
echo "    Disk image: $DISK_IMAGE (GPT: ESP + NeoDOS FS)"
echo ""
echo "Next: bash scripts/qemu-debug.sh"
