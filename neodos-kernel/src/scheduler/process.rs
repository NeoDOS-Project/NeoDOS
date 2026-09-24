//! Scheduler Eprocess impl — extracted from mod.rs
use alloc::string::ToString;
use crate::scheduler::types::{Eprocess, Kthread};
use crate::scheduler::address_space;

impl Eprocess {
    pub fn new_idle(pid: u32) -> Self {
        Eprocess {
            pid,
            parent_pid: 0,
            handle_table: crate::handle::HandleTable::new(),
            cwd_drive: 2,
            cwd_path: alloc::string::String::from("\\"),
            heap_base: 0,
            heap_break: 0,
            user_slot: None,
            mmap_regions: alloc::vec::Vec::new(),
            mmap_next: 0,
            thread_count: 0,
            exit_code: 0,
            obj_id: None,
            ob_id: None,
            address_space: address_space::AddressSpace::new(),
            token: crate::security::DEFAULT_ADMIN_TOKEN.clone(),
            vt_num: 0,
        }
    }

    pub fn new_kernel(pid: u32) -> Self {
        Eprocess {
            pid,
            parent_pid: 0,
            handle_table: crate::handle::HandleTable::new(),
            cwd_drive: 2,
            cwd_path: alloc::string::String::from("\\"),
            heap_base: 0,
            heap_break: 0,
            user_slot: None,
            mmap_regions: alloc::vec::Vec::new(),
            mmap_next: 0,
            thread_count: 0,
            exit_code: 0,
            obj_id: None,
            ob_id: None,
            address_space: address_space::AddressSpace::new(),
            token: crate::security::DEFAULT_ADMIN_TOKEN.clone(),
            vt_num: 0,
        }
    }

    pub fn new_ring3(pid: u32, slot_idx: u8, cwd_drive: u8, cwd_path: &str, heap_base: u64, parent_pid: u32) -> Self {
        Eprocess {
            pid,
            parent_pid,
            handle_table: crate::handle::HandleTable::with_defaults(),
            cwd_drive,
            cwd_path: cwd_path.to_string(),
            heap_base,
            heap_break: heap_base,
            user_slot: Some(slot_idx),
            mmap_regions: alloc::vec::Vec::new(),
            mmap_next: crate::arch::x64::paging::MMAP_BASE,
            thread_count: 1,
            exit_code: 0,
            obj_id: None,
            ob_id: None,
            address_space: address_space::AddressSpace::new(),
            token: crate::security::DEFAULT_ADMIN_TOKEN.clone(),
            vt_num: 0,
        }
    }
}
