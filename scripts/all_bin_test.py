#!/usr/bin/env python3
"""
all_bin_test.py — Runs all user binaries sequentially via QEMU sendkey,
checks for crashes, and reports pass/fail per binary.
"""

import subprocess
import time
import os
import sys
import socket
import re

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
QEMU_OUTPUT_LOG = os.path.join(PROJECT_ROOT, "qemu_output_allbin.log")
SERIAL_LOG = "/tmp/neodos_serial_allbin.log"

def send_monitor(sock, cmd, wait=0.15):
    try:
        sock.sendall((cmd + "\n").encode())
        time.sleep(wait)
        data = ""
        try:
            sock.settimeout(1)
            while True:
                chunk = sock.recv(4096)
                if not chunk: break
                data += chunk.decode('utf-8', errors='replace')
        except:
            pass
        return data
    except Exception as e:
        return f""

def send_keys(sock, keys, wait_after=0.3):
    time.sleep(0.2)
    for key in keys:
        send_monitor(sock, f"sendkey {key}", 0.1)
    time.sleep(wait_after)

def type_text(sock, text):
    for ch in text:
        k = {".": "dot", "-": "minus", "/": "slash", ":": "shift-semicolon",
             "\\": "backslash", "_": "underscore", "?": "shift-slash",
             "!": "shift-1", "@": "shift-2", "#": "shift-3", "$": "shift-4",
             "%": "shift-5", "^": "shift-6", "&": "shift-7", "*": "shift-8",
             "(": "shift-9", ")": "shift-0", " ": "spc", "|": "shift-backslash",
             "=": "equal", "+": "shift-equal", ",": "comma", "'": "apostrophe"}.get(ch, ch)
        if ch.isupper():
            send_monitor(sock, f"sendkey shift-{ch.lower()}", 0.04)
        else:
            send_monitor(sock, f"sendkey {k}", 0.04)

def type_command(sock, cmd_text):
    for ch in cmd_text:
        type_text(sock, ch)
    send_monitor(sock, "sendkey ret", 0.6)

BINARIES = [
    ("HELP",     "help"),
    ("DIR",      "dir"),
    ("ECHO",     "echo Hello"),
    ("VER",      "ver"),
    ("NEOMEM",   "neomem"),
    ("VOL",      "vol"),
    ("CPUINFO",  "cpuinfo"),
    ("DATETIME", "datetime"),
    ("CLS",      "cls"),
    ("DRIVES",   "drives"),
    ("KOBJ",     "kobj"),
    ("PS",       "ps"),
    ("NDREG",    "ndreg"),
    ("FSCK",     "fsck"),
    ("TREE",     "tree"),
    ("LABEL",    "label"),
    ("MD",       "md TestDir"),
    ("RD",       "rd TestDir"),
    ("TYPE",     "type C:\\readme.txt"),
    ("CD",       "cd Programs"),
    ("CD_BACK",  "cd .."),
    ("PROGRESS", "progress"),
    ("NEOTOP",   "neotop"),
]

def has_crash(text):
    crashes = ["KERNEL PANIC", "GPF:", "BUGCHECK", "STACK_CORRUPTION", "panic", "Panic"]
    return any(c in text for c in crashes)

