target remote localhost:1234
set logging on gdb.log
set pagination off

# Load kernel symbols
file neodos-kernel/target/x86_64-unknown-none/release/neodos_kernel

# Breakpoints
break *0x200000
commands
  silent
  printf "Kernel entry point reached!\n"
  printf "RAX: 0x%016llx\n", $rax
  printf "RBX: 0x%016llx\n", $rbx
  printf "RSI: 0x%016llx\n", $rsi
  printf "RSP: 0x%016llx\n", $rsp
  printf "RIP: 0x%016llx\n", $rip
  continue
end

# Display useful info
define kernel_state
  printf "=== Kernel State ===\n"
  printf "RIP: 0x%016llx\n", $rip
  printf "RSP: 0x%016llx\n", $rsp
  printf "RBP: 0x%016llx\n", $rbp
  printf "CR3: 0x%016llx\n", $cr3
  printf "CR4: 0x%016llx\n", $cr4
end

# Watch stack pointer changes
watch $rsp

# Start execution
continue
