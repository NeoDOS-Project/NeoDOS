#!/usr/bin/env python3
"""
gen_system_hiv.py — Generate NeoDOS SYSTEM.HIV with default values.

Produces a NEOHv1 binary hive file matching the kernel's cell-based format
(src/cm/hive.rs). Embeds default values identical to kernel's
cm_ensure_default_values() in src/cm/mod.rs.

Usage:
  python3 scripts/gen_system_hiv.py [options] [output_path]

Options:
  --enable-tests         Set EnableTests=1 (runs cmdtest/shtest/stresscmd at boot)
  --enable-network-test  Set EnableNetworkTest=1 (runs dhcptest at boot)

Default output: scripts/system.hiv (copied to neodos_image.img later)
"""

import argparse
import struct
import sys
from pathlib import Path

NEOH_MAGIC = 0x484F454E  # "NEOH"
NEOH_VERSION = 1
NULL_CELL = 0xFFFFFFFF

REG_SZ = 1
REG_DWORD = 2


class HiveBuilder:
    """Build a NEOH hive binary cell by cell."""

    def __init__(self):
        self.cells: dict[int, bytes] = {}
        self.next_idx = 1  # cell 0 is root

    def alloc_idx(self) -> int:
        idx = self.next_idx
        self.next_idx += 1
        return idx

    def add_key(self, idx: int, name: str, parent: int,
                subkeys_head: int = NULL_CELL,
                subkeys_sibling: int = NULL_CELL,
                values_head: int = NULL_CELL,
                sec_desc: int = NULL_CELL,
                last_write: int = 0) -> int:
        name_bytes = name.encode("ascii")
        assert len(name_bytes) <= 65535
        buf = bytearray()
        buf.extend(struct.pack("<I", idx))
        buf.append(1)  # Key cell type
        buf.extend(struct.pack("<H", len(name_bytes)))
        buf.extend(name_bytes)
        buf.extend(struct.pack("<IIIII", parent, subkeys_head,
                               subkeys_sibling, values_head, sec_desc))
        buf.extend(struct.pack("<Q", last_write))
        self.cells[idx] = bytes(buf)
        return idx

    def add_value(self, idx: int, name: str, value_type: int,
                  data: bytes, next_val: int = NULL_CELL) -> int:
        name_bytes = name.encode("ascii")
        buf = bytearray()
        buf.extend(struct.pack("<I", idx))
        buf.append(2)  # Value cell type
        buf.extend(struct.pack("<H", len(name_bytes)))
        buf.extend(name_bytes)
        buf.extend(struct.pack("<II", value_type, len(data)))
        buf.extend(data)
        buf.extend(struct.pack("<I", next_val))
        self.cells[idx] = bytes(buf)
        return idx

    def serialize(self) -> bytes:
        """Serialize all cells into NEOH binary format.
        Checksum must match Rust's calc in cm/hive/serialize.rs:
        For Key(1): idx + type + name_len + name_bytes + parent + subkeys_head
                    + subkeys_sibling + values_head + sec_desc + last_write(high+low)
        For Value(2): idx + type + name_len + name_bytes + value_type
                      + data_len + data_bytes + next_val
        """
        entries = sorted(self.cells.items())
        raw_payload = b"".join(data for _, data in entries)

        # Compute checksum matching Rust's algorithm
        checksum = 0
        for idx, data in entries:
            # Read fields from raw bytes to compute checksum like Rust does
            # The raw data starts with [idx][type][payload...]
            cell_idx = struct.unpack("<I", data[0:4])[0]
            cell_type = data[4]
            checksum = (checksum + cell_idx) & 0xFFFFFFFF
            checksum = (checksum + cell_type) & 0xFFFFFFFF
            pos = 5
            name_len = struct.unpack("<H", data[pos:pos+2])[0]
            checksum = (checksum + name_len) & 0xFFFFFFFF
            pos += 2
            for b in data[pos:pos+name_len]:
                checksum = (checksum + b) & 0xFFFFFFFF
            pos += name_len
            if cell_type == 1:  # Key
                parent = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + parent) & 0xFFFFFFFF
                pos += 4
                subkeys_head = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + subkeys_head) & 0xFFFFFFFF
                pos += 4
                subkeys_sibling = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + subkeys_sibling) & 0xFFFFFFFF
                pos += 4
                values_head = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + values_head) & 0xFFFFFFFF
                pos += 4
                sec_desc = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + sec_desc) & 0xFFFFFFFF
                pos += 4
                last_write = struct.unpack("<Q", data[pos:pos+8])[0]
                lw_low = last_write & 0xFFFFFFFF
                lw_high = (last_write >> 32) & 0xFFFFFFFF
                checksum = (checksum + lw_low) & 0xFFFFFFFF
                checksum = (checksum + lw_high) & 0xFFFFFFFF
            elif cell_type == 2:  # Value
                value_type = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + value_type) & 0xFFFFFFFF
                pos += 4
                data_len = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + data_len) & 0xFFFFFFFF
                pos += 4
                for b in data[pos:pos+data_len]:
                    checksum = (checksum + b) & 0xFFFFFFFF
                pos += data_len
                nxt = struct.unpack("<I", data[pos:pos+4])[0]
                checksum = (checksum + nxt) & 0xFFFFFFFF

        header = struct.pack("<IIII", NEOH_MAGIC, NEOH_VERSION,
                             len(entries), checksum)
        data = header + raw_payload
        return data


