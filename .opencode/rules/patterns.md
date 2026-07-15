# Kernel Patterns (NeoDOS)

## ObOperation Trait

```rust
pub trait ObOperation {
    fn create(&mut self, name: &str, access: &AccessState) -> Result<(), NtStatus>;
    fn open(&mut self, access: &AccessState) -> Result<ObHandle, NtStatus>;
    fn close(&mut self) -> Result<(), NtStatus>;
    fn query_info(&self, class: ObInfoClass) -> Result<ObInfo, NtStatus>;
    fn set_info(&mut self, class: ObSetInfoClass, info: &ObInfo) -> Result<(), NtStatus>;
}
```

## Syscall Dispatch Pattern

```rust
// In src/syscall/dispatcher.rs
pub fn dispatch_syscall(cpu: &mut CpuContext) -> bool {
    let rax = cpu.regs.rax;
    match rax {
        0 => sys_nop(cpu),
        1 => sys_debug_output(cpu),
        // ...
        60.. => sys_ob_dispatch(cpu),
    }
}
```

## NEM Driver Lifecycle

1. DriverEntry — register dispatch routines, declare capabilities
2. DriverUnload — clean up resources
3. IRP dispatch — handle I/O requests
4. ABI negotiation — verify compatibility at load time

## Error Propagation

```rust
fn do_something() -> Result<SuccessInfo, NtStatus> {
    let handle = ob_open_object(name, access)?;  // Propagates NtStatus
    let info = handle.query_info(ObInfoClass::Basic)?;
    Ok(info)
}
```

## Test Pattern

```rust
test_case!(
    name: descriptive_name,
    description: "What this test verifies",
    group: subsystem,
    fn test() {
        // Arrange
        // Act
        // Assert
    }
);
```
