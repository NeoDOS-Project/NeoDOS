#!/bin/bash
# NeoDOS QEMU Network Setup
# Creates a persistent bridge for QEMU VMs without requiring sudo at runtime.
# Uses qemu-bridge-helper (SUID root) for TAP creation.
#
# Usage:
#   sudo bash scripts/setup-network.sh          # Interactive setup
#   sudo bash scripts/setup-network.sh --check   # Check current status only
#   sudo bash scripts/setup-network.sh --remove  # Teardown everything
#
# Run once after cloning the repo. After setup, QEMU runs without sudo.

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BRIDGE_NAME="${NEODOS_BRIDGE:-neodos0}"
BRIDGE_SUBNET="${NEODOS_SUBNET:-10.0.2.0/24}"
BRIDGE_IP="${NEODOS_IP:-10.0.2.1}"
BRIDGE_NETMASK="${NEODOS_NETMASK:-24}"
BRIDGE_DHCP_RANGE="${NEODOS_DHCP_RANGE:-10.0.2.2,10.0.2.254}"
QEMU_BRIDGE_CONF="/etc/qemu/bridge.conf"
QEMU_BRIDGE_HELPER="/usr/libexec/qemu-bridge-helper"

# ── Color helpers ──
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
ok()  { echo -e "${GREEN}[✓]${NC} $1"; }
warn(){ echo -e "${YELLOW}[!]${NC} $1"; }
err() { echo -e "${RED}[✗]${NC} $1"; }

# ── Preflight checks ──
check_prereqs() {
    local fail=0

    if [ "$EUID" -ne 0 ]; then
        err "This script must be run as root (sudo)."
        err "After setup, QEMU will run without sudo."
        exit 1
    fi

    if ! command -v qemu-system-x86_64 &>/dev/null; then
        warn "qemu-system-x86_64 not found. Install qemu-kvm or qemu."
    fi

    if [ ! -f "$QEMU_BRIDGE_HELPER" ]; then
        err "qemu-bridge-helper not found at $QEMU_BRIDGE_HELPER"
        err "Install qemu-kvm or qemu-common package."
        fail=1
    fi

    if [ ! -x "$QEMU_BRIDGE_HELPER" ]; then
        err "qemu-bridge-helper is not executable."
        fail=1
    fi

    # Check SUID bit (should be set by package manager)
    local helper_mode
    helper_mode=$(stat -c "%A" "$QEMU_BRIDGE_HELPER" 2>/dev/null)
    if [[ "$helper_mode" != *s* ]]; then
        warn "qemu-bridge-helper is not SUID root."
        warn "Run: chmod u+s $QEMU_BRIDGE_HELPER"
        warn "Or reinstall qemu-kvm package."
    fi

    if ! command -v nmcli &>/dev/null; then
        err "nmcli not found. Install NetworkManager."
        fail=1
    fi

    if ! systemctl is-active NetworkManager &>/dev/null; then
        err "NetworkManager is not running."
        fail=1
    fi

    # Check for nftables or iptables
    if ! command -v nft &>/dev/null && ! command -v iptables &>/dev/null; then
        err "Neither nftables nor iptables found. NAT requires one."
        fail=1
    fi

    # Check ipforward
    local ipfwd
    ipfwd=$(sysctl -n net.ipv4.ip_forward 2>/dev/null || echo 0)
    if [ "$ipfwd" != "1" ] && [ "$ipfwd" != "0" ]; then
        warn "Cannot determine IP forwarding state."
    fi

    return $fail
}