def build_default_system_hive() -> bytes:
    """Build SYSTEM.HIV with default registry values matching the kernel."""
    b = HiveBuilder()

    # Cell 0: Root key "SYSTEM"
    b.add_key(0, "SYSTEM", NULL_CELL)

    # CurrentControlSet (child of root)
    ccs = b.alloc_idx()
    b.add_key(ccs, "CurrentControlSet", 0, subkeys_head=0)

    # CurrentControlSet\Services (next sibling will chain)
    svc = b.alloc_idx()
    b.add_key(svc, "Services", ccs, subkeys_head=0)

    # CurrentControlSet\Control
    ctrl = b.alloc_idx()
    b.add_key(ctrl, "Control", ccs, subkeys_head=0)

    # Update Services and Control as siblings under CurrentControlSet
    b.add_key(svc, "Services", ccs,
              subkeys_sibling=ctrl)  # Services -> Control chain

    # CurrentControlSet\Services\NeoInit
    neoinit = b.alloc_idx()
    b.add_key(neoinit, "NeoInit", svc, subkeys_head=0)

    # Values under NeoInit
    v_default_shell = b.alloc_idx()
    b.add_value(v_default_shell, "DefaultShell", REG_SZ,
                b"C:\\Programs\\neoshell.nxe\x00")

    v_enable_vt = b.alloc_idx()
    b.add_value(v_enable_vt, "EnableVT", REG_DWORD,
                struct.pack("<I", 1),
                next_val=v_default_shell)

    v_auto_start = b.alloc_idx()
    b.add_value(v_auto_start, "AutoStartServices", REG_SZ,
                b"\x00",
                next_val=v_enable_vt)

    # Update NeoInit key with values_head pointing to chain: AutoStart -> EnableVT -> DefaultShell
    b.add_key(neoinit, "NeoInit", svc,
              values_head=v_auto_start)

    # CurrentControlSet\Services\Network
    net = b.alloc_idx()
    b.add_key(net, "Network", svc)

    # CurrentControlSet\Services\Network\Interfaces
    ifaces = b.alloc_idx()
    b.add_key(ifaces, "Interfaces", net)

    # CurrentControlSet\Services\Network\Interfaces\0
    iface0 = b.alloc_idx()
    b.add_key(iface0, "0", ifaces)

    # DHCPEnabled = 1 under Interfaces\0
    v_dhcp = b.alloc_idx()
    b.add_value(v_dhcp, "DHCPEnabled", REG_DWORD,
                struct.pack("<I", 1))
    b.add_key(iface0, "0", ifaces, values_head=v_dhcp)

    # CurrentControlSet\Control\WaitForNetwork = 0
    v_wait = b.alloc_idx()
    b.add_value(v_wait, "WaitForNetwork", REG_DWORD,
                struct.pack("<I", 0))
    b.add_key(ctrl, "Control", ccs, values_head=v_wait)

    # Now wire up the tree properly:
    # Root subkeys: CurrentControlSet only
    b.add_key(0, "SYSTEM", NULL_CELL, subkeys_head=ccs)

    # CurrentControlSet subkeys: Services(head) -> Control(sibling)
    b.add_key(ccs, "CurrentControlSet", 0,
              subkeys_head=svc)
    b.add_key(svc, "Services", ccs,
              subkeys_sibling=ctrl)

    # Services subkeys: Network (NeoInit is NOT a subkey of Services in our tree)
    # Wait, looking at the kernel code again:
    # ensure_key_path(&mut hm.hive, root, "CurrentControlSet\\Services\\NeoInit")
    # So the path is root -> CurrentControlSet -> Services -> NeoInit
    # And Services -> Network -> Interfaces -> 0
    # And CurrentControlSet -> Control
    #
    # So Services has subkeys: NeoInit and Network
    # Let me restructure

    return b.serialize()


