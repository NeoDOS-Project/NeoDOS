#!/usr/bin/env python3
"""Stress test: 300 iteraciones de comandos mixtos en NeoShell."""
import subprocess, time, os, sys, socket, itertools

os.chdir('/home/amartinper/rust-os/neodos')
serial_log = '/tmp/neodos_stress_300.log'
if os.path.exists(serial_log): os.remove(serial_log)
ovmf_vars = f'/tmp/OVMF_VARS_stress300_{os.getpid()}.fd'
subprocess.run(['cp', '/usr/share/OVMF/OVMF_VARS.fd', ovmf_vars], check=True)
subprocess.run(['pkill', '-9', 'qemu-system'], capture_output=True)
time.sleep(2)

MONITOR_PORT = 4449
proc = subprocess.Popen([
    'qemu-system-x86_64', '-machine', 'q35,accel=tcg', '-no-reboot',
    '-monitor', f'telnet:127.0.0.1:{MONITOR_PORT},server,nowait',
    '-display', 'none',
    '-drive', 'if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd',
    '-drive', f'if=pflash,format=raw,file={ovmf_vars}',
    '-device', 'ahci,id=ahci',
    '-drive', 'if=none,format=raw,file=disk_image.img,id=mydisk',
    '-device', 'ide-hd,drive=mydisk,bus=ahci.0',
    '-m', '512M', '-serial', f'file:{serial_log}',
], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print(f'[*] QEMU PID {proc.pid}')

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(2)
for i in range(60):
    try: sock.connect(('127.0.0.1', MONITOR_PORT)); break
    except: time.sleep(1)
else: print('[!] No monitor'); proc.kill(); sys.exit(1)

kp = {' ': 'spc', ':': 'shift-semicolon', '/': 'slash', '.': 'dot', '-': 'minus'}
def sk(keys, w=0.05):
    for k in keys: sock.sendall(f'sendkey {k}\n'.encode()); time.sleep(0.02)
    time.sleep(w)
def ty(t):
    for c in t.lower():
        if c in kp: sk([kp[c]])
        elif 'a' <= c <= 'z' or '0' <= c <= '9': sk([c])

print('[*] Waiting for shell...')
for a in range(300):
    try:
        with open(serial_log, 'rb') as f:
            if b'C:\\> _' in f.read(): break
    except: pass
    time.sleep(2)
else: print('[!] Timeout'); proc.kill(); sys.exit(1)
print('[+] Shell ready')

# Let serial log stabilize
time.sleep(5)
snap = os.path.getsize(serial_log)
pool = ['VER', 'DIR', 'NEOMEM', 'VOL', 'CPUINFO', 'DATETIME', 'DRIVES', 'PS', 'LABEL', 'HELP']

def check_crash():
    try:
        with open(serial_log, 'rb') as f:
            d = f.read()
    except: return False, b''
    new = d[snap:]
    if b'NeoDOS Bootloader v0' in new:
        return True, new
    return False, new

failed_at = None
for i in range(300):
    cmd = pool[i % len(pool)]
    try:
        ty(cmd)
        sk(['ret'], 0.10)
    except BrokenPipeError:
        print(f'\n[!] QEMU disconnected at command {i+1}/300')
        failed_at = i + 1
        break
    if (i + 1) % 30 == 0:
        crashed, new = check_crash()
        if crashed:
            failed_at = i + 1
            print(f'\n[!] CRASH at command {i+1}/300: {cmd}')
            for l in new.decode('utf-8', errors='replace').split('\n')[-20:]:
                if l.strip(): print(f'  {l.strip()[:120]}')
            break
        print(f'  [+] {i+1}/300 OK')

time.sleep(1)
if not failed_at:
    crashed, new = check_crash()
    if crashed:
        failed_at = 300
        print('\n[!] CRASH at end')

if failed_at:
    print(f'\n[FAIL] Stress test FAILED at command {failed_at}')
    proc.kill()
    sys.exit(1)

print('\n[OK] STRESS TEST PASSED: 300 commands without crash')
proc.kill()
