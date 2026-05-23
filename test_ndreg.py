#!/usr/bin/env python3
import subprocess
import time
import os
import socket
import re

print("[*] Testing NDREG LOAD...")

ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd"
ovmf_vars = "/tmp/OVMF_VARS_test.fd"
subprocess.run(["cp", ovmf_vars_template, ovmf_vars], check=True)

cmd = [
    "qemu-system-x86_64",
    "-machine", "q35,accel=tcg",
    "-display", "none",
    "-drive", "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd",
    "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
    "-drive", "format=raw,file=disk_image.img,index=0,media=disk",
    "-m", "512M",
    "-monitor", "telnet:127.0.0.1:4447,server,nowait",
    "-serial", "file:/tmp/test_ndreg_serial.log"
]

try:
    os.unlink("/tmp/test_ndreg_serial.log")
except:
    pass

proc = subprocess.Popen(cmd)
time.sleep(3)

def send_monitor(sock, cmd, wait=0.2):
    try:
        sock.sendall((cmd + "\n").encode())
        time.sleep(wait)
    except:
        pass

def send_keys(sock, keys):
    for key in keys:
        send_monitor(sock, f"sendkey {key}", 0.15)
    time.sleep(0.5)

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(5)
try:
    sock.connect(('127.0.0.1', 4447))
except:
    print("Failed to connect to monitor")
    proc.terminate()
    exit(1)

last_pos = 0
found_shell = False
start_time = time.time()
while time.time() - start_time < 30:
    if os.path.exists("/tmp/test_ndreg_serial.log"):
        with open("/tmp/test_ndreg_serial.log", 'rb') as f:
            f.seek(last_pos)
            data = f.read()
            last_pos += len(data)
            text = data.decode('utf-8', errors='replace')
            if "Type HELP for a list of commands" in text or "C:\\>" in text:
                found_shell = True
                break
    time.sleep(1)

if not found_shell:
    print("Shell not found")
    proc.terminate()
    exit(1)

print("[+] Shell found, sending NDREG LOAD command...")

keys = []
for c in "ndreg load \\system\\drivers\\test\\stress_lifecycle.nem":
    if c == ' ':
        keys.append("spc")
    elif c == '-':
        keys.append("minus")
    elif c == '_':
        keys.append("minus") # wait, shift-minus is underscore. Let's just avoid underscore if sendkey doesn't support it, but QEMU supports shift-minus. Actually, QEMU's sendkey takes 'shift-minus'
    elif c == '\\':
        keys.append("backslash")
    elif c == '.':
        keys.append("dot")
    else:
        keys.append(c)

# Correct the underscore mapping
keys = [k if k != 'minus' else 'shift-minus' for k in keys]
# Wait, "ndreg load ..." spaces will be 'shift-minus' if I'm not careful. No, I mapped ' ' to 'spc'
# And I mapped '_' to 'minus', wait! I need to map '_' to 'shift-minus'. Let's just fix it.

keys = []
for c in "ndreg load \\system\\drivers\\test\\stress_lifecycle.nem":
    if c == ' ': keys.append("spc")
    elif c == '_': keys.append("shift-minus")
    elif c == '\\': keys.append("backslash")
    elif c == '.': keys.append("dot")
    else: keys.append(c)

keys.append("ret")

send_keys(sock, keys)

print("[+] Command sent. Waiting for output...")
time.sleep(5)

with open("/tmp/test_ndreg_serial.log", 'r', errors='replace') as f:
    text = f.read()
    
print("--- OUTPUT BEGIN ---")
for line in text.splitlines():
    clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', line)
    if "ndreg load" in clean or "NEM" in clean or "Driver" in clean or "loaded" in clean.lower() or "sys_write" in clean or "Lifecycle" in clean:
        print(clean)
print("--- OUTPUT END ---")

proc.terminate()
sock.close()
