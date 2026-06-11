#!/usr/bin/env python3
"""
NeoDOS Crash Dump Analyzer
Parses binary crash dumps saved from the 16 KB header at 0x0F000000.
Reads from a raw binary file or from stdin.

Usage:
    python3 scripts/crash_analyzer.py < dump.bin
    python3 scripts/crash_analyzer.py --file dump.bin
    python3 scripts/crash_analyzer.py --file dump.bin --symbols kernel.elf

The dump.bin file should contain at least the 16 KB header written by
the kernel's crash dump facility.
"""

import struct
import sys
import os

CRASH_DUMP_MAGIC = b'NDMP'
CRASH_DUMP_HEADER_SIZE = 0x4000  # 16 KB

CAUSE_NAMES = {
    0: "PANIC",
    1: "DOUBLE_FAULT",
    2: "TRIPLE_FAULT",
    3: "NMI",
    4: "WATCHDOG",
}

TRACE_EVENT_NAMES = {
    0x01: "ContextSwitch",
    0x02: "SyscallEnter",
    0x03: "SyscallExit",
    0x04: "IrqEnter",
    0x05: "IrqExit",
    0x06: "SchedDecision",
    0x07: "IrqTimerTick",
    0xFF: "Panic",
}


class CrashDumpHeader:
    FORMAT = '<4s I Q II 4Q'  # first 56 bytes
    FORMAT += 'I I 32Q'       # stack_trace: depth + pad + 32 × Q
    FORMAT += '17Q'           # GPRs: rax..r15, rsp, rip, rflags
    FORMAT += '5Q'            # CR0-4, EFER
    FORMAT += 'I I B B 6s'    # tid, pid, state, cpu_id, pad1
    FORMAT += '512Q'          # PML4
    FORMAT += 'I 28s'         # trace_count + pad2
    FORMAT += '128s'          # trace events (128 × 24 = 3072)
    FORMAT = '<' + FORMAT[1:]

    def __init__(self, data):
        if len(data) < CRASH_DUMP_HEADER_SIZE:
            raise ValueError(f"Data too small: {len(data)} < {CRASH_DUMP_HEADER_SIZE}")

        magic = data[0:4]
        if magic != CRASH_DUMP_MAGIC:
            raise ValueError(f"Bad magic: {magic} != {CRASH_DUMP_MAGIC}")

        off = 0
        self.magic, self.version, self.timestamp, self.cause, self.cpu_count = \
            struct.unpack_from('<4s I Q I I', data, off)
        off += 4 + 4 + 8 + 4 + 4

        self.param = list(struct.unpack_from('<4Q', data, off))
        off += 32

        self.stack_depth, pad0 = struct.unpack_from('<I I', data, off)
        off += 8
        self.stack_trace = list(struct.unpack_from('<32Q', data, off))
        off += 256

        # GPRs
        self.gprs = {}
        regs = ['rax', 'rbx', 'rcx', 'rdx', 'rsi', 'rdi',
                'r8', 'r9', 'r10', 'r11', 'r12', 'r13', 'r14', 'r15',
                'rsp', 'rip', 'rflags']
        vals = struct.unpack_from('<17Q', data, off)
        off += 136
        for r, v in zip(regs, vals):
            self.gprs[r] = v

        self.cr = {}
        cr_names = ['cr0', 'cr2', 'cr3', 'cr4', 'efer']
        cr_vals = struct.unpack_from('<5Q', data, off)
        off += 40
        for r, v in zip(cr_names, cr_vals):
            self.cr[r] = v

        tid, pid, self.thread_state, self.cpu_id = \
            struct.unpack_from('<I I B B', data, off)
        off += 12
        self.current_tid = tid
        self.current_pid = pid
        # Skip pad1 (6 bytes)
        off += 6

        # PML4
        self.pml4 = list(struct.unpack_from('<512Q', data, off))
        off += 4096

        self.trace_count, = struct.unpack_from('<I', data, off)
        off += 32  # +28 for pad2

        # Trace events: 128 × 24-byte entries
        self.trace_events = []
        trace_data = data[off:off + 3072]
        for i in range(128):
            te_off = i * 24
            if te_off + 24 > len(trace_data):
                break
            ts, event, cpu, pad2, arg0, arg1, rip_low = \
                struct.unpack_from('<Q B B HH I I I', trace_data, te_off)
            self.trace_events.append({
                'timestamp': ts,
                'event': event,
                'cpu': cpu,
                'arg0': arg0,
                'arg1': arg1,
                'rip_low': rip_low,
            })

    def cause_name(self):
        return CAUSE_NAMES.get(self.cause, f"UNKNOWN({self.cause})")

    def trace_event_name(self, code):
        return TRACE_EVENT_NAMES.get(code, f"EVENT_{code:#x}")

    def print_summary(self):
        print("=" * 60)
        print("    N e o D O S   C r a s h   D u m p   A n a l y z e r")
        print("=" * 60)
        print(f"  Magic:          {self.magic.decode('ascii', errors='replace')}")
        print(f"  Version:        {self.version}")
        print(f"  Timestamp:      {self.timestamp} ticks")
        print(f"  Cause:          {self.cause_name()} ({self.cause})")
        print(f"  CPU count:      {self.cpu_count}")
        print(f"  Param:          {', '.join(f'{p:#x}' for p in self.param)}")
        print()

        print("  --- Registers ---")
        for r in ['rax', 'rbx', 'rcx', 'rdx', 'rsi', 'rdi']:
            print(f"    {r.upper():4s} = {self.gprs[r]:#018x}")
        for r in ['r8', 'r9', 'r10', 'r11', 'r12', 'r13', 'r14', 'r15']:
            print(f"    {r.upper():4s} = {self.gprs[r]:#018x}")
        print(f"    RSP  = {self.gprs['rsp']:#018x}")
        print(f"    RIP  = {self.gprs['rip']:#018x}")
        print(f"    RFL  = {self.gprs['rflags']:#018x}")

        print()
        print("  --- Control Registers ---")
        for r in ['cr0', 'cr2', 'cr3', 'cr4', 'efer']:
            print(f"    {r.upper():4s} = {self.cr[r]:#018x}")

        print()
        print("  --- Scheduler ---")
        print(f"    TID  = {self.current_tid}")
        print(f"    PID  = {self.current_pid}")
        print(f"    CPU  = {self.cpu_id}")
        print(f"    State = {self.thread_state}")

        print()
        print("  --- Stack Trace ({:2} frames) ---".format(self.stack_depth))
        for i in range(min(self.stack_depth, 32)):
            print(f"    [{i:2d}] {self.stack_trace[i]:#018x}")

        print()
        print("  --- PML4 (first 16 entries) ---")
        for i in range(16):
            entry = self.pml4[i]
            if entry != 0:
                addr = entry & ~0xFFF
                flags = entry & 0xFFF
                flag_str = ""
                if flags & 0x001:
                    flag_str += "P"
                if flags & 0x002:
                    flag_str += "W"
                if flags & 0x004:
                    flag_str += "U"
                if flags & 0x080:
                    flag_str += "G"
                if flags & 0x100:
                    flag_str += "NX" if (flags & (1 << 63)) else "XD"
                print(f"    [{i:2d}] addr={addr:#010x} flags={flags:#06x} ({flag_str})")

        print()
        print("  --- Trace Buffer ({:3} events) ---".format(len(self.trace_events)))
        count = 0
        for ev in self.trace_events:
            if ev['timestamp'] != 0:
                ev_name = self.trace_event_name(ev['event'])
                print(f"    [{ev['timestamp']:6d}] {ev_name:15s} cpu={ev['cpu']} "
                      f"a0={ev['arg0']:#010x} a1={ev['arg1']:#010x} "
                      f"rip_low={ev['rip_low']:#010x}")
                count += 1
                if count >= 32:
                    print("    ... (truncated to 32 events)")
                    break

        print("=" * 60)


