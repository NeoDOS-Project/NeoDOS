#!/usr/bin/env python3
"""
auto_test.py — Automatic test runner for NeoDOS
Uses serial log file monitoring + QEMU monitor sendkey for input.
"""

import subprocess
import time
import os
import sys
import re
import socket

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)

QEMU_OUTPUT_LOG = os.path.join(PROJECT_ROOT, "qemu_output_auto.log")

def send_monitor(sock, cmd, wait=0.2):
    try:
        sock.sendall((cmd + "\n").encode())
        time.sleep(wait)
        data = ""
        try:
            sock.settimeout(2)
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                data += chunk.decode('utf-8', errors='replace')
        except:
            pass
        return data
    except Exception as e:
        return f"[monitor error: {e}]"

def send_keys(sock, keys):
    time.sleep(0.3)
    for key in keys:
        resp = send_monitor(sock, f"sendkey {key}", 0.25)
    time.sleep(1.0)

def run_test():
    print("[*] NeoDOS Automatic Test Runner (serial log + sendkey)")
    print()
    
    use_ahci = True
    use_virtio = False
    for arg in sys.argv[1:]:
        if arg == "--ata":
            use_ahci = False
            use_virtio = False
        elif arg == "--ahci":
            use_ahci = True
            use_virtio = False
        elif arg == "--virtio":
            use_ahci = False
            use_virtio = True
    
    disk_image = os.path.join(PROJECT_ROOT, "disk_image.img")
    ovmf_code = "/usr/share/OVMF/OVMF_CODE.fd"
    ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd"
    
    for f in [disk_image, ovmf_code, ovmf_vars_template]:
        if not os.path.exists(f):
            print(f"[!] Missing: {f}")
            return 1
    
    # Create temp OVMF_VARS
    ovmf_vars = f"/tmp/OVMF_VARS_auto_{os.getpid()}.fd"
    subprocess.run(["cp", ovmf_vars_template, ovmf_vars], check=True)
    
    accel = os.environ.get("QEMU_ACCEL", "tcg")
    if accel == "kvm" and not (os.path.exists("/dev/kvm") and os.access("/dev/kvm", os.R_OK | os.W_OK)):
        print("[!] QEMU_ACCEL=kvm but /dev/kvm not available; falling back to tcg")
        accel = "tcg"
    print(f"[+] QEMU accelerator: {accel}")
    
    if use_virtio:
        # Q35 with disable-legacy=on forces modern MMIO transport
        machine = "q35"
    else:
        machine = "q35"

    cmd = [
        "qemu-system-x86_64",
        "-machine", f"{machine},accel={accel}",
        "-monitor", "telnet:127.0.0.1:4446,server,nowait",
        "-display", "none",
        "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf_code}",
        "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
    ]
    
    if use_virtio:
        cmd.extend([
            "-drive", f"if=none,format=raw,file={disk_image},id=virtioblk",
            "-device", "virtio-blk-pci,disable-modern=on,drive=virtioblk"
        ])
        print("[+] Storage: VirtIO Block Mode (q35, legacy-only)")
    elif use_ahci:
        cmd.extend([
            "-device", "ahci,id=ahci",
            "-drive", f"if=none,format=raw,file={disk_image},id=mydisk",
            "-device", "ide-hd,drive=mydisk,bus=ahci.0"
        ])
        print("[+] Storage: AHCI Mode")
    else:
        cmd.extend([
            "-drive", f"format=raw,file={disk_image},index=0,media=disk"
        ])
        print("[+] Storage: ATA/IDE Mode")

    # Default to user-mode networking (no sudo/root required).
    # Set NETWORK_MODE=tap or --tap for TAP mode (requires preconfigured tap0).
    use_tap = os.environ.get("NETWORK_MODE", "") == "tap"
    if use_tap:
        # Verify tap0 interface exists and is accessible
        if os.path.exists("/dev/net/tun"):
            try:
                r = subprocess.run(["ip", "link", "show", "tap0"], capture_output=True, text=True)
                use_tap = r.returncode == 0
            except:
                use_tap = False
    if not use_tap:
        # Also check command line for --tap
        for arg in sys.argv[1:]:
            if arg == "--tap":
                use_tap = True
                break

    if use_tap:
        print("[+] Network: TAP (tap0, 10.0.1.0/24)")
        print("[!] TAP requires pre-configured tap0 — see scripts/qemu-debug.sh")
        cmd.extend(["-netdev", "tap,id=net0,ifname=tap0,script=no", "-device", "e1000,netdev=net0"])
    else:
        print("[+] Network: user-mode (SLiRP) — no sudo needed")
        cmd.extend(["-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1", "-device", "e1000,netdev=net0"])

    cmd.extend([
        "-m", "512M",
        "-serial", "file:/tmp/neodos_serial.log",
    ])
    
    timeout = 180
    start_time = time.time()
    output_lines = []
    all_output = []
    
    # Clean up old serial log
    try:
        os.unlink("/tmp/neodos_serial.log")
    except:
        pass
    
    try:
        print("[+] Launching QEMU (headless, boot may take 30-60s)...")
        print()
        sys.stdout.flush()
        
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0
        )
        
        # Wait for QEMU to start
        time.sleep(3)
        
        # Connect to QEMU monitor
        monitor_sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        monitor_sock.settimeout(5)
        try:
            monitor_sock.connect(('127.0.0.1', 4446))
            resp = send_monitor(monitor_sock, "")
            print(f"[+] Monitor connected")
        except Exception as e:
            print(f"[!] Monitor connection failed: {e}")
            monitor_sock = None
        
        state = "booting"
        last_serial_len = 0
        waiting_lines = 0
        waiting_start = time.time()
        test_sent = False
        serial_file = "/tmp/neodos_serial.log"
        
        while time.time() - start_time < timeout:
            if proc.poll() is not None:
                print(f"\n[!] QEMU exited early with code {proc.returncode}")
                break
            
            # Read serial log file - only new bytes
            try:
                if os.path.exists(serial_file):
                    with open(serial_file, 'rb') as f:
                        f.seek(last_serial_len)
                        new_data = f.read()
                        if new_data:
                            last_serial_len += len(new_data)
                            
                            # Parse new bytes as lines
                            new_text = new_data.decode('utf-8', errors='replace')
                            for line in new_text.split('\r'):
                                line = line.strip()
                                if line:
                                    output_lines.append(line)
                                    all_output.append((line + "\r\n").encode())
                                    clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', line)
                                    print(f"[QEMU] {clean}")
                                    sys.stdout.flush()
                                    
                                    # State machine
                                    if state == "booting":
                                        if "ALL_TESTS_COMPLETE" in clean:
                                            print(f"\n[+] ALL TESTS COMPLETE")
                                            state = "done"
                                        if ("kernel tests" in clean.lower() or "passed" in clean or "failed" in clean) and "TEST" not in clean:
                                            print(f"[TEST] {clean}")
                                        if "Type HELP" in clean or ("NeoDOS" in clean and "FS Started" in clean):
                                            print("\n[+] Shell detected!")
                                            sys.stdout.flush()
                                            state = "idle"
                                    elif state == "idle":
                                        waiting_lines += 1
                                        if waiting_lines <= 3:
                                            print(f"[WAIT] {clean[:80]}")
                                    elif state == "waiting_response":
                                        if "ALL_TESTS_COMPLETE" in clean:
                                            print(f"\n[+] ALL TESTS COMPLETE")
                                            state = "done"
                                        if "kernel tests" in clean.lower() or "passed" in clean or "failed" in clean:
                                            print(f"[TEST] {clean}")
                                        if time.time() - waiting_start > 60:
                                            print(f"\n[!] Response timeout ({time.time()-waiting_start:.1f}s)")
                                            state = "done"
            except Exception as e:
                pass
            
            # Timeout fallback
            if state == "waiting_response" and time.time() - waiting_start > 60:
                print(f"\n[!] Response timeout ({time.time()-waiting_start:.1f}s)")
                state = "done"
                break
            
            time.sleep(0.3)
        
        # Cleanup
        if monitor_sock:
            monitor_sock.close()
        
        # Terminate QEMU
        if proc.poll() is None:
            print("\n[*] Terminating QEMU...")
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
        
        # Read stderr if any
        try:
            stderr_data = proc.stderr.read()
            if stderr_data:
                stderr_text = stderr_data.decode('utf-8', errors='replace')
                if stderr_text.strip():
                    print(f"[STDERR] {stderr_text[:200]}")
        except:
            pass
        
        # Analyze results
        print()
        print("=" * 60)
        print("TEST RESULTS")
        print("=" * 60)
        
        full_text = "\n".join(output_lines)
        
        # Save full log
        with open(QEMU_OUTPUT_LOG, "wb") as f:
            f.write(b"".join(all_output))
        print(f"[*] Full log saved to: {QEMU_OUTPUT_LOG}")
        
        # Check kernel tests
        if "kernel tests passed" in full_text:
            match = re.search(r"All (\d+) kernel tests passed", full_text)
            if match:
                print(f"[PASS] Kernel tests: {match.group(1)} tests passed")
            else:
                print("[PASS] Kernel tests: passed")
        elif "passed," in full_text and "failed" in full_text:
            print("[FAIL] Kernel tests had failures")
        else:
            print("[UNKNOWN] Could not determine kernel test results")
        
        # Check user-mode command tests (cmdtest.nxe)
        if "CMDTEST_COMPLETE" in full_text:
            if "ALL_COMMAND_TESTS_PASSED" in full_text:
                # Extract pass/fail count
                cmd_match = re.search(r"\[CMDTEST\] (\d+) passed, (\d+) failed", full_text)
                if cmd_match:
                    print(f"[PASS] Command tests: {cmd_match.group(1)} passed, {cmd_match.group(2)} failed")
                else:
                    print("[PASS] Command tests: all passed")
            else:
                cmd_match = re.search(r"\[CMDTEST\] (\d+) passed, (\d+) failed", full_text)
                if cmd_match:
                    print(f"[FAIL] Command tests: {cmd_match.group(1)} passed, {cmd_match.group(2)} failed")
                else:
                    print("[FAIL] Command tests: some failed")
        else:
            print("[SKIP] Command tests not run or incomplete")
        
        # Overall: kernel tests must pass; command tests optional
        kernel_ok = "kernel tests passed" in full_text
        cmdtest_ran = "CMDTEST_COMPLETE" in full_text
        cmd_ok = "ALL_COMMAND_TESTS_PASSED" in full_text
        if kernel_ok:
            if cmdtest_ran and cmd_ok:
                print("\n" + "=" * 60)
                print("OVERALL: SUCCESS (kernel + commands)")
                print("=" * 60)
                return 0
            elif cmdtest_ran:
                print("\n" + "=" * 60)
                print("OVERALL: KERNEL OK, COMMAND TESTS FAILED")
                print("=" * 60)
                return 1
            else:
                print("\n" + "=" * 60)
                print("OVERALL: KERNEL OK (no command tests)")
                print("=" * 60)
                return 0
        else:
            print("\n" + "=" * 60)
            print("OVERALL: INCOMPLETE")
            print("=" * 60)
            return 1
            
    except KeyboardInterrupt:
        print("\n[*] Interrupted by user")
        if 'proc' in locals() and proc.poll() is None:
            proc.terminate()
        return 1
    finally:
        if os.path.exists(ovmf_vars):
            os.unlink(ovmf_vars)
        try:
            os.unlink("/tmp/neodos_serial.log")
        except:
            pass

if __name__ == "__main__":
    sys.exit(run_test())