# ── Check current status ──
check_status() {
    echo ""
    echo "=== NeoDOS Network Setup: Status Check ==="
    echo ""

    # Bridge
    if ip link show "$BRIDGE_NAME" &>/dev/null; then
        ok "Bridge $BRIDGE_NAME exists"
        ip -4 addr show "$BRIDGE_NAME" 2>/dev/null | grep inet | awk '{print "    IP: " $2}'
    else
        warn "Bridge $BRIDGE_NAME does not exist"
    fi

    # Bridge config
    if [ -f "$QEMU_BRIDGE_CONF" ]; then
        if grep -q "^allow $BRIDGE_NAME$" "$QEMU_BRIDGE_CONF" 2>/dev/null; then
            ok "Bridge $BRIDGE_NAME is allowed in $QEMU_BRIDGE_CONF"
        else
            warn "Bridge $BRIDGE_NAME NOT in $QEMU_BRIDGE_CONF"
            echo "    Current content: $(tr '\n' ' ' < "$QEMU_BRIDGE_CONF")"
        fi
    else
        warn "$QEMU_BRIDGE_CONF does not exist"
    fi

    # qemu-bridge-helper
    if [ -x "$QEMU_BRIDGE_HELPER" ]; then
        ok "qemu-bridge-helper is executable"
    else
        err "qemu-bridge-helper not found or not executable"
    fi

    # /dev/kvm
    if [ -c /dev/kvm ]; then
        local kvm_mode
        kvm_mode=$(stat -c "%a" /dev/kvm 2>/dev/null)
        ok "/dev/kvm exists (mode $kvm_mode)"
        if groups "$SUDO_USER" | grep -qw kvm; then
            ok "User $SUDO_USER is in kvm group"
        elif [ "${kvm_mode: -1}" -ge 6 ]; then
            ok "User $SUDO_USER can access /dev/kvm (world-accessible)"
        else
            warn "User $SUDO_USER may not have /dev/kvm access"
            warn "Run: usermod -aG kvm $SUDO_USER && newgrp kvm"
        fi
    else
        warn "/dev/kvm not found (KVM acceleration unavailable)"
    fi

    # /dev/net/tun
    if [ -c /dev/net/tun ]; then
        ok "/dev/net/tun exists"
    else
        err "/dev/net/tun not found"
    fi

    # NAT
    if nft list table ip nat &>/dev/null 2>&1; then
        ok "nftables NAT table exists"
    elif iptables -t nat -L POSTROUTING -n &>/dev/null 2>&1; then
        ok "iptables NAT rules exist"
    else
        warn "No NAT rules found for bridge traffic"
    fi

    # IP forwarding
    local ipfwd
    ipfwd=$(sysctl -n net.ipv4.ip_forward 2>/dev/null || echo 0)
    if [ "$ipfwd" = "1" ]; then
        ok "IP forwarding is enabled"
    else
        warn "IP forwarding is disabled (NAT won't work)"
    fi

    echo ""
    echo "To start QEMU with bridge networking:"
    echo "  bash scripts/qemu-debug.sh --bridge"
    echo ""
}

