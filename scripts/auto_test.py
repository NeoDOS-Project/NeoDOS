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
    for arg in sys.argv[1:]:
        if arg == "--ata":
            use_ahci = False
        elif arg == "--ahci":
            use_ahci = True
    
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
    
    cmd = [
        "qemu-system-x86_64",
        "-machine", f"pc,accel={accel}",
        "-monitor", "telnet:127.0.0.1:4446,server,nowait",
        "-display", "none",
        "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf_code}",
        "-drive", f"if=pflash,format=raw,file={ovmf_vars}",
    ]
    
    if use_ahci:
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

    cmd.extend([
        "-m", "512M",
        "-serial", "file:/tmp/neodos_serial.log",
    ])
    
    timeout = 120
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
                                        if "Type HELP" in clean or ("NeoDOS" in clean and "FS Started" in clean):
                                            print("\n[+] Shell detected! Waiting for prompt...")
                                            sys.stdout.flush()
                                            state = "waiting_prompt"
                                            waiting_lines = 0
                                            waiting_start = time.time()
                                    elif state == "waiting_prompt":
                                        waiting_lines += 1
                                        print(f"[WAIT] line #{waiting_lines}: {clean[:80]}")
                                        is_neoshell = "neoshell" in clean.lower() or "[ns]" in clean
                                        has_prompt = "C:\\>" in clean
                                        if has_prompt and not test_sent:
                                            if is_neoshell:
                                                print(f"[+] Neoshell detected, sending 'exit' to reach kernel shell...")
                                                sys.stdout.flush()
                                                if monitor_sock:
                                                    send_keys(monitor_sock, ["e", "x", "i", "t", "ret"])
                                                    time.sleep(1.0)
                                                    state = "booting"
                                                    test_sent = False
                                                    waiting_start = time.time()
                                            else:
                                                print(f"[+] Kernel shell ready, sending 'test' via sendkey...")
                                                sys.stdout.flush()
                                                if monitor_sock:
                                                    send_keys(monitor_sock, ["t", "e", "s", "t", "ret"])
                                                    test_sent = True
                                                    state = "waiting_response"
                                                    waiting_start = time.time()
                                                    print("[+] 'test' command sent!")
                                    elif state == "waiting_response":
                                        if "Running" in clean and "self-tests" in clean:
                                            print(f"\n[+] TEST EXECUTED!")
                                        if "kernel tests" in clean.lower() or "passed" in clean or "failed" in clean:
                                            print(f"[TEST] {clean}")
                                        if "ALL_TESTS_COMPLETE" in clean:
                                            print(f"\n[+] ALL TESTS COMPLETE")
                                            state = "done"
                                            break
                                        if time.time() - waiting_start > 60:
                                            print(f"\n[!] Response timeout ({time.time()-waiting_start:.1f}s)")
                                            state = "done"
                                            break
            except Exception as e:
                pass
            
            # Monitor timeout fallback: send 'test' directly (already in kernel shell)
            if monitor_sock and state == "waiting_prompt" and time.time() - waiting_start > 10 and not test_sent:
                print("[*] Prompt timeout: sending 'test' via sendkey...")
                sys.stdout.flush()
                send_keys(monitor_sock, ["t", "e", "s", "t", "ret"])
                test_sent = True
                state = "waiting_response"
                waiting_start = time.time()
            
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
        
        # Check user-mode tests
        user_tests_found = 0
        user_bins = ["cpuinfo.nxe", "dir.nxe", "datetime.nxe", "ver.nxe"]
        for ut in user_bins:
            if f"--- Running" in full_text and ut in full_text:
                user_tests_found += 1
        if user_tests_found >= 4:
            print(f"[PASS] All {user_tests_found} user-mode binaries executed")
        elif user_tests_found > 0:
            print(f"[PARTIAL] {user_tests_found}/{len(user_bins)} user-mode binaries executed")
        else:
            print("[UNKNOWN] No user-mode binary output found")
        
        # Overall
        if "kernel tests passed" in full_text and "ALL_TESTS_COMPLETE" in full_text:
            print("\n" + "=" * 60)
            print("OVERALL: SUCCESS")
            print("=" * 60)
            return 0
        elif "kernel tests passed" in full_text:
            print("\n" + "=" * 60)
            print("OVERALL: KERNEL OK, BUT USER TESTS INCOMPLETE")
            print("=" * 60)
            return 1
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