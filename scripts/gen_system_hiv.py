#!/usr/bin/env python3
"""
gen_system_hiv.py — Generate NeoDOS SYSTEM.HIV with default values.

Produces a NEOHv1 binary hive file matching the kernel's cell-based format
(src/cm/hive.rs). Embeds default values identical to kernel's
cm_ensure_default_values() in src/cm/mod.rs.

Usage:
  python3 scripts/gen_system_hiv.py [output_path]

  Default output: scripts/system.hiv (copied to neodos_image.img later)
"""

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
        """Serialize all cells into NEOH binary format."""
        entries = sorted(self.cells.items())
        raw_payload = b"".join(data for _, data in entries)

        # Checksum: wrapping_add of all cell idx + type + payload words
        checksum = 0
        for _, data in entries:
            for b in data:
                checksum = (checksum + b) & 0xFFFFFFFF

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


def build_default_system_hive_v2() -> bytes:
    """
    Build SYSTEM.HIV with proper tree structure matching kernel's
    ensure_key_path() traversal. Includes default service entries
    (Dhcpd) for the Service Manager (SM-001).
    """
    b = HiveBuilder()

    # Total cells needed: root + CurrentControlSet + Services + NeoInit +
    #   Network + Interfaces + 0 + Control +
    #   Dhcpc + DhcpcDisplayName + DhcpcBinaryPath + DhcpcImagePath +
    #   DhcpcStartType + DhcpcRestartPolicy + DhcpcMaxFailures + DhcpcDeps +
    #   DefaultShell + EnableVT + AutoStartServices +
    #   DHCPEnabled + WaitForNetwork
    # = 22 cells

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

    # Pre-alloc: ensure next_idx doesn't conflict
    b.next_idx = 27

    # ── VALUES (linked lists) ──
    # NeoInit values: DefaultShell -> EnableVT -> AutoStartServices -> EnableTests
    b.add_value(_V_SHELL, "DefaultShell", REG_SZ,
                b"C:\\Programs\\neoshell.nxe\x00")
    b.add_value(_V_VT, "EnableVT", REG_DWORD,
                struct.pack("<I", 1), next_val=_V_SHELL)
    b.add_value(_V_AUTO, "AutoStartServices", REG_SZ,
                b"\x00", next_val=_V_VT)
    b.add_value(_V_TESTS, "EnableTests", REG_DWORD,
                struct.pack("<I", 0), next_val=_V_AUTO)

    # Dhcpc values: Description -> Dependencies -> MaxFailures -> RestartPolicy -> StartType -> ImagePath -> BinaryPath -> DisplayName
    b.add_value(_V_DESC, "Description", REG_SZ,
                b"DHCP Client Service\x00")
    b.add_value(_V_DEPS, "Dependencies", REG_SZ,
                b"\x00", next_val=_V_DESC)
    b.add_value(_V_MFAIL, "MaxFailures", REG_DWORD,
                struct.pack("<I", 3), next_val=_V_DEPS)
    b.add_value(_V_RPOL, "RestartPolicy", REG_DWORD,
                struct.pack("<I", 1), next_val=_V_MFAIL)  # OnCrash
    b.add_value(_V_STYPE, "StartType", REG_DWORD,
                struct.pack("<I", 2), next_val=_V_RPOL)  # Auto
    b.add_value(_V_IPATH, "ImagePath", REG_SZ,
                b"C:\\System\\Tools\\dhcpd.nxe\x00", next_val=_V_STYPE)
    b.add_value(_V_BPATH, "BinaryPath", REG_SZ,
                b"C:\\System\\Tools\\dhcpd.nxe\x00", next_val=_V_IPATH)
    b.add_value(_V_DNAME, "DisplayName", REG_SZ,
                b"DHCP Client\x00", next_val=_V_BPATH)

    # Interfaces\0 values: DHCPEnabled
    b.add_value(_V_DHCP, "DHCPEnabled", REG_DWORD,
                struct.pack("<I", 1))

    # Control values: BenchmarkReport(1) -> AhciDebug(1) -> WaitForNetwork(0)
    b.add_value(_V_BENCH, "BenchmarkReport", REG_DWORD,
                struct.pack("<I", 0))
    b.add_value(_V_AHCI, "AhciDebug", REG_DWORD,
                struct.pack("<I", 0), next_val=_V_BENCH)
    b.add_value(_V_WAIT, "WaitForNetwork", REG_DWORD,
                struct.pack("<I", 0), next_val=_V_AHCI)

    # Locale\Language value
    b.add_value(_V_LANG, "Language", REG_SZ, b"es-ES")

    # ── KEYS (bottom-up) ──
    # NeoInit (child of Services)
    b.add_key(_NEO, "NeoInit", _SVC, values_head=_V_TESTS)

    # Dhcpc (child of Services, sibling of NeoInit)
    b.add_key(_DHCPC, "Dhcpc", _SVC, values_head=_V_DNAME)

    # Interfaces\0 (child of Interfaces)
    b.add_key(_IF0, "0", _IFC, values_head=_V_DHCP)

    # Interfaces (child of Network)
    b.add_key(_IFC, "Interfaces", _NET, subkeys_head=_IF0)

    # Network (child of Services, sibling of Dhcpc)
    b.add_key(_NET, "Network", _SVC, subkeys_head=_IFC)

    # Services: subkeys = NeoInit -> Dhcpc -> Network (sibling chain)
    b.add_key(_NEO, "NeoInit", _SVC, values_head=_V_TESTS,
              subkeys_sibling=_DHCPC)
    b.add_key(_DHCPC, "Dhcpc", _SVC, values_head=_V_DNAME,
              subkeys_sibling=_NET)
    b.add_key(_SVC, "Services", _CCS, subkeys_head=_NEO)

    # Locale (child of Control)
    b.add_key(_LOC_KEY, "Locale", _CTL, values_head=_V_LANG)

    # Control (child of CurrentControlSet, sibling of Services)
    b.add_key(_CTL, "Control", _CCS, values_head=_V_WAIT, subkeys_head=_LOC_KEY)

    # CurrentControlSet: subkeys = Services -> Control (sibling chain)
    b.add_key(_SVC, "Services", _CCS, subkeys_head=_NEO,
              subkeys_sibling=_CTL)
    b.add_key(_CCS, "CurrentControlSet", _ROOT, subkeys_head=_SVC)

    # Root: subkeys = CurrentControlSet
    b.add_key(_ROOT, "SYSTEM", NULL_CELL, subkeys_head=_CCS)

    return b.serialize()