def build_default_system_hive_v2(enable_tests=False, enable_network_test=False) -> bytes:
    """
    Build SYSTEM.HIV with proper tree structure matching kernel's
    ensure_key_path() traversal. Includes default service entries
    (Dhcpd) for the Service Manager (SM-001) and Power plan defaults
    for the Power Manager (PM-PHASE2).

    Args:
        enable_tests: Set EnableTests=1 for boot-time test execution
        enable_network_test: Set EnableNetworkTest=1 for DHCP test
    """
    b = HiveBuilder()

    # Total cells: 55 — root + CurrentControlSet tree (30) + Power tree (24) + EnableNetworkTest (1)

    # ── ALLOCATE all cells first ──
    _ROOT = 0
    _CCS = 1
    _SVC = 2
    _NEO = 3
    _DHCPC = 4
    _NET = 5
    _IFC = 6
    _IF0 = 7
    _CTL = 8
    _V_SHELL = 9
    _V_VT = 10
    _V_AUTO = 11
    _V_TESTS = 12
    _V_DHCP = 13
    _V_WAIT = 14
    _V_BENCH = 23
    _V_AHCI = 24
    _LOC_KEY = 25
    _V_LANG = 26
    _V_DNAME = 15
    _V_BPATH = 16
    _V_IPATH = 17
    _V_STYPE = 18
    _V_RPOL = 19
    _V_MFAIL = 20
    _V_DEPS = 21
    _V_DESC = 22

    # ── Session Manager + Environment cells ──
    _SM = 27
    _ENV = 28
    _V_PATH = 29

    # ── Power tree cells (PM-PHASE2) ──
    _PWR = 30
    _PLN = 31
    _BAL = 32
    _PER = 33
    _SAV = 34
    _V_AP = 35
    _V_BAL_DT = 36
    _V_BAL_ST = 37
    _V_BAL_HE = 38
    _V_BAL_CP = 39
    _V_BAL_LA = 40
    _V_BAL_PBA = 41
    _V_PER_DT = 42
    _V_PER_ST = 43
    _V_PER_HE = 44
    _V_PER_CP = 45
    _V_PER_LA = 46
    _V_PER_PBA = 47
    _V_SAV_DT = 48
    _V_SAV_ST = 49
    _V_SAV_HE = 50
    _V_SAV_CP = 51
    _V_SAV_LA = 52
    _V_SAV_PBA = 53

    # ── Network test flag cell ──
    _V_NETTEST = 54

    b.next_idx = 55

    # ── VALUES (linked lists) ──
    # NeoInit values: DefaultShell -> EnableVT -> AutoStartServices -> EnableTests -> EnableNetworkTest
    tests_val = 1 if enable_tests else 0
    net_test_val = 1 if enable_network_test else 0
    b.add_value(_V_SHELL, "DefaultShell", REG_SZ,
                b"C:\\Programs\\neoshell.nxe\x00")
    b.add_value(_V_VT, "EnableVT", REG_DWORD,
                struct.pack("<I", 1), next_val=_V_SHELL)
    b.add_value(_V_AUTO, "AutoStartServices", REG_SZ,
                b"", next_val=_V_VT)
    b.add_value(_V_TESTS, "EnableTests", REG_DWORD,
                struct.pack("<I", tests_val), next_val=_V_AUTO)
    b.add_value(_V_NETTEST, "EnableNetworkTest", REG_DWORD,
                struct.pack("<I", net_test_val), next_val=_V_TESTS)

    # Dhcpc values: Description -> Dependencies -> MaxFailures -> RestartPolicy -> StartType -> ImagePath -> BinaryPath -> DisplayName
    b.add_value(_V_DESC, "Description", REG_SZ,
                b"DHCP Client Service\x00")
    b.add_value(_V_DEPS, "Dependencies", REG_SZ,
                b"", next_val=_V_DESC)
    b.add_value(_V_MFAIL, "MaxFailures", REG_DWORD,
                struct.pack("<I", 3), next_val=_V_DEPS)
    b.add_value(_V_RPOL, "RestartPolicy", REG_DWORD,
                struct.pack("<I", 1), next_val=_V_MFAIL)  # OnCrash
    b.add_value(_V_STYPE, "StartType", REG_DWORD,
                struct.pack("<I", 4), next_val=_V_RPOL)  # Disabled
    b.add_value(_V_IPATH, "ImagePath", REG_SZ,
                b"C:\\System\\Tools\\dhcpd.nxe\x00", next_val=_V_STYPE)
    b.add_value(_V_BPATH, "BinaryPath", REG_SZ,
                b"C:\\System\\Tools\\dhcpd.nxe\x00", next_val=_V_IPATH)
    b.add_value(_V_DNAME, "DisplayName", REG_SZ,
                b"DHCP Client\x00", next_val=_V_BPATH)

    # Interfaces\0 values: DHCPEnabled
    b.add_value(_V_DHCP, "DHCPEnabled", REG_DWORD,
                struct.pack("<I", 0))

    # Control values: BenchmarkReport(1) -> AhciDebug(1) -> WaitForNetwork(0)
    b.add_value(_V_BENCH, "BenchmarkReport", REG_DWORD,
                struct.pack("<I", 0))
    b.add_value(_V_AHCI, "AhciDebug", REG_DWORD,
                struct.pack("<I", 0), next_val=_V_BENCH)
    b.add_value(_V_WAIT, "WaitForNetwork", REG_DWORD,
                struct.pack("<I", 0), next_val=_V_AHCI)

    # Locale\Language value
    b.add_value(_V_LANG, "Language", REG_SZ, b"es-ES")

    # ── Power plan values (PM-PHASE2) ──
    # Balanced chain: PowerButtonAction -> LidAction -> CpuPolicy -> HibernateEnabled -> SleepTimeout -> DisplayTimeout
    b.add_value(_V_BAL_DT, "DisplayTimeout", REG_DWORD, struct.pack("<I", 10))
    b.add_value(_V_BAL_ST, "SleepTimeout", REG_DWORD, struct.pack("<I", 30), next_val=_V_BAL_DT)
    b.add_value(_V_BAL_HE, "HibernateEnabled", REG_DWORD, struct.pack("<I", 1), next_val=_V_BAL_ST)
    b.add_value(_V_BAL_CP, "CpuPolicy", REG_SZ, b"Balanced", next_val=_V_BAL_HE)
    b.add_value(_V_BAL_LA, "LidAction", REG_DWORD, struct.pack("<I", 1), next_val=_V_BAL_CP)
    b.add_value(_V_BAL_PBA, "PowerButtonAction", REG_DWORD, struct.pack("<I", 3), next_val=_V_BAL_LA)

    # Performance chain
    b.add_value(_V_PER_DT, "DisplayTimeout", REG_DWORD, struct.pack("<I", 30))
    b.add_value(_V_PER_ST, "SleepTimeout", REG_DWORD, struct.pack("<I", 60), next_val=_V_PER_DT)
    b.add_value(_V_PER_HE, "HibernateEnabled", REG_DWORD, struct.pack("<I", 0), next_val=_V_PER_ST)
    b.add_value(_V_PER_CP, "CpuPolicy", REG_SZ, b"Performance", next_val=_V_PER_HE)
    b.add_value(_V_PER_LA, "LidAction", REG_DWORD, struct.pack("<I", 0), next_val=_V_PER_CP)
    b.add_value(_V_PER_PBA, "PowerButtonAction", REG_DWORD, struct.pack("<I", 3), next_val=_V_PER_LA)

    # PowerSaver chain
    b.add_value(_V_SAV_DT, "DisplayTimeout", REG_DWORD, struct.pack("<I", 3))
    b.add_value(_V_SAV_ST, "SleepTimeout", REG_DWORD, struct.pack("<I", 10), next_val=_V_SAV_DT)
    b.add_value(_V_SAV_HE, "HibernateEnabled", REG_DWORD, struct.pack("<I", 1), next_val=_V_SAV_ST)
    b.add_value(_V_SAV_CP, "CpuPolicy", REG_SZ, b"PowerSave", next_val=_V_SAV_HE)
    b.add_value(_V_SAV_LA, "LidAction", REG_DWORD, struct.pack("<I", 1), next_val=_V_SAV_CP)
    b.add_value(_V_SAV_PBA, "PowerButtonAction", REG_DWORD, struct.pack("<I", 2), next_val=_V_SAV_LA)

    # ActivePlan = 0 (Balanced)
    b.add_value(_V_AP, "ActivePlan", REG_DWORD, struct.pack("<I", 0))

    # Environment\PATH value
    b.add_value(_V_PATH, "PATH", REG_SZ,
                b"\\Programs;\\System\\Tools\x00")

    # ── KEYS (bottom-up) ──
    # NeoInit (child of Services)
    b.add_key(_NEO, "NeoInit", _SVC, values_head=_V_NETTEST)

    # Dhcpc (child of Services, sibling of NeoInit)
    b.add_key(_DHCPC, "Dhcpc", _SVC, values_head=_V_DNAME)

    # Interfaces\0 (child of Interfaces)
    b.add_key(_IF0, "0", _IFC, values_head=_V_DHCP)

    # Interfaces (child of Network)
    b.add_key(_IFC, "Interfaces", _NET, subkeys_head=_IF0)

    # Network (child of Services, sibling of Dhcpc)
    b.add_key(_NET, "Network", _SVC, subkeys_head=_IFC)

    # Services: subkeys = NeoInit -> Dhcpc -> Network (sibling chain)
    b.add_key(_NEO, "NeoInit", _SVC, values_head=_V_NETTEST,
              subkeys_sibling=_DHCPC)
    b.add_key(_DHCPC, "Dhcpc", _SVC, values_head=_V_DNAME,
              subkeys_sibling=_NET)
    b.add_key(_SVC, "Services", _CCS, subkeys_head=_NEO)

    # Locale (child of Control, sibling of Session Manager)
    b.add_key(_LOC_KEY, "Locale", _CTL, values_head=_V_LANG, subkeys_sibling=_SM)

    # Environment (child of Session Manager)
    b.add_key(_ENV, "Environment", _SM, values_head=_V_PATH)

    # Session Manager (child of Control, sibling of Locale)
    b.add_key(_SM, "Session Manager", _CTL, subkeys_head=_ENV)

    # Control (child of CurrentControlSet, sibling of Services)
    b.add_key(_CTL, "Control", _CCS, values_head=_V_WAIT, subkeys_head=_LOC_KEY)

    # ── Power keys (PM-PHASE2) ──
    b.add_key(_PWR, "Power", _ROOT, subkeys_head=_PLN, values_head=_V_AP)
    b.add_key(_PLN, "Plans", _PWR, subkeys_head=_BAL)
    # Plans subkey chain: Balanced -> Performance -> PowerSaver
    b.add_key(_BAL, "Balanced", _PLN, values_head=_V_BAL_PBA, subkeys_sibling=_PER)
    b.add_key(_PER, "Performance", _PLN, values_head=_V_PER_PBA, subkeys_sibling=_SAV)
    b.add_key(_SAV, "PowerSaver", _PLN, values_head=_V_SAV_PBA)

    # CurrentControlSet: subkeys = Services -> Control (sibling chain)
    b.add_key(_SVC, "Services", _CCS, subkeys_head=_NEO,
              subkeys_sibling=_CTL)
    b.add_key(_CCS, "CurrentControlSet", _ROOT, subkeys_head=_SVC,
              subkeys_sibling=_PWR)  # Power is sibling under root

    # Root: subkeys = CurrentControlSet -> Power
    b.add_key(_ROOT, "SYSTEM", NULL_CELL, subkeys_head=_CCS)

    return b.serialize()


