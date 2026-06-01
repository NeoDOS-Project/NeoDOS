#!/usr/bin/env python3
"""Minimal test: boot, run CPUTEST.BIN, capture output."""
import subprocess, time, os, sys, socket, re

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)
disk_image = os.path.join(PROJECT_ROOT, "disk_image.img")
ovmf_code = "/usr/share/OVMF/OVMF_CODE.fd"
ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd"
ovmf_vars = f"/tmp/OVMF_VARS_cputest_{os.getpid()}.fd"
subprocess.run(["cp", ovmf_vars_template, ovmf_vars], check=True)

accel = os.environ.get("QEMU_ACCEL", "tcg")
cmd = [
    "qemu-system-x86_64", "-machine", f"pc,accel={accel}",
    "-monitor", "telnet:127.0.0.1:4446,server,nowait",
    "-display", "none",
    "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf_code}",
    "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
    "-drive", f"format=raw,file={disk_image},index=0,media=disk",
    "-m", "512M",
    "-serial", "file:/tmp/neodos_cputest.log",
]

try:
    os.unlink("/tmp/neodos_cputest.log")
except:
    pass

proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, bufsize=0)
time.sleep(3)

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(5)
sock.connect(('127.0.0.1', 4446))
sock.sendall(b"\n")
time.sleep(0.5)

print("[+] Boot starting (up to 60s)...")
sys.stdout.flush()

timeout = 120
start = time.time()
sent_run = False
sent_exit = False
boot_detected = False
last_len = 0
last_display = ""

while time.time() - start < timeout:
    if proc.poll() is not None:
        break
    if os.path.exists("/tmp/neodos_cputest.log"):
        with open("/tmp/neodos_cputest.log", "rb") as f:
            f.seek(last_len)
            data = f.read()
            if data:
                last_len += len(data)
                text = data.decode('utf-8', errors='replace')
                for line in text.split('\r'):
                    line = line.strip()
                    if line:
                        clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', line)
                        if clean != last_display:
                            print(f"  {clean}")
                            sys.stdout.flush()
                            last_display = clean

                        if "C:\\>" in clean:
                            boot_detected = True

    if boot_detected and not sent_run and time.time() > start + 25:
        time.sleep(2)
        print("[+] Sending: run CPUTEST.BIN")
        sys.stdout.flush()
        for ch in "r", "u", "n", "spc", "c", "p", "u", "t", "e", "s", "t", "dot", "b", "i", "n", "ret":
            sock.sendall(f"sendkey {ch}\n".encode())
            time.sleep(0.15)
        sent_run = True
        time.sleep(3)

    if sent_run and time.time() - start > 80 and not sent_exit:
        print("[+] Timeout, stopping QEMU")
        sys.stdout.flush()
        sent_exit = True

    time.sleep(0.05)

sock.close()
proc.terminate()
try:
    proc.wait(timeout=3)
except:
    proc.kill()

print("\n[Done]")
sys.exit(0)