def run_test():
    print("[*] NeoDOS All-Binary Test Runner")
    print()

    disk_image = os.path.join(PROJECT_ROOT, "disk_image.img")
    ovmf_code = "/usr/share/OVMF/OVMF_CODE.fd"
    ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd"

    for f in [disk_image, ovmf_code, ovmf_vars_template]:
        if not os.path.exists(f):
            print(f"[!] Missing: {f}")
            return 1

    ovmf_vars = f"/tmp/OVMF_VARS_allbin_{os.getpid()}.fd"
    subprocess.run(["cp", ovmf_vars_template, ovmf_vars], check=True)

    accel = os.environ.get("QEMU_ACCEL", "tcg")
    print(f"[+] QEMU accelerator: {accel}")

    cmd = [
        "qemu-system-x86_64",
        "-machine", f"q35,accel={accel}",
        "-monitor", "telnet:127.0.0.1:4447,server,nowait",
        "-display", "none",
        "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf_code}",
        "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
        "-device", "ahci,id=ahci",
        "-drive", f"if=none,format=raw,file={disk_image},id=mydisk",
        "-device", "ide-hd,drive=mydisk,bus=ahci.0",
        "-m", "512M",
        "-serial", f"file:{SERIAL_LOG}",
    ]

    timeout = 240
    start_time = time.time()

    try:
        os.unlink(SERIAL_LOG)
    except:
        pass

    try:
        print("[+] Launching QEMU...")
        sys.stdout.flush()

        proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        time.sleep(4)

        monitor_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        monitor_sock.settimeout(5)
        try:
            monitor_sock.connect(('127.0.0.1', 4447))
            send_monitor(monitor_sock, "")
            print("[+] Monitor connected")
        except Exception as e:
            print(f"[!] Monitor failed: {e}")
            monitor_sock = None
            proc.terminate()
            return 1

        # Wait for shell prompt
        print("[+] Waiting for shell prompt...")
        shell_ready = False
        last_len = 0
        while time.time() - start_time < timeout:
            if proc.poll() is not None:
                print("[!] QEMU exited early")
                return 1
            try:
                if os.path.exists(SERIAL_LOG):
                    with open(SERIAL_LOG, 'rb') as f:
                        f.seek(last_len)
                        data = f.read().decode('utf-8', errors='replace')
                        if data:
                            last_len += len(data.encode())
                            clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', data)
                            if "C:\\>" in clean or re.search(r'C:\\.*>', clean):
                                shell_ready = True
                                break
            except:
                pass
            time.sleep(0.5)

        if not shell_ready:
            print("[!] Shell not detected within timeout")
            proc.terminate()
            return 1

        print(f"[+] Shell ready at t={time.time()-start_time:.1f}s")
        time.sleep(1)

        passed = 0
        failed = 0
        results = []

        for name, cmd_text in BINARIES:
            if proc.poll() is not None:
                print("[!] QEMU died mid-test")
                break

            # Read serial to get baseline length
            try:
                with open(SERIAL_LOG, 'rb') as f:
                    f.seek(last_len)
            except:
                pass

            print(f"  [{name}] ", end="", flush=True)
            type_command(monitor_sock, cmd_text)
            time.sleep(1.0)

            # Read serial output since last command
            cmd_output = ""
            try:
                with open(SERIAL_LOG, 'rb') as f:
                    f.seek(last_len)
                    new_data = f.read().decode('utf-8', errors='replace')
                    last_len += len(new_data.encode())
                    cmd_output = new_data
            except:
                pass

            clean_out = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', cmd_output)

            if has_crash(clean_out):
                print("CRASH")
                failed += 1
                results.append((name, "CRASH"))
                break
            elif "ENOENT" in clean_out or "Path not found" in clean_out:
                print("PATH_ERR")
                failed += 1
                results.append((name, "PATH_ERR"))
            elif "Bad command" in clean_out:
                print("NOT_FOUND")
                failed += 1
                results.append((name, "NOT_FOUND"))
            else:
                # Print first line of output for verification
                lines = [l.strip() for l in clean_out.split('\n') if l.strip() and not l.strip().startswith('C:\\>') and 'OB ' not in l and 'SCHED' not in l and 'EXIT' not in l and 'NXL' not in l]
                preview = lines[0][:60] if lines else "(empty)"
                print(f"OK [{preview}]")
                passed += 1
                results.append((name, "OK"))

            if has_crash(clean_out):
                break

        # Cleanup
        if monitor_sock:
            monitor_sock.close()
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except:
                proc.kill()

        # Report
        print()
        print("=" * 60)
        print("RESULTS")
        print("=" * 60)
        for name, status in results:
            print(f"  {name:15s} {status}")
        print(f"  {'---':15s} ----")
        print(f"  {'PASSED':15s} {passed}")
        print(f"  {'FAILED':15s} {failed}")

        # Save full log
        try:
            with open(SERIAL_LOG, 'rb') as f:
                full = f.read()
            with open(QEMU_OUTPUT_LOG, 'wb') as f:
                f.write(full)
            print(f"[*] Full log: {QEMU_OUTPUT_LOG}")
        except:
            pass

        return 0 if failed == 0 else 1

    except KeyboardInterrupt:
        print("\n[*] Interrupted")
        if 'proc' in locals() and proc.poll() is None:
            proc.terminate()
        return 1
    finally:
        try:
            os.unlink(ovmf_vars)
        except:
            pass

if __name__ == "__main__":
    sys.exit(run_test())