def main():
    parser = argparse.ArgumentParser(description='Generate NeoDOS SYSTEM.HIV')
    parser.add_argument('output', nargs='?', default=Path("scripts/system.hiv"),
                        type=Path, help='Output path for SYSTEM.HIV')
    parser.add_argument('--enable-tests', action='store_true',
                        help='Set EnableTests=1 for boot-time tests')
    parser.add_argument('--enable-network-test', action='store_true', default=False,
                        help='Set EnableNetworkTest=1 for DHCP test')
    args = parser.parse_args()

    output = args.output
    data = build_default_system_hive_v2(
        enable_tests=args.enable_tests,
        enable_network_test=args.enable_network_test,
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(data)
    size = len(data)
    tests_val = 1 if args.enable_tests else 0
    net_test_val = 1 if args.enable_network_test else 0
    print(f"Generated {output} ({size} bytes, NEOHv1)")
    print(f"  Cells: 55")
    print(f"  Root key: SYSTEM")
    print("  Values:")
    print("    CurrentControlSet\\Services\\NeoInit\\DefaultShell = 'C:\\Programs\\NeoShell.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\NeoInit\\EnableVT = 1 (REG_DWORD)")
    print("    CurrentControlSet\\Services\\NeoInit\\AutoStartServices = '' (REG_SZ)")
    print(f"    CurrentControlSet\\Services\\NeoInit\\EnableTests = {tests_val} (REG_DWORD)")
    print(f"    CurrentControlSet\\Services\\NeoInit\\EnableNetworkTest = {net_test_val} (REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\DisplayName = 'DHCP Client' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\BinaryPath = 'C:\\System\\Tools\\dhcpd.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\ImagePath = 'C:\\System\\Tools\\dhcpd.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\StartType = 4 (Disabled, REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\RestartPolicy = 1 (OnCrash, REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\MaxFailures = 3 (REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\Dependencies = '' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\Description = 'DHCP Client Service' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Network\\Interfaces\\0\\DHCPEnabled = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\WaitForNetwork = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\BenchmarkReport = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\AhciDebug = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\Locale\\Language = 'es-ES' (REG_SZ)")
    print("    CurrentControlSet\\Control\\Session Manager\\Environment\\PATH = '\\\\Programs;\\\\System\\\\Tools' (REG_SZ)")
    print("    Power\\ActivePlan = 0 (Balanced, REG_DWORD)")
    print("    Power\\Plans\\Balanced\\DisplayTimeout = 10 (REG_DWORD)")
    print("    Power\\Plans\\Balanced\\SleepTimeout = 30 (REG_DWORD)")
    print("    Power\\Plans\\Balanced\\HibernateEnabled = 1 (REG_DWORD)")
    print("    Power\\Plans\\Balanced\\CpuPolicy = 'Balanced' (REG_SZ)")
    print("    Power\\Plans\\Balanced\\LidAction = 1 (Sleep, REG_DWORD)")
    print("    Power\\Plans\\Balanced\\PowerButtonAction = 3 (Shutdown, REG_DWORD)")
    print("    Power\\Plans\\Performance\\DisplayTimeout = 30 (REG_DWORD)")
    print("    Power\\Plans\\Performance\\SleepTimeout = 60 (REG_DWORD)")
    print("    Power\\Plans\\Performance\\HibernateEnabled = 0 (REG_DWORD)")
    print("    Power\\Plans\\Performance\\CpuPolicy = 'Performance' (REG_SZ)")
    print("    Power\\Plans\\Performance\\LidAction = 0 (Nothing, REG_DWORD)")
    print("    Power\\Plans\\Performance\\PowerButtonAction = 3 (Shutdown, REG_DWORD)")
    print("    Power\\Plans\\PowerSaver\\DisplayTimeout = 3 (REG_DWORD)")
    print("    Power\\Plans\\PowerSaver\\SleepTimeout = 10 (REG_DWORD)")
    print("    Power\\Plans\\PowerSaver\\HibernateEnabled = 1 (REG_DWORD)")
    print("    Power\\Plans\\PowerSaver\\CpuPolicy = 'PowerSave' (REG_SZ)")
    print("    Power\\Plans\\PowerSaver\\LidAction = 1 (Sleep, REG_DWORD)")
    print("    Power\\Plans\\PowerSaver\\PowerButtonAction = 2 (Hibernate, REG_DWORD)")


if __name__ == "__main__":
    main()
