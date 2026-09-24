# QEMU Setup for NeoDOS

## Objective

Run NeoDOS QEMU virtual machines without `sudo`, following the principle of
least privilege.

## Quick Start

```bash
# 1. Create the bridge (one-time, needs sudo)
sudo bash scripts/setup-network.sh

# 2. Run QEMU with bridge networking (no sudo)
bash scripts/qemu-debug.sh --bridge
```

## Networking Modes

NeoDOS supports three networking modes, selected via flags to
`scripts/qemu-debug.sh`:

| Mode | Flag | Sudo needed? | Guest → Internet | Host → Guest | Setup |
| ------ | ------ | ------------- | ----------------- | ------------- | ------- |
| SLiRP | _(default)_ | No | Yes (NAT) | No | None |
| Bridge | `--bridge` | Setup only | Yes (NAT) | Yes | `setup-network.sh` |
| TAP | `--tap` | Each session | Configurable | Configurable | Manual |

### SLiRP (Default)

QEMU's built-in user-mode networking (SLiRP). No privileges required, works
out of the box. The guest receives an IP via QEMU's virtual DHCP server and
can access the internet through NAT.

**Limitations:**

- The host cannot directly reach the guest.
- Performance is slightly lower than bridge mode.
- No support for advanced features (e.g., VLANs).

### Bridge (Recommended)

Uses a persistent Linux bridge (`neodos0`) with `qemu-bridge-helper` to create
TAP interfaces. The bridge is created by `setup-network.sh` and persists
across reboots.

**Advantages:**

- Host can reach the guest (e.g., for network service testing).
- Guest appears as a real device on the bridge subnet.
- Better performance than SLiRP.
- No `sudo` at runtime (qemu-bridge-helper is SUID root).

**How it works:**

1. `setup-network.sh` creates bridge `neodos0` via NetworkManager.
2. QEMU's `-netdev bridge` option calls `qemu-bridge-helper` (SUID root) to
   create a TAP interface and attach it to the bridge.
3. The bridge provides DHCP (via QEMU) in the `10.0.2.0/24` range.
4. NAT (nftables or iptables) masquerades traffic to the internet.

### TAP (Raw TAP Device)

Direct TAP networking via `/dev/net/tun`. Used for advanced scenarios where
full control over the TAP device is needed (e.g., connecting to an existing
network bridge or VLAN).

Requires the TAP device to be created before running QEMU:

```bash
sudo ip tuntap add tap0 mode tap user $(whoami)
sudo ip addr add 10.0.1.1/24 dev tap0
sudo ip link set tap0 up
```

## Bridge Setup Details

### Prerequisites

- QEMU with `qemu-bridge-helper` (SUID root at `/usr/libexec/qemu-bridge-helper`)
- NetworkManager (for persistent bridge configuration)
- nftables or iptables (for NAT)
- `/dev/kvm` and `/dev/net/tun` accessible

### What `setup-network.sh` Does

1. **Creates bridge `neodos0`** via NetworkManager with static IP `10.0.2.1/24`.
2. **Adds `allow neodos0`** to `/etc/qemu/bridge.conf`.
3. **Configures NAT** via nftables or iptables for subnet `10.0.2.0/24`.
4. **Enables IP forwarding** (`net.ipv4.ip_forward=1`), persisted to
   `/etc/sysctl.d/99-neodos.conf`.
5. **Adds FORWARD rules** to allow bridge traffic through the firewall.
6. **Adds user to `kvm` group** if needed (requires logout/login).

### Environment Variables

Override defaults:

```bash
sudo NEODOS_BRIDGE=mybridge NEODOS_SUBNET=192.168.100.0/24 bash scripts/setup-network.sh
```

| Variable | Default | Description |
| ---------- | --------- | ------------- |
| `NEODOS_BRIDGE` | `neodos0` | Bridge interface name |
| `NEODOS_SUBNET` | `10.0.2.0/24` | Bridge subnet (CIDR) |
| `NEODOS_IP` | `10.0.2.1` | Bridge IP address |
| `NEODOS_NETMASK` | `24` | Bridge netmask bits |
| `NEODOS_DHCP_RANGE` | `10.0.2.2,10.0.2.254` | DHCP range (for reference) |

### Status Check

```bash
sudo bash scripts/setup-network.sh --check
```

