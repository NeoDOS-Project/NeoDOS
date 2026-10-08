//! Syscall tests — SSDT validation, permission checks, Ob create/query/set/enum,
//! and A4.6 integration tests.

use super::{SYSCALL_TABLE, SYSCALL_PERMISSIONS, check_syscall_permission,
           syscall_dispatch, err_to_u64, SyscallError};


mod table;
mod sync;
mod path;

pub use table::register_syscall_table_tests;
pub use sync::register_sync_tests;
pub use path::register_path_tests;
