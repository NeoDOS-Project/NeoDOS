# VirtualBox Backend for NeoDOS

## Overview

NeoDev supports multiple hypervisor backends through a common abstraction layer.
VirtualBox is a fully supported backend alongside QEMU.

## Prerequisites

- [VirtualBox](https://www.virtualbox.org/) installed
- `VBoxManage` in PATH (included with VirtualBox)

### Installation

**Debian/Ubuntu:**
```bash
sudo apt install virtualbox virtualbox-ext-pack
```

**Fedora:**
```bash
sudo dnf install VirtualBox
```

**Arch:**
```bash
sudo pacman -S virtualbox
```

Verify installation:
```bash
VBoxManage --version
```

## Quick Start

```bash
# 1. Build NeoDOS disk image
cargo run --manifest-path tools/neodev/Cargo.toml -- build --image

# 2. Run with VirtualBox (auto-creates VM)
cargo run --manifest-path tools/neodev/Cargo.toml -- run --backend virtualbox

# Run headless
cargo run --manifest-path tools/neodev/Cargo.toml -- run --backend virtualbox --headless
```

## VM Lifecycle

### Create VM
```bash
neodev vm create --backend virtualbox
```
Creates and configures the VM if it doesn't exist. Automatically converts
`disk_image.img` to `disk_image.vdi`.

### Start VM
```bash
neodev vm start --backend virtualbox           # GUI mode
neodev vm start --backend virtualbox --headless # Headless mode
```

### Stop VM
```bash
neodev vm stop --backend virtualbox
```
Sends ACPI power button signal. Forces poweroff if ACPI fails.

### Reset VM
```bash
neodev vm reset --backend virtualbox
```

### Check Status
```bash
neodev vm status --backend virtualbox
```

### Delete VM
```bash
neodev vm delete --backend virtualbox
```
Removes the VirtualBox VM and all associated files.

## Configuration

Configure the default backend in `neodev.toml`:

```toml
[vm]
backend = "virtualbox"
memory = 4096
cpus = 4
```

Override per-command with `--backend`:
```bash
neodev run --backend qemu
neodev run --backend virtualbox
```

## VM Configuration

The VirtualBox backend automatically configures:

| Setting | Value |
|---------|-------|
| OS Type | Linux_64 |
| Firmware | EFI |
| Chipset | ICH9 |
| Memory | Configurable (default: 512 MB) |
| CPUs | Configurable (default: 2) |
| Storage | AHCI/SATA with VDI disk |
| Serial | COM1 → file (for test logging) |
| Network | NAT (default) or Bridged |

### Network Modes

- **NAT** (default): Guest accesses internet via NAT. Host cannot reach guest.
- **Bridged**: Guest appears on the host network. Auto-detects active interface.

## Running Tests

```bash
# With QEMU (default)
cargo run --manifest-path tools/neodev/Cargo.toml -- test

# With VirtualBox
cargo run --manifest-path tools/neodev/Cargo.toml -- test --backend virtualbox
```

The test runner starts the VM headless, monitors the serial log for
completion markers (`ALL_TESTS_COMPLETE`, `CMDTEST_COMPLETE`, etc.),
and stops the VM when tests finish or timeout.

## Serial Output

VirtualBox serial output is logged to file for debugging:
- Default path during testing: `/tmp/neodos_serial.log`
- During interactive runs: path specified via `--serial` flag

## Architecture

```text
NeoDev
  └── vmm (Virtual Machine Manager)
       ├── HypervisorBackend trait
       ├── QemuBackend
       └── VirtualBoxBackend    ← this
```

All hypervisor-specific logic is encapsulated in backend implementations.
Adding a new hypervisor requires only implementing the `HypervisorBackend` trait.

## DHCP Integration Test

NeoDev includes an automated DHCP integration test that uses VirtualBox
in bridged networking mode to obtain a real IP address from the local
network router.

### Requirements

- VirtualBox installed with VBoxManage in PATH
- Your user added to the `vboxusers` group
- A physical network interface (Ethernet preferred, Wi-Fi supported)
- A DHCP server on the local network (typically your router)

### Running the Test

```bash
# 1. Build NeoDOS with all components
cargo run --manifest-path tools/neodev/Cargo.toml -- build --image

# 2. Run the DHCP integration test
cargo run --manifest-path tools/neodev/Cargo.toml -- dhcp --backend virtualbox
```

### What the Test Does

1. Generates a registry hive with `EnableNetworkTest=1`
2. Rebuilds the disk image with the test configuration
3. Configures VirtualBox with **Bridged Adapter** mode
4. Automatically detects a physical Ethernet or Wi-Fi interface
5. Starts NeoDOS in headless mode
6. The kernel boots and initializes networking (e1000 NIC)
7. NeoInit launches `dhcptest.nxe` which performs DHCP DORA
8. Validates: IP obtained, not APIPA, mask exists, gateway exists
9. Displays full network configuration
10. Reports pass/fail

### Expected Output

```
[*] NeoDOS DHCP Integration Test
  Backend: virtualbox
  Network: Bridged (real DHCP)

  Bridged interface candidates: 1 Ethernet (eth0), 0 Wi-Fi ()
  Selected: eth0 for bridged networking
  ...

[DHCPTEST] DISCOVER xid=0x12345678
[DHCPTEST] OFFER from 192.168.1.1 IP=192.168.1.100
[DHCPTEST] ACK: IP=192.168.1.100 mask=255.255.255.0 gw=192.168.1.1 lease=86400s
[DHCPTEST] DORA complete
[DHCPTEST] === Validation ===
[DHCPTEST] [PASS] IP obtained: 192.168.1.100
[DHCPTEST] [PASS] IP is not APIPA
[DHCPTEST] [PASS] Subnet mask: 255.255.255.0
[DHCPTEST] [PASS] Gateway: 192.168.1.1
[DHCPTEST] [PASS] DNS: 8.8.8.8
[DHCPTEST] [PASS] Lease time: 86400 s
...
[+] DHCP TEST COMPLETE
[✓] DHCP INTEGRATION TEST: SUCCESS
```

### Validation Checks

| Check | Description |
|-------|-------------|
| IP obtained | IP != 0.0.0.0 |
| Not APIPA | IP not in 169.254.x.x range |
| Subnet mask | Valid mask (not 0 or 0xFFFFFFFF) |
| Gateway | Gateway assigned (warning, not fatal) |
| DNS server | DNS server assigned (warning, not fatal) |
| Lease time | Lease time > 0 (warning, not fatal) |

### Timeouts

The test has a default timeout of 180 seconds. If DHCP negotiation
does not complete within this time, the test fails with a timeout error.
This can be overridden with `--timeout`:

```bash
cargo run --manifest-path tools/neodev/Cargo.toml -- dhcp --backend virtualbox --timeout 300
```

## Backend Comparison

| Feature | QEMU | VirtualBox |
|---------|------|------------|
| Interactive run | `neodev run` | `neodev run --backend virtualbox` |
| Headless mode | `--headless` | `--headless` |
| Tests | `neodev test` | `neodev test --backend virtualbox` |
| DHCP test | ❌ | `neodev dhcp --backend virtualbox` |
| Serial monitoring | Telnet monitor + file | File only |
| KVM acceleration | ✅ | N/A (native) |
| GDB debugging | ✅ | ❌ |
| EFI support | ✅ (OVMF) | ✅ |
| Network modes | user/tap/bridge | NAT/bridged |
| Disk format | Raw `.img` | VDI (auto-converted) |
| Bridge interface detection | `ip link` | `ip link` + carrier + IP check |

## Troubleshooting

### VBoxManage not found
```bash
which VBoxManage
# Should output a path like /usr/bin/VBoxManage
# If not found, ensure VirtualBox is installed correctly
```

### VM already exists
If the VM already exists, NeoDev reuses it automatically. To recreate:
```bash
neodev vm delete --backend virtualbox
neodev vm create --backend virtualbox
```

### Disk image updated
When `disk_image.img` is rebuilt, NeoDev automatically re-converts it to
VDI on the next run if the raw image is newer.

### Permission denied
Ensure your user has permission to run VirtualBox VMs:
```bash
# Add user to vboxusers group
sudo usermod -aG vboxusers $USER
# Log out and back in
```

## Deprecated Script

The old `scripts/vbox-setup.sh` has been removed. All VirtualBox management
is now handled by NeoDev via:
```bash
neodev vm create --backend virtualbox
neodev run --backend virtualbox
neodev test --backend virtualbox
```
