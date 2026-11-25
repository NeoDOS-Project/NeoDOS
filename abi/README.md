# Kernel ABI manifest

`kernel-abi.json` is the versioned, machine-readable description of the
NeoDOS kernel ABI. It is generated from the kernel sources and is meant to be
consumed by external tools (NeoTools, `neodos-dev-server` MCP/LSP, future SDK
tooling).

## Contents

| Field | Source |
| ------- | -------- |
| `kernel_version` | `neodos-kernel/Cargo.toml` |
| `abi.syscall` | `libneodos/src/export.rs` (`ABI_VERSION`) |
| `abi.nem.{min,target,max}` | `neodos-kernel/src/drivers/nem/format.rs` |
| `abi.nem_format` | `NEM_VERSION_3` |
| `syscalls[]` | SSDT and permission tables in `neodos-kernel/src/syscall/mod.rs` |
| `ob_types[]` | `ObType` in `neodos-kernel/src/object/types.rs` |
| `ob_info_classes[]` | `ObInfoClass` |
| `ob_set_info_classes[]` | `ObSetInfoClass` |

## Regenerate

```bash
cargo run --manifest-path tools/abi-manifest/Cargo.toml
```

## Verify

```bash
cargo run --manifest-path tools/abi-manifest/Cargo.toml -- --check
```

CI runs the check on every pull request. A change to a syscall number,
permission, Ob type, info class, or ABI version must regenerate this file in the
same commit; otherwise the check fails.

## Schema versioning

`schema_version` describes this JSON layout (currently `1`). It changes only if
fields are renamed or removed in an incompatible way; additive fields do not
bump it.
