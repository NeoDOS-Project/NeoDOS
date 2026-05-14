// src/tsr/mod.rs

#![allow(dead_code)]

use spin::Mutex;
use lazy_static::lazy_static;
use crate::fs::neodos_fs::NeoDosFs;
use crate::buffer::block_cache::BlockCache;
use crate::drivers::ata::AtaDriver;

const MAX_TSR_SIZE: usize = 65536;

static mut TSR_LOAD_BUF: [u8; MAX_TSR_SIZE] = [0u8; MAX_TSR_SIZE];

#[derive(Clone, Copy)]
pub struct TsrInfo {
    pub name: [u8; 16],
    pub base_address: u64,
    #[allow(dead_code)]
    pub size: usize,
    pub interrupt_num: u8,
}

pub struct TsrRegistry {
    pub programs: [Option<TsrInfo>; 16],
    pub next_address: u64,
}

lazy_static! {
    pub static ref TSR_REGISTRY: Mutex<TsrRegistry> = Mutex::new(TsrRegistry {
        programs: [None; 16],
        next_address: 0x430000,
    });
}

pub fn install_tsr(filename: &str, interrupt_num: u8, fs: &mut NeoDosFs, cache: &mut BlockCache, ata: &mut AtaDriver) -> Result<u64, ()> {
    match fs.find_file(filename, cache, ata) {
        Ok(inode_num) => {
            let buf = unsafe { &mut TSR_LOAD_BUF };
            match fs.read_file_to_buf(inode_num, buf, cache, ata) {
                Ok(read) => {
                    let mut registry = TSR_REGISTRY.lock();
                    let addr = registry.next_address;
                    
                    if addr + read as u64 > 0x500000 {
                        return Err(()); // No more TSR memory
                    }

                    // Copy to memory
                    unsafe {
                        core::ptr::copy_nonoverlapping(buf.as_ptr(), addr as *mut u8, read);
                    }

                    // Check vector conflict
                    for existing in &registry.programs {
                        if let Some(info) = existing {
                            if info.interrupt_num == interrupt_num {
                                return Err(()); // Vector already claimed
                            }
                        }
                    }

                    // Register
                    for i in 0..16 {
                        if registry.programs[i].is_none() {
                            let mut name = [0u8; 16];
                            let name_bytes = filename.as_bytes();
                            let len = name_bytes.len().min(16);
                            name[..len].copy_from_slice(&name_bytes[..len]);

                            registry.programs[i] = Some(TsrInfo {
                                name,
                                base_address: addr,
                                size: read,
                                interrupt_num,
                            });
                            
                            // Align next address to 4KB
                            registry.next_address = (addr + read as u64 + 0xFFF) & !0xFFF;
                            return Ok(addr);
                        }
                    }
                    Err(()) // Too many TSRs
                }
                Err(_) => Err(()),
            }
        }
        Err(_) => Err(()),
    }
}

pub fn dispatch_interrupt(interrupt_num: u8) {
    let registry = TSR_REGISTRY.lock();
    for prog in &registry.programs {
        if let Some(info) = prog {
            if info.interrupt_num == interrupt_num {
                // Call the TSR entry point
                unsafe {
                    let func: extern "C" fn() = core::mem::transmute(info.base_address);
                    func();
                }
            }
        }
    }
}