def main():
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("scripts/system.hiv")
    data = build_default_system_hive_v2()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(data)
    size = len(data)
    print(f"Generated {output} ({size} bytes, NEOHv1)")
    print(f"  Cells: {27}")
    print(f"  Root key: SYSTEM")
    print("  Values:")
    print("    CurrentControlSet\\Services\\NeoInit\\DefaultShell = 'C:\\Programs\\NeoShell.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\NeoInit\\EnableVT = 1 (REG_DWORD)")
    print("    CurrentControlSet\\Services\\NeoInit\\AutoStartServices = '' (REG_SZ)")
    print("    CurrentControlSet\\Services\\NeoInit\\EnableTests = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\DisplayName = 'DHCP Client' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\BinaryPath = 'C:\\System\\Tools\\dhcpd.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\ImagePath = 'C:\\System\\Tools\\dhcpd.nxe' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\StartType = 2 (Auto, REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\RestartPolicy = 1 (OnCrash, REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\MaxFailures = 3 (REG_DWORD)")
    print("    CurrentControlSet\\Services\\Dhcpc\\Dependencies = '' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Dhcpc\\Description = 'DHCP Client Service' (REG_SZ)")
    print("    CurrentControlSet\\Services\\Network\\Interfaces\\0\\DHCPEnabled = 1 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\WaitForNetwork = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\BenchmarkReport = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\AhciDebug = 0 (REG_DWORD)")
    print("    CurrentControlSet\\Control\\Locale\\Language = 'es-ES' (REG_SZ)")


if __name__ == "__main__":
    main()
