#!/usr/bin/env python3
"""NeoDOS comprehensive command test — runs all commands in one QEMU session."""

import subprocess, os, sys, time

WORKDIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
QEMU = "qemu-system-x86_64"
BIOS = "/usr/share/ovmf/OVMF.fd"
DISK = os.path.join(WORKDIR, "disk_image.img")
ACCEL = os.environ.get("QEMU_ACCEL", "tcg")

COMMANDS = """
VER
MEM
CPUINFO
DATETIME
DRIVES
VOL
ECHO Hello_NeoDOS
MD C:\\Temp\\testdir
DIR C:\\Temp
COPY C:\\System\\Config\\system.cfg C:\\Temp\\testcfg.txt
TYPE C:\\Temp\\testcfg.txt
REN C:\\Temp\\testcfg.txt C:\\Temp\\testcfg_renamed.txt
DEL C:\\Temp\\testcfg_renamed.txt
RD C:\\Temp\\testdir
HELP
HELP CLS
TREE C:\\System
PS
LABEL
NDREG QUERY
PRI 3 2
KOBJ
CD C:\\
DIR
ECHO COMMAND_TEST_COMPLETE
"""

def run_test(timeout=600):
    qemu_args = [
        QEMU, "-bios", BIOS, "-m", "256M",
        "-drive", f"file={DISK},format=raw,if=none,id=hd",
        "-device", "ahci,id=ahci", "-device", "ide-hd,drive=hd,bus=ahci.0",
        "-serial", "stdio", "-nographic",
        "-no-reboot", "-accel", ACCEL,
        "-machine", "q35",
    ]
    proc = subprocess.Popen(
        qemu_args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, cwd=WORKDIR,
    )
    # Wait for boot, then send all commands
    time.sleep(6)
    inp = b""
    for line in COMMANDS.strip().split("\n"):
        inp += line.strip().encode("latin-1") + b"\r\n"
    inp += b"EXIT\r\n"
    try:
        stdout, stderr = proc.communicate(input=inp, timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        stdout, stderr = proc.communicate()
    text = (stdout + stderr).decode("latin-1", errors="replace")

    print(f"[*] Output length: {len(text)} chars")

    # Check for crashes
    for cp in ["PANIC", "BUGCHECK", "TRIPLE FAULT", "stack overflow"]:
        if cp in text.upper():
            idx = text.upper().find(cp)
            s = max(0, idx - 100)
            e = min(len(text), idx + 300)
            print(f"[!] CRASH: {cp} at pos {idx}")
            print(text[s:e])
            return False

    checks = [
        ("NeoDOS Kernel", "NeoDOS Kernel" in text or "v0." in text),
        ("VER", "NeoDOS" in text or "v0." in text),
        ("MEM", "KB" in text or "MB" in text or "bytes" in text),
        ("CPUINFO", "MHz" in text or "QEMU" in text or "vendor" in text or "cpu" in text.lower()),
        ("DRIVES", "C:" in text),
        ("VOL", "Volume" in text or "label" in text or "NeoDOS" in text),
        ("ECHO", "Hello_NeoDOS" in text),
        ("MD+DIR", "testdir" in text),
        ("COPY", "testcfg.txt" in text),
        ("TYPE", "System" in text or "Config" in text),
        ("REN", "testcfg_renamed" in text),
        ("HELP", "cmd" in text.lower() or "command" in text.lower()),
        ("PS", "PID" in text or "TID" in text),
        ("NDREG", "driver" in text.lower()),
        ("KOBJ", "Process" in text or "Driver" in text),
        ("CMDTEST", "passed" in text and "failed" in text),
        ("COMPLETE", "COMMAND_TEST_COMPLETE" in text),
    ]

    passed = sum(1 for _, ok in checks if ok)
    failed = sum(1 for _, ok in checks if not ok)
    for label, ok in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {label}")

    print(f"\n[*] Results: {passed}/{passed+failed} passed")
    return failed == 0

if __name__ == "__main__":
    print("=" * 60)
    print(" NeoDOS Comprehensive Command Test")
    print("=" * 60)
    success = run_test()
    print("=" * 60)
    sys.exit(0 if success else 1)
