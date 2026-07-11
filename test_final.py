#!/usr/bin/env python3
import subprocess, time, os, socket, sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DISK_IMAGE = os.path.join(SCRIPT_DIR, "disk_image.img")
OVMF_CODE = "/usr/share/OVMF/OVMF_CODE.fd"
OVMF_VARS_TMPL = "/usr/share/OVMF/OVMF_VARS.fd"
OVMF_VARS = f"/tmp/ovmf_test_{os.getpid()}.fd"
subprocess.run(["cp", OVMF_VARS_TMPL, OVMF_VARS], check=True)

serial_log = "/tmp/qemu_ser.log"
try: os.unlink(serial_log)
except: pass

proc = subprocess.Popen([
    "qemu-system-x86_64",
    "-machine", "q35,accel=tcg",
    "-monitor", "telnet:127.0.0.1:4451,server,nowait",
    "-display", "none",
    "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF_CODE}",
    "-drive", f"if=pflash,format=raw,file={OVMF_VARS}",
    "-device", "ahci,id=ahci",
    "-drive", f"if=none,format=raw,file={DISK_IMAGE},id=mydisk",
    "-device", "ide-hd,drive=mydisk,bus=ahci.0",
    "-netdev", "user,id=net0",
    "-device", "e1000,netdev=net0",
    "-m", "512M",
    "-serial", f"file:{serial_log}",
], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def mon_connect(host, port, retries=30):
    for i in range(retries):
        try:
            s = socket.socket()
            s.settimeout(3)
            s.connect((host, port))
            # Telnet negotiation - eat welcome
            time.sleep(0.5)
            s.sendall(b"\n")
            time.sleep(0.3)
            try:
                while True:
                    d = s.recv(4096)
                    if not d: break
            except: pass
            return s
        except:
            time.sleep(1)
    return None

def send_key(sock, key):
    sock.sendall(f"sendkey {key}\n".encode())
    time.sleep(0.08)

def send_text(sock, text):
    keymap = {
        ' ': 'spc', '>': 'shift-.', '<': 'shift-,', '|': 'shift-\\\\',
        '/': 'slash', '.': 'dot', '\\\\': 'backslash', ':': 'shift-;',
        '-': 'minus', '_': 'shift-minus', '!': 'shift-1', '@': 'shift-2',
        '#': 'shift-3', '$': 'shift-4', '%': 'shift-5', '^': 'shift-6',
        '&': 'shift-7', '*': 'shift-8', '(': 'shift-9', ')': 'shift-0',
        '\'': 'apostrophe', '"': 'shift-apostrophe',
    }
    for ch in text:
        if ch in keymap:
            send_key(sock, keymap[ch])
        elif 'a' <= ch <= 'z':
            send_key(sock, ch)
        elif 'A' <= ch <= 'Z':
            send_key(sock, f"shift-{ch.lower()}")
        elif ch.isdigit():
            send_key(sock, ch)
        else:
            send_key(sock, ch)

def send_line(sock, text):
    send_text(sock, text)
    send_key(sock, "ret")
    time.sleep(2)

# Wait for shell
print("[*] Waiting for shell...", end="", flush=True)
for i in range(180):
    time.sleep(1)
    if os.path.exists(serial_log):
        with open(serial_log, errors='replace') as f:
            if "Type HELP" in f.read():
                print(f" {i}s")
                break
else:
    print(" TIMEOUT")
    proc.kill()
    sys.exit(1)

time.sleep(2)

mon = mon_connect("127.0.0.1", 4451)
if not mon:
    print("[!] Could not connect to monitor")
    proc.kill()
    sys.exit(1)
print("[+] Monitor connected")

# === TEST 1: dir > hello.txt ===
print("[TEST] dir > hello.txt")
send_line(mon, "dir > hello.txt")
time.sleep(3)

with open(serial_log, errors='replace') as f:
    out = f.read()
    if "Bad command" in out:
        print("  FAIL: Bad command")
    else:
        print("  OK")

# === TEST 2: type hello.txt ===
print("[TEST] type hello.txt")
send_line(mon, "type hello.txt")
time.sleep(3)

with open(serial_log, errors='replace') as f:
    out = f.read()

# === TEST 3: dir > hello.txt | type hello.txt ===
print("[TEST] dir > hello.txt | type hello.txt")
send_line(mon, "dir > hello.txt | type hello.txt")
time.sleep(3)

with open(serial_log, errors='replace') as f:
    out = f.read()
    if "Pipe error" in out:
        print("  FAIL: Pipe error")
    elif "Bad command" in out:
        print("  FAIL: Bad command")
    else:
        print("  OK (no errors)")

# Show relevant output
print("\n=== Shell output ===")
with open(serial_log, errors='replace') as f:
    for line in f:
        line = line.strip().rstrip('\r')
        if any(x in line for x in ['C:\\>', 'Bad command', 'Pipe error', 'SYSTEM', 'Programs',
                                     'volume', 'kernel.elf', 'hello.txt', 'Directory']):
            print(f"  {line}")

mon.close()
proc.terminate()
time.sleep(1)
proc.kill()
os.unlink(OVMF_VARS)
print("\n=== Done ===")