# ── Setup bridge ──
setup_bridge() {
    echo ""
    echo "=== NeoDOS Network Setup: Creating Bridge ==="
    echo ""

    # 1. Create bridge via NetworkManager
    if ip link show "$BRIDGE_NAME" &>/dev/null; then
        ok "Bridge $BRIDGE_NAME already exists"
    else
        echo "[*] Creating bridge $BRIDGE_NAME..."
        nmcli connection add type bridge con-name "$BRIDGE_NAME" ifname "$BRIDGE_NAME" \
            ipv4.method manual ipv4.addresses "$BRIDGE_IP/$BRIDGE_NETMASK" \
            ipv4.gateway "" ipv4.dns "" 2>&1
        nmcli connection up "$BRIDGE_NAME" 2>&1
        ok "Bridge $BRIDGE_NAME created with IP $BRIDGE_IP/$BRIDGE_NETMASK"
    fi

    # 2. Allow bridge in qemu-bridge-helper config
    mkdir -p "$(dirname "$QEMU_BRIDGE_CONF")"
    if [ -f "$QEMU_BRIDGE_CONF" ]; then
        if grep -q "^allow $BRIDGE_NAME$" "$QEMU_BRIDGE_CONF"; then
            ok "$BRIDGE_NAME already allowed in $QEMU_BRIDGE_CONF"
        else
            echo "allow $BRIDGE_NAME" >> "$QEMU_BRIDGE_CONF"
            ok "Added allow $BRIDGE_NAME to $QEMU_BRIDGE_CONF"
        fi
    else
        echo "allow $BRIDGE_NAME" > "$QEMU_BRIDGE_CONF"
        ok "Created $QEMU_BRIDGE_CONF with allow $BRIDGE_NAME"
    fi

    # 3. Enable NAT for bridge traffic
    local fw_tool=""
    if command -v nft &>/dev/null; then
        fw_tool="nft"
    elif command -v iptables &>/dev/null; then
        fw_tool="iptables"
    fi

    case "$fw_tool" in
        nft)
            if ! nft list table ip nat &>/dev/null 2>&1; then
                echo "[*] Creating nftables NAT rules..."
                nft add table ip nat
                nft add chain ip nat POSTROUTING { type nat hook postrouting priority srcnat\; }
                nft add rule ip nat POSTROUTING ip saddr "$BRIDGE_SUBNET" masquerade
                ok "nftables NAT configured for $BRIDGE_SUBNET"
            else
                ok "nftables NAT table already exists"
                # Ensure our rule exists
                if ! nft list chain ip nat POSTROUTING 2>/dev/null | grep -q "$BRIDGE_SUBNET"; then
                    nft add rule ip nat POSTROUTING ip saddr "$BRIDGE_SUBNET" masquerade
                    ok "Added NAT rule for $BRIDGE_SUBNET"
                fi
            fi

            # Save nftables rules
            mkdir -p /etc/nftables
            nft list ruleset > /etc/nftables/neodos.conf 2>/dev/null || true
            ok "nftables rules saved to /etc/nftables/neodos.conf"
            ;;

        iptables)
            if ! iptables -t nat -C POSTROUTING -s "$BRIDGE_SUBNET" -j MASQUERADE 2>/dev/null; then
                iptables -t nat -A POSTROUTING -s "$BRIDGE_SUBNET" -j MASQUERADE
                ok "iptables NAT configured for $BRIDGE_SUBNET"
            else
                ok "iptables NAT rule already exists"
            fi

            # Save iptables rules
            if command -v iptables-save &>/dev/null; then
                mkdir -p /etc/iptables
                iptables-save > /etc/iptables/neodos.rules 2>/dev/null || true
                ok "iptables rules saved"
            fi
            ;;
    esac

    # 4. Enable IP forwarding
    local ipfwd
    ipfwd=$(sysctl -n net.ipv4.ip_forward 2>/dev/null || echo 0)
    if [ "$ipfwd" != "1" ]; then
        sysctl -w net.ipv4.ip_forward=1
        echo "net.ipv4.ip_forward = 1" > /etc/sysctl.d/99-neodos.conf
        ok "IP forwarding enabled (persistent)"
    else
        ok "IP forwarding already enabled"
    fi

    # 5. Ensure forward rules in iptables/nftables allow bridge traffic
    case "$fw_tool" in
        nft)
            # Ensure forward chain allows bridge traffic
            if ! nft list chain ip filter FORWARD 2>/dev/null | grep -q "$BRIDGE_SUBNET"; then
                nft add rule ip filter FORWARD ip saddr "$BRIDGE_SUBNET" accept 2>/dev/null || true
                nft add rule ip filter FORWARD ip daddr "$BRIDGE_SUBNET" accept 2>/dev/null || true
                ok "Added FORWARD accept rules for $BRIDGE_SUBNET"
            fi
            ;;
        iptables)
            if ! iptables -C FORWARD -s "$BRIDGE_SUBNET" -j ACCEPT 2>/dev/null; then
                iptables -I FORWARD -s "$BRIDGE_SUBNET" -j ACCEPT
            fi
            if ! iptables -C FORWARD -d "$BRIDGE_SUBNET" -j ACCEPT 2>/dev/null; then
                iptables -I FORWARD -d "$BRIDGE_SUBNET" -j ACCEPT
            fi
            ok "Added FORWARD accept rules for $BRIDGE_SUBNET"
            ;;
    esac

    # 6. User setup
    if [ -n "${SUDO_USER:-}" ]; then
        # Add to kvm group if needed
        if groups "$SUDO_USER" | grep -qw kvm; then
            ok "User $SUDO_USER is already in kvm group"
        else
            if usermod -aG kvm "$SUDO_USER" 2>/dev/null; then
                warn "Added $SUDO_USER to kvm group. Log out and back in for this to take effect."
                warn "Or run: newgrp kvm"
            else
                warn "Could not add $SUDO_USER to kvm group"
            fi
        fi
    fi

    echo ""
    ok "Bridge setup complete!"
    echo ""
    echo "Bridge:       $BRIDGE_NAME ($BRIDGE_IP/$BRIDGE_NETMASK)"
    echo "Guest DHCP:   QEMU built-in (10.0.2.x)"
    echo "NAT:          $BRIDGE_SUBNET -> internet"
    echo ""
    echo "Next step:"
    echo "  bash scripts/qemu-debug.sh --bridge"
    echo ""
}