### Teardown

```bash
sudo bash scripts/setup-network.sh --remove
```

This removes:

- The bridge and its NetworkManager connection.
- The `allow` line from `/etc/qemu/bridge.conf`.
- NAT rules for the bridge subnet.
- IP forwarding config from `/etc/sysctl.d/99-neodos.conf`.
- FORWARD firewall rules for the bridge subnet.

## Architecture

### Bridge Mode Packet Flow

```text
┌─────────────────────────────────────────────────┐
│ Host                                            │
│                                                 │
│  ┌──────────┐    ┌─────────────────────────┐   │
│  │ neodos0  │◄───│ qemu-bridge-helper       │   │
│  │ Bridge   │    │ (SUID root, creates TAP) │   │
│  │ 10.0.2.1 │    └─────────────────────────┘   │
│  └────┬─────┘                                   │
│       │                                         │
│  ┌────▼─────┐    ┌──────────────────────┐      │
│  │ TAP0     │────│ QEMU Guest (NeoDOS)  │      │
│  │ (vnet)   │    │ DHCP → 10.0.2.x      │      │
│  └──────────┘    └──────────────────────┘      │
│                                                 │
│  NAT (nft/iptables) ─── enp0s31f6 ─── Internet  │
└─────────────────────────────────────────────────┘
```

### Bridge Mode vs. TAP Mode

| Aspect | Bridge Mode | TAP Mode |
| -------- | ------------- | ---------- |
| TAP creation | Automatic via helper | Manual (`ip tuntap add`) |
| Runtime privilege | None (helper is SUID) | None (if pre-created) |
| Persistence | Bridge persists across reboots | TAP deleted on reboot |
| Configuration | One-time setup | Each session |
| Recommendation | **Primary choice** | Advanced use only |

## Security Considerations

### qemu-bridge-helper (SUID Root)

`qemu-bridge-helper` is installed as SUID root by the QEMU package. This is
the standard, intended mechanism for unprivileged bridge networking.

**What it allows:**

- Any user (via QEMU) to create TAP interfaces and attach them to
  bridge interfaces allowed in `/etc/qemu/bridge.conf`.

**What it prevents:**

- Only bridges listed in `/etc/qemu/bridge.conf` can be used.
- The helper only creates TAP interfaces; it cannot modify other network
  configuration.
- The helper validates its input and drops privileges after opening the
  TAP device.

**Risk:** A compromised QEMU process could create TAP interfaces on allowed
bridges. Mitigated by:

- Limited to bridges in `bridge.conf` (only `neodos0` in our setup).
- QEMU's sandboxing options (Seccomp, namespace isolation).
- The helper drops to the calling user's UID after setup.

### Groups

The setup script adds the user to the `kvm` group for `/dev/kvm` access. This
is a standard, low-risk privilege:

- Allows hardware-accelerated virtualization.
- Does not grant root or other elevated privileges.
- The `kvm` group membership is restricted by the system administrator on
  production systems.

### Linux Capabilities (Alternative)

An alternative to groups is using `setcap` on the QEMU binary:

```bash
sudo setcap cap_net_admin+ep /usr/bin/qemu-system-x86_64
```

**Why we do NOT use this:**

- Broadens the attack surface (any user running QEMU gets `CAP_NET_ADMIN`).
- Harder to audit and control (capability applies to the whole binary).
- Not compatible with all QEMU deployments (snap, flatpak, etc.).
- The `qemu-bridge-helper` approach is more granular (per-bridge control).

### SELinux / AppArmor

On Fedora (default SELinux enforcing), `qemu-bridge-helper` has the
appropriate SELinux context (`virtd_t` or `qemu_bridge_helper_t`). The setup
script does not modify SELinux policies.

If you encounter SELinux denials:

```bash
sudo ausearch -m avc -ts recent
sudo sealert -a /var/log/audit/audit.log  # if setroubleshoot is installed
```

## Distribution Notes

### Fedora (Tested)

- **QEMU package:** `qemu-kvm` (includes `qemu-bridge-helper` at
  `/usr/libexec/qemu-bridge-helper`)
- **Bridge helper SUID:** Set by package manager
- **Groups:** `kvm` group exists by default
- **Firewall:** nftables by default (`firewalld`)
- **Network:** NetworkManager with nmcli

