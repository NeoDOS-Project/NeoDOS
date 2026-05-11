#!/usr/bin/env python3
"""
auto_test.py — Automatic test runner for NeoDOS
Launches QEMU headless, sends 'test' command, captures output.
"""

import subprocess
import time
import os
import sys
import re
import signal
import select

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.dirname(SCRIPT_DIR)

QEMU_OUTPUT_LOG = os.path.join(PROJECT_ROOT, "qemu_output_auto.log")

def run_test():
    print("[*] NeoDOS Automatic Test Runner")
    print()
    
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
        "-drive", f"format=raw,file={disk_image},index=0,media=disk",
        "-m", "512M",
        "-serial", "stdio",
    ] + kvm_args
    
    timeout = 120  # 2 minute timeout
    start_time = time.time()
    output_lines = []
    all_output = []
    
    try:
        print("[+] Launching QEMU (headless, boot may take 30-60s)...")
        print()
        sys.stdout.flush()
        
        proc = subprocess.Popen(
            cmd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            bufsize=0
        )
        
        import fcntl
        flags = fcntl.fcntl(proc.stdout, fcntl.F_GETFL)
        fcntl.fcntl(proc.stdout, fcntl.F_SETFL, flags | os.O_NONBLOCK)
        
        state = "booting"
        line_buffer = b""
        test_command_sent = False
        last_output_time = time.time()
        
        while time.time() - start_time < timeout:
            if proc.poll() is not None:
                print(f"\n[!] QEMU exited early with code {proc.returncode}")
                break
            
            try:
                chunk = proc.stdout.read(4096)
                if chunk:
                    last_output_time = time.time()
                    line_buffer += chunk
                    all_output.append(chunk)
                    
                    while b"\r" in line_buffer or b"\n" in line_buffer:
                        # Find first newline
                        cr_pos = line_buffer.find(b"\r")
                        lf_pos = line_buffer.find(b"\n")
                        
                        if cr_pos >= 0 and (lf_pos < 0 or cr_pos < lf_pos):
                            line = line_buffer[:cr_pos]
                            line_buffer = line_buffer[cr_pos+1:]
                            if line_buffer.startswith(b"\n"):
                                line_buffer = line_buffer[1:]
                        elif lf_pos >= 0:
                            line = line_buffer[:lf_pos]
                            line_buffer = line_buffer[lf_pos+1:]
                        else:
                            break
                        
                        line_str = line.decode('utf-8', errors='replace')
                        output_lines.append(line_str)
                        
                        # Filter ANSI codes for display
                        clean = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', line_str)
                        clean = re.sub(r'^\[2J|\[001;001H|\[=3h|\[8;[0-9]+;[0-9]+t', '', clean)
                        if clean.strip():
                            print(f"[QEMU] {clean}")
                            sys.stdout.flush()
                        
                        # State machine
                        if state == "booting":
                            # Look for shell prompt or "Type HELP"
                            if "Type HELP" in clean or "NeoDOS" in clean and "FS Started" in clean:
                                print("\n[+] Shell detected! Waiting for prompt...")
                                sys.stdout.flush()
                                state = "waiting_prompt"
                        
                        elif state == "waiting_prompt":
                            # Look for "C:\" anywhere in the line
                            if "C:\\" in clean:
                                print("[+] Got prompt! Sending 'test' command...")
                                sys.stdout.flush()
                                # Send "test\r"
                                proc.stdin.write(b"test\r")
                                proc.stdin.flush()
                                test_command_sent = True
                                state = "running_test"
                                start_test = time.time()
                        
                        elif state == "running_test":
                            # Test is running, watch for completion
                            # Kernel tests complete with "All X kernel tests passed" or "failed"
                            if "kernel tests passed" in clean:
                                print("\n[+] KERNEL TESTS PASSED!")
                                sys.stdout.flush()
                            elif "passed," in clean and "failed" in clean:
                                print("\n[!] KERNEL TESTS HAD FAILURES")
                                sys.stdout.flush()
                            
                            # Watch for user-mode test output
                            if "=== NeoDOS v0.9 Syscall Test ===" in clean:
                                print("\n[+] USER-MODE SYSTEST.BIN STARTED!")
                                sys.stdout.flush()
                            
                            # Watch for completion: "Process.*exited" or another prompt
                            if "exited" in clean and ("Process" in clean or "PID" in clean):
                                print("\n[+] Process exited - tests complete!")
                                sys.stdout.flush()
                                # Small delay for final output
                                time.sleep(2)
                                state = "done"
                                break
                            
                            # Timeout within test phase
                            if time.time() - start_test > 45:
                                print("\n[!] Test phase timeout (45s)")
                                state = "done"
                                break
                
                else:
                    time.sleep(0.1)
                    
                    # If waiting for prompt and it's been >30s, force send
                    if state == "waiting_prompt" and time.time() - last_output_time > 5:
                        print("[*] Sending 'test' command (no prompt detected)...")
                        sys.stdout.flush()
                        proc.stdin.write(b"test\r")
                        proc.stdin.flush()
                        test_command_sent = True
                        state = "running_test"
                        start_test = time.time()
                    
            except BlockingIOError:
                time.sleep(0.1)
                continue
            except Exception as e:
                print(f"[!] Error: {e}")
                import traceback
                traceback.print_exc()
                break
        
        # Terminate QEMU
        if proc.poll() is None:
            print("\n[*] Terminating QEMU...")
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
        
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
        if "=== NeoDOS v0.9 Syscall Test ===" in full_text:
            print("[PASS] User-mode SYSTEST.BIN executed")
            
            # Check for specific syscalls
            if "sys_open" in full_text and "readfile" in full_text.lower():
                print("[PASS] File I/O syscalls attempted")
                if "sys_open: empty path" in full_text:
                    print("[FAIL] sys_open received empty path (BUG!)")
                elif "File content:" in full_text:
                    print("[PASS] File content displayed")
        else:
            print("[UNKNOWN] SYSTEST.BIN output not found")
        
        # Overall
        if "kernel tests passed" in full_text and "sys_open: empty path" not in full_text:
            print("\n" + "=" * 60)
            print("OVERALL: SUCCESS")
            print("=" * 60)
            return 0
        elif "kernel tests passed" in full_text:
            print("\n" + "=" * 60)
            print("OVERALL: KERNEL OK, BUT SYSTEST HAS ISSUES")
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

if __name__ == "__main__":
    sys.exit(run_test())
