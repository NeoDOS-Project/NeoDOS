#!/usr/bin/env python3
"""Quick test for shell redirection: dir > hello.txt | type hello.txt"""
import subprocess, time, os, socket, sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DISK_IMAGE = os.path.join(SCRIPT_DIR, "disk_image.img")
OVMF_CODE = "/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TMPL = "/usr/share/OVMF/OVMF_VARS.fd"
OVMF_VARS = f"/tmp/OVMF_VARS_test_{os.getpid()}.fd"

assert all(os.path.exists(f) for f in [DISK_IMAGE, OVMF_CODE, OVMF_VARS_TMPL])
subprocess.run(["cp", OVMF_VARS_TMPL, OVMF_VARS], check=True)

serial_log = "/tmp/neodos_serial_test.log"
try: os.unlink(serial_log)
except: pass

def send_monitor(sock, cmd, wait=0.2):
    try:
        sock.sendall((cmd + "\n").encode())
        time.sleep(wait)
        data = ""
        try:
            sock.settimeout(2)
            while True:
                chunk = sock.recv(4096)
                if not chunk: break
                data += chunk.decode('utf-8', errors='replace')
        except: pass
        return data
    except Exception as e: return f"[error: {e}]"

def send_type(sock, text):
    # Map characters to QEMU key names
    keymap = {
        ' ': 'spc', '>': 'shift-.', '<': 'shift-,', '|': 'shift-\\',
        '/': 'slash', '.': 'dot', '\\': 'backslash', ':': 'shift-;',
    }
    time.sleep(0.5)
    for ch in text:
        if ch in keymap:
            send_monitor(sock, f"sendkey {keymap[ch]}", 0.08)
        elif 'a' <= ch <= 'z':
            send_monitor(sock, f"sendkey {ch}", 0.08)
        elif 'A' <= ch <= 'Z':
            send_monitor(sock, f"sendkey shift-{ch.lower()}", 0.08)
        elif ch.isdigit():
            send_monitor(sock, f"sendkey {ch}", 0.08)
    send_monitor(sock, "sendkey ret", 0.5)
    time.sleep(2)

proc = subprocess.Popen([
    "qemu-system-x86_64",
    "-machine", "q35,accel=kvm",
    "-monitor", "telnet:127.0.0.1:4447,server,nowait",
    "-display", "none",
    "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF_CODE}",
    "-drive", f"if=pflash,format=raw,file={OVMF_VARS}",
    "-device", "ahci,id=ahci",
    "-drive", f"if=none,format=raw,file={DISK_IMAGE},id=mydisk",
    "-device", "ide-hd,drive=mydisk,bus=ahci.0",
    "-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1",
    "-device", "e1000,netdev=net0",
    "-m", "512M",
    "-serial", f"file:{serial_log}",
], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

time.sleep(3)
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(5)
try:
    sock.connect(('127.0.0.1', 4447))
    print("[+] Monitor connected")
except:
    print("[!] Monitor connection failed")
    proc.kill()
    sys.exit(1)

# Wait for shell
print("[*] Waiting for shell...", end="", flush=True)
for _ in range(120):
    if os.path.exists(serial_log):
        with open(serial_log) as f:
            if "Type HELP" in f.read():
                print(" OK")
                break
    time.sleep(1)
else:
    print(" TIMEOUT")
    proc.kill()
    sys.exit(1)

time.sleep(2)

# Test 1: dir > hello.txt
print("[TEST] dir > hello.txt")
send_type(sock, "dir > hello.txt")

# Read serial output
time.sleep(1)
with open(serial_log) as f:
    out = f.read()
    if "Bad command" not in out:
        print("  OK - command ran")
    else:
        print("  FAIL - Bad command")

# Test 2: type hello.txt  
print("[TEST] type hello.txt")
send_type(sock, "type hello.txt")

time.sleep(1)
with open(serial_log) as f:
    out = f.read()
    # Should show directory listing
    if "SYSTEM" in out or "Programs" in out or "kernel.elf" in out.lower():
        print("  PASS - file has content!")
    else:
        print("  WARN - file may be empty or type shows nothing")

# Test 3: dir > hello.txt | type hello.txt (pipe + redirect)
print("[TEST] dir > hello.txt | type hello.txt")
send_type(sock, "dir > hello.txt | type hello.txt")

time.sleep(1)
with open(serial_log) as f:
    out = f.read()
    if "Pipe error" in out:
        print("  FAIL - Pipe error (not fixed yet)")
    elif "SYSTEM" in out or "Programs" in out:
        print("  PASS - pipeline + redirect works!")
    else:
        print("  RESULT:", [l for l in out.split('\n') if l.strip()][-5:])

# Show last 20 lines of serial output
print("\n=== Last 30 lines of serial ===")
with open(serial_log) as f:
    lines = f.readlines()
    for l in lines[-30:]:
        print(l.rstrip())

# Cleanup
sock.close()
proc.terminate()
time.sleep(1)
proc.kill()
os.unlink(OVMF_VARS)
print("\n=== Done ===")