### Debian / Ubuntu

- **QEMU package:** `qemu-system-x86` or `qemu-kvm`
- **qemu-bridge-helper location:** `/usr/lib/qemu/qemu-bridge-helper`
- **SUID:** May need `sudo chmod u+s /usr/lib/qemu/qemu-bridge-helper`
- **Groups:** `kvm` group exists; add user via `sudo adduser $USER kvm`
- **Firewall:** iptables or nftables depending on version
- **Network:** NetworkManager or systemd-networkd

**Check helper location:**

```bash
dpkg -L qemu-system-x86 | grep bridge-helper
```

### Arch Linux

- **QEMU package:** `qemu-full` or `qemu-desktop`
- **qemu-bridge-helper location:** `/usr/lib/qemu/qemu-bridge-helper`
- **SUID:** Installed with SUID by default
- **Groups:** `kvm` group: `sudo usermod -aG kvm $USER`
- **Firewall:** iptables or nftables

## Troubleshooting

### Bridge not found

```bash
# Check if bridge exists
ip link show neodos0

# Run setup
sudo bash scripts/setup-network.sh
```

### Permission denied: `qemu-bridge-helper`

```bash
# Check SUID bit
ls -la /usr/libexec/qemu-bridge-helper
# Should show: -rwsr-xr-x

# Fix if missing
sudo chmod u+s /usr/libexec/qemu-bridge-helper

# Or check package
rpm -qf /usr/libexec/qemu-bridge-helper   # Fedora
dpkg -S /usr/lib/qemu/qemu-bridge-helper  # Debian
```

### Permission denied: `/dev/kvm`

```bash
# Check permissions
ls -la /dev/kvm

# Add user to kvm group
sudo usermod -aG kvm $USER
# Log out and back in, or: newgrp kvm
```

### Permission denied: `/dev/net/tun`

```bash
# Check permissions
ls -la /dev/net/tun

# Fix (if not world-accessible)
sudo chmod 666 /dev/net/tun
```

Note: Making `/dev/net/tun` world-accessible is less secure than using
`qemu-bridge-helper`. Prefer the bridge helper approach.

### Guest has no internet access

```bash
# Check NAT rules
sudo nft list table ip nat       # nftables
sudo iptables -t nat -L -n      # iptables

# Check IP forwarding
sysctl net.ipv4.ip_forward

# Check FORWARD rules
sudo iptables -L FORWARD -n     # iptables
sudo nft list chain ip filter FORWARD  # nftables
```

### Firewall blocking bridge traffic

```bash
# Fedora: allow forwarded traffic to the bridge subnet
sudo firewall-cmd --permanent --direct --add-rule ipv4 filter FORWARD 0 -s 10.0.2.0/24 -j ACCEPT
sudo firewall-cmd --permanent --direct --add-rule ipv4 filter FORWARD 0 -d 10.0.2.0/24 -j ACCEPT
sudo firewall-cmd --reload
```

## Comparison of Approaches

| Approach | Runtime Sudo? | Setup Sudo? | Persistence | Security |
| ---------- | :------------: | :-----------: | :-----------: | :--------: |
| **Bridge + qemu-bridge-helper** | No | Yes (once) | ✓ Bridge persistent | ★★★ Granular |
| SLiRP (default) | No | No | N/A | ★★★★★ No setup |
| TAP (pre-created) | No | Yes (per boot) | ✗ | ★★★★ |
| `setcap cap_net_admin` | No | Yes (once) | ✓ | ★★ Broad |
| sudo for every QEMU run | Yes | No | N/A | ★ Depends on sudoers |
| Running as root | No (no) | No | N/A | ✗ Terrible |

## References

- [QEMU Documentation: Networking](https://www.qemu.org/docs/master/system/net.html)
- [qemu-bridge-helper man page](https://manpages.debian.org/unstable/qemu-system-common/qemu-bridge-helper.8.en.html)
- [NetworkManager: Bridge](https://networkmanager.dev/docs/api/latest/settings-bridge.html)
- [nftables: NAT](https://wiki.nftables.org/wiki-nftables/index.php/NAT)
- [KVM Group](https://www.linux-kvm.org/page/FAQ#How_can_I_use_KVM_without_root_permissions.3F)