def resolve_symbols(binary_path, addresses):
    """Optionally resolve addresses to symbols using 'nm' or 'objdump'."""
    if not binary_path or not os.path.exists(binary_path):
        return {}
    try:
        if binary_path.endswith('.elf'):
            cmd = f"nm -n {binary_path} 2>/dev/null"
        else:
            return {}
        result = os.popen(cmd).read()
        sym_map = {}
        for line in result.strip().split('\n'):
            parts = line.split()
            if len(parts) >= 3:
                try:
                    addr = int(parts[0], 16)
                    sym_type = parts[1]
                    name = parts[2]
                    if sym_type in ('T', 't', 'W', 'w'):  # text/code symbols
                        sym_map[addr] = name
                except ValueError:
                    pass
        # Find nearest symbol for each address
        resolved = {}
        sorted_addrs = sorted(sym_map.keys())
        for addr in addresses:
            if addr == 0:
                continue
            best = None
            for sa in sorted_addrs:
                if sa <= addr:
                    best = sa
                else:
                    break
            if best is not None:
                offset = addr - best
                resolved[addr] = f"{sym_map[best]}+{offset:#x}"
            else:
                resolved[addr] = f"{addr:#x}"
        return resolved
    except Exception:
        return {}


def main():
    import argparse
    parser = argparse.ArgumentParser(description="NeoDOS Crash Dump Analyzer")
    parser.add_argument('--file', '-f', type=str, help='Binary dump file')
    parser.add_argument('--symbols', '-s', type=str, help='kernel.elf for symbol resolution')
    args = parser.parse_args()

    data = None
    if args.file:
        with open(args.file, 'rb') as f:
            data = f.read()
    else:
        data = sys.stdin.buffer.read()

    if not data or len(data) < CRASH_DUMP_HEADER_SIZE:
        print(f"Error: need at least {CRASH_DUMP_HEADER_SIZE} bytes, got {len(data) if data else 0}")
        sys.exit(1)

    try:
        header = CrashDumpHeader(data[:CRASH_DUMP_HEADER_SIZE])
    except ValueError as e:
        print(f"Error parsing header: {e}")
        sys.exit(1)

    header.print_summary()

    # Symbol resolution
    if args.symbols:
        all_addrs = [header.gprs['rip']] + header.stack_trace[:header.stack_depth]
        syms = resolve_symbols(args.symbols, all_addrs)
        if syms:
            print()
            print("  --- Symbol Resolution ---")
            if header.gprs['rip'] in syms:
                print(f"    RIP: {syms[header.gprs['rip']]}")
            print("    Stack:")
            for i in range(min(header.stack_depth, 32)):
                addr = header.stack_trace[i]
                if addr in syms:
                    print(f"      [{i:2d}] {syms[addr]}")


if __name__ == '__main__':
    main()