# ── Teardown ──
teardown() {
    echo ""
    echo "=== NeoDOS Network Setup: Removing Bridge ==="
    echo ""

    if ip link show "$BRIDGE_NAME" &>/dev/null; then
        nmcli connection down "$BRIDGE_NAME" 2>/dev/null || true
        nmcli connection delete "$BRIDGE_NAME" 2>/dev/null || true
        ip link delete "$BRIDGE_NAME" 2>/dev/null || true
        ok "Bridge $BRIDGE_NAME removed"
    else
        warn "Bridge $BRIDGE_NAME does not exist"
    fi

    # Remove from qemu bridge config
    if [ -f "$QEMU_BRIDGE_CONF" ]; then
        sed -i "/^allow $BRIDGE_NAME$/d" "$QEMU_BRIDGE_CONF"
        ok "Removed $BRIDGE_NAME from $QEMU_BRIDGE_CONF"
        # Remove file if empty
        if [ ! -s "$QEMU_BRIDGE_CONF" ]; then
            rm -f "$QEMU_BRIDGE_CONF"
            ok "Removed empty $QEMU_BRIDGE_CONF"
        fi
    fi

    # Remove NAT rules
    if command -v nft &>/dev/null; then
        if nft list chain ip nat POSTROUTING 2>/dev/null | grep -q "$BRIDGE_SUBNET"; then
            local handle
            handle=$(nft -a list chain ip nat POSTROUTING 2>/dev/null | grep "$BRIDGE_SUBNET" | grep -oP 'handle \K\d+')
            if [ -n "$handle" ]; then
                nft delete rule ip nat POSTROUTING handle "$handle" 2>/dev/null || true
            fi
            ok "Removed NAT rule for $BRIDGE_SUBNET"
        fi
    elif command -v iptables &>/dev/null; then
        iptables -t nat -D POSTROUTING -s "$BRIDGE_SUBNET" -j MASQUERADE 2>/dev/null || true
        ok "Removed iptables NAT rule"
    fi

    # Remove forward rules
    if command -v nft &>/dev/null; then
        nft delete rule ip filter FORWARD ip saddr "$BRIDGE_SUBNET" accept 2>/dev/null || true
        nft delete rule ip filter FORWARD ip daddr "$BRIDGE_SUBNET" accept 2>/dev/null || true
    elif command -v iptables &>/dev/null; then
        iptables -D FORWARD -s "$BRIDGE_SUBNET" -j ACCEPT 2>/dev/null || true
        iptables -D FORWARD -d "$BRIDGE_SUBNET" -j ACCEPT 2>/dev/null || true
    fi

    # Remove sysctl config
    rm -f /etc/sysctl.d/99-neodos.conf
    ok "Removed sysctl IP forwarding config"

    echo ""
    ok "Teardown complete."
    echo ""
}

# ── Main ──
case "${1:-}" in
    --check|-c)
        check_prereqs || true
        check_status
        ;;
    --remove|-r|--teardown)
        if [ "$EUID" -ne 0 ]; then
            err "Teardown requires root (sudo)."
            exit 1
        fi
        teardown
        ;;
    --help|-h)
        echo "NeoDOS QEMU Network Setup"
        echo ""
        echo "Usage:"
        echo "  sudo bash scripts/setup-network.sh              Create bridge + configure"
        echo "  sudo bash scripts/setup-network.sh --check      Check current status"
        echo "  sudo bash scripts/setup-network.sh --remove     Teardown everything"
        echo "  sudo bash scripts/setup-network.sh --help       This help"
        echo ""
        echo "Environment variables (optional):"
        echo "  NEODOS_BRIDGE=neodos0       Bridge name (default: neodos0)"
        echo "  NEODOS_SUBNET=10.0.2.0/24   Bridge subnet (default: 10.0.2.0/24)"
        echo "  NEODOS_IP=10.0.2.1          Bridge IP (default: 10.0.2.1)"
        ;;
    *)
        check_prereqs
        check_status
        setup_bridge
        ;;
esac
