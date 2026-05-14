#!/usr/bin/env python3
import subprocess, time, os, socket, re, sys

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)

disk_image = os.path.join(PROJECT_ROOT, "disk_image.img")
disk_image2 = os.path.join(PROJECT_ROOT, "disk_image2.img")
ovmf_vars = "/tmp/OVMF_VARS_disk.fd"
subprocess.run(["cp", "/usr/share/OVMF/OVMF_VARS.fd", ovmf_vars], check=True)

cmd = [
    "qemu-system-x86_64", "-machine", "pc,accel=tcg",
    "-monitor", "telnet:127.0.0.1:4447,server,nowait",
    "-display", "none",
    "-drive", f"if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd",
    "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
    "-drive", f"format=raw,file={disk_image},index=0,media=disk",
    "-drive", f"format=raw,file={disk_image2},index=2,media=disk",
    "-m", "512M", "-serial", "file:/tmp/neodos_serial_disk.log",
]

os.unlink("/tmp/neodos_serial_disk.log") if os.path.exists("/tmp/neodos_serial_disk.log") else None
proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
time.sleep(8)

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(5)
try:
    sock.connect(('127.0.0.1', 4447))
except:
    print("[!] Monitor connection failed")
    sys.exit(1)

def send_keys(keys):
    for k in keys:
        sock.sendall(f"sendkey {k}\n".encode()); time.sleep(0.2)

def wait_serial(contains, timeout=25):
    start = time.time()
    while time.time() - start < timeout:
        if os.path.exists("/tmp/neodos_serial_disk.log"):
            with open("/tmp/neodos_serial_disk.log") as f:
                text = f.read()
                if contains in text:
                    return text
        time.sleep(0.3)
    return None

print("[*] Waiting for shell...")
wait_serial("C:\\>", 30)
print("[+] Shell ready, sending commands...")
time.sleep(3)

# Switch to US keyboard layout first (KEYB US) so we know the scancodes
send_keys(["k", "e", "y", "b", "spc", "u", "s", "ret"])
time.sleep(1)

# "dir" on C:\
send_keys(["d", "i", "r", "ret"])
time.sleep(2)

# "cd d:" to switch to D:
send_keys(["c", "d", "spc", "d", "shift-semicolon", "ret"])
time.sleep(2)

# "dir" on D:\
send_keys(["d", "i", "r", "ret"])
time.sleep(2)

# "type test.txt"
send_keys(["t", "y", "p", "e", "spc", "t", "e", "s", "t", ".", "t", "x", "t", "ret"])
time.sleep(2)

with open("/tmp/neodos_serial_disk.log") as f:
    text = f.read()
clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', text)
print("=== D: dir output ===")
for line in clean.split('\r'):
    l = line.strip()
    if l and ('D:' in l or 'Directory' in l or 'test' in l.lower() or len(l) < 60):
        print(f"  {l}")
print("======================")

proc.terminate(); proc.wait()
sock.close()
os.unlink(ovmf_vars)
