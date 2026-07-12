//! Pipe IPC system.
//!
//! FROZEN ABI (v0.43). See protocol invariants below.

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use crate::object::{self, ObOperations, ObId, ObType};

pub const PIPE_BUF_SIZE: usize = 4096;
pub const MAX_PIPES: usize = 16;

// ── Pipe buffer (heap-allocated, v0.41) ──

struct PipeInner {
    buf: Box<[u8; PIPE_BUF_SIZE]>,
    head: usize,
    tail: usize,
    write_closed: bool,
    read_closed: bool,
    in_use: bool,
    read_refs: u8,
    write_refs: u8,
}

impl PipeInner {
    fn new_unused() -> Self {
        PipeInner {
            buf: Box::new([0u8; PIPE_BUF_SIZE]),
            head: 0,
            tail: 0,
            write_closed: false,
            read_closed: false,
            in_use: false,
            read_refs: 0,
            write_refs: 0,
        }
    }

    fn reset(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.write_closed = false;
        self.read_closed = false;
        self.read_refs = 0;
        self.write_refs = 0;
    }

    fn used(&self) -> usize {
        if self.tail >= self.head {
            self.tail - self.head
        } else {
            PIPE_BUF_SIZE - self.head + self.tail
        }
    }

    fn free(&self) -> usize {
        PIPE_BUF_SIZE - self.used() - 1
    }

    fn read_into(&mut self, buf: &mut [u8]) -> usize {
        let available = self.used();
        let to_read = core::cmp::min(available, buf.len());
        for (i, b) in buf.iter_mut().enumerate().take(to_read) {
            *b = self.buf[(self.head + i) % PIPE_BUF_SIZE];
        }
        self.head = (self.head + to_read) % PIPE_BUF_SIZE;
        to_read
    }

    fn write_from(&mut self, buf: &[u8]) -> usize {
        let free = self.free();
        let to_write = core::cmp::min(free, buf.len());
        for (i, &b) in buf.iter().enumerate().take(to_write) {
            self.buf[(self.tail + i) % PIPE_BUF_SIZE] = b;
        }
        self.tail = (self.tail + to_write) % PIPE_BUF_SIZE;
        to_write
    }
}

// ── Pipe Manager (dynamic, v0.41) ──

pub struct PipeManager {
    pipes: Mutex<Vec<Option<Mutex<PipeInner>>>>,
    kobj_ids: Mutex<Vec<Option<ObId>>>,
}

impl PipeManager {
    pub fn new() -> Self {
        PipeManager {
            pipes: Mutex::new(Vec::new()),
            kobj_ids: Mutex::new(Vec::new()),
        }
    }

    fn ensure_idx(&self, idx: usize) {
        let mut pipes = self.pipes.lock();
        if idx >= pipes.len() {
            pipes.resize_with(idx + 1, || None);
        }
        drop(pipes);
        let mut ids = self.kobj_ids.lock();
        if idx >= ids.len() {
            ids.resize_with(idx + 1, || None);
        }
    }

    fn set_kobj_id(&self, idx: usize, id: Option<ObId>) {
        self.ensure_idx(idx);
        self.kobj_ids.lock()[idx] = id;
    }

    fn get_kobj_id(&self, idx: usize) -> Option<ObId> {
        self.ensure_idx(idx);
        self.kobj_ids.lock()[idx]
    }

    pub fn alloc(&self) -> Option<u8> {
        // Try to find an unused pipe by scanning
        let mut pipes = self.pipes.lock();
        for i in 0..pipes.len() {
            if let Some(ref mp) = pipes[i] {
                let mut pipe = mp.lock();
                if !pipe.in_use {
                    pipe.in_use = true;
                    pipe.reset();
                    drop(pipe);
                    // Drop pipes *before* set_kobj_id to avoid reentrancy:
                    // set_kobj_id → ensure_idx → pipes.lock() would deadlock.
                    drop(pipes);
                    let name = alloc::format!("pipe/{}", i);
                                if let Ok(kid) = object::ob_create_object(ObType::Pipe, &name, i as u64, 0, None) {
                        self.set_kobj_id(i, Some(kid));
                    }
                    return Some(i as u8);
                }
            }
        }
        // No free slot found — create a new one
        if pipes.len() >= MAX_PIPES {
            return None;
        }
        let i = pipes.len();
        let inner = Mutex::new(PipeInner::new_unused());
        // Mark as in_use
        {
            let mut pipe = inner.lock();
            pipe.in_use = true;
        }
        pipes.push(Some(inner));
        drop(pipes);

        self.ensure_idx(i);
        let name = alloc::format!("pipe/{}", i);
        if let Ok(kid) = object::ob_create_object(ObType::Pipe, &name, i as u64, 0, None) {
            self.set_kobj_id(i, Some(kid));
        }
        Some(i as u8)
    }

    pub fn inc_read_ref(&self, pipe_id: u8) {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        if let Some(Some(ref mp)) = pipes.get(pipe_id as usize) {
            let mut pipe = mp.lock();
            pipe.read_refs = pipe.read_refs.saturating_add(1);
        }
    }

    pub fn inc_write_ref(&self, pipe_id: u8) {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        if let Some(Some(ref mp)) = pipes.get(pipe_id as usize) {
            let mut pipe = mp.lock();
            pipe.write_refs = pipe.write_refs.saturating_add(1);
        }
    }

    fn maybe_free_pipe(&self, pipe_id: u8) {
        let idx = pipe_id as usize;
        // Extract kobj_id *before* the pipes lock to avoid reentrancy:
        // get_kobj_id calls ensure_idx which tries to lock self.pipes.
        let kid = self.get_kobj_id(idx);
        let should_free = {
            let pipes = self.pipes.lock();
            if let Some(Some(ref mp)) = pipes.get(idx) {
                let mut pipe = mp.lock();
                if pipe.read_refs == 0 && pipe.write_refs == 0 {
                    pipe.in_use = false;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if should_free {
            if let Some(kobj_id) = kid {
                object::ob_destroy_object(kobj_id).ok();
                self.set_kobj_id(idx, None);
            }
        }
    }

    pub fn dec_read_ref(&self, pipe_id: u8) {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        if let Some(Some(ref mp)) = pipes.get(pipe_id as usize) {
            let mut pipe = mp.lock();
            if pipe.read_refs > 0 { pipe.read_refs -= 1; }
            pipe.read_closed = true;
        }
        drop(pipes);
        self.maybe_free_pipe(pipe_id);
    }

    pub fn dec_write_ref(&self, pipe_id: u8) {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        if let Some(Some(ref mp)) = pipes.get(pipe_id as usize) {
            let mut pipe = mp.lock();
            if pipe.write_refs > 0 { pipe.write_refs -= 1; }
            pipe.write_closed = true;
        }
        drop(pipes);
        self.maybe_free_pipe(pipe_id);
    }

    pub fn read(&self, pipe_id: u8, buf: &mut [u8]) -> Result<usize, ()> {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        let mp = pipes.get(pipe_id as usize)
            .and_then(|p| p.as_ref())
            .ok_or(())?;
        let mut pipe = mp.lock();
        if pipe.used() > 0 {
            let n = pipe.read_into(buf);
            drop(pipe);
            drop(pipes);
            wake_pipe_readers(pipe_id);
            Ok(n)
        } else if pipe.write_closed {
            Ok(0)
        } else {
            Err(())
        }
    }

    pub fn write(&self, pipe_id: u8, buf: &[u8]) -> Result<usize, ()> {
        self.ensure_idx(pipe_id as usize);
        let pipes = self.pipes.lock();
        let mp = pipes.get(pipe_id as usize)
            .and_then(|p| p.as_ref())
            .ok_or(())?;
        let mut pipe = mp.lock();
        if pipe.read_closed {
            return Err(());
        }
        if pipe.free() == 0 {
            return Err(());
        }
        let n = pipe.write_from(buf);
        drop(pipe);
        drop(pipes);
        wake_pipe_readers(pipe_id);
        Ok(n)
    }
}

// ── Poll helpers (added v0.43 for sys_poll) ──

/// Peek whether a pipe has data available for reading.
/// Returns None if pipe_id is invalid.
pub fn pipe_peek_read_ready(pipe_id: u8) -> Option<bool> {
    let pipes = PIPE_MANAGER.pipes.lock();
    let mp = pipes.get(pipe_id as usize).and_then(|p| p.as_ref())?;
    let pipe = mp.lock();
    Some(pipe.used() > 0 || pipe.write_closed)
}

/// Peek whether a pipe's write end is still open.
/// Returns None if pipe_id is invalid.
pub fn pipe_peek_write_closed(pipe_id: u8) -> Option<bool> {
    let pipes = PIPE_MANAGER.pipes.lock();
    let mp = pipes.get(pipe_id as usize).and_then(|p| p.as_ref())?;
    let pipe = mp.lock();
    Some(pipe.write_closed)
}

/// Peek whether a pipe's read end is still open.
/// Returns None if pipe_id is invalid.
pub fn pipe_peek_read_closed(pipe_id: u8) -> Option<bool> {
    let pipes = PIPE_MANAGER.pipes.lock();
    let mp = pipes.get(pipe_id as usize).and_then(|p| p.as_ref())?;
    let pipe = mp.lock();
    Some(pipe.read_closed)
}

// ── Blocking support (via KWait, OB-031) ──

fn wake_pipe_readers(pipe_id: u8) {
    crate::kwait::kwait_wake(&crate::kwait::WaitReason::PipeRead { pipe_id: pipe_id as u16 });
}

pub fn block_current_for_pipe(pipe_id: u8) {
    crate::kwait::kwait_block(crate::kwait::WaitReason::PipeRead { pipe_id: pipe_id as u16 });
}

// ── ObOperations integration (OB-016) ──

pub struct PipeObOps;

impl ObOperations for PipeObOps {
    fn on_destroy(&self, _id: ObId, native_id: u64) {
        let pipe_id = native_id as u8;
        PIPE_MANAGER.free_pipe(pipe_id);
    }
}

pub static PIPE_OPS: PipeObOps = PipeObOps;

impl PipeManager {
    pub fn free_pipe(&self, pipe_id: u8) {
        let idx = pipe_id as usize;
        self.ensure_idx(idx);
        // Unregister KOBJ if registered
        let kid = self.get_kobj_id(idx);
        if let Some(obj_id) = kid {
            let _ = object::ob_destroy_object(obj_id);
            self.set_kobj_id(idx, None);
        }
        // Mark pipe slot as unused
        let pipes = self.pipes.lock();
        if let Some(Some(ref mp)) = pipes.get(idx) {
            let mut pipe = mp.lock();
            pipe.in_use = false;
            pipe.reset();
        }
    }
}

lazy_static::lazy_static! {
    pub static ref PIPE_MANAGER: PipeManager = PipeManager::new();
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("pipe_alloc_free", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc failed");
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_write_read", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let data = b"Hello, Pipe!";
        let n = PIPE_MANAGER.write(pid, data).unwrap();
        test_eq!(n, data.len());
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_multiple_writes", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.write(pid, b"abc").unwrap();
        PIPE_MANAGER.write(pid, b"def").unwrap();
        PIPE_MANAGER.write(pid, b"ghi").unwrap();
        let mut buf = [0u8; 16];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 9);
        test_eq!(&buf[..n], b"abcdefghi");
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_eof", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.write(pid, b"data").unwrap();
        PIPE_MANAGER.dec_write_ref(pid);
        let mut buf = [0u8; 16];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 4);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0);
        PIPE_MANAGER.dec_read_ref(pid);
    });

    test_case!("pipe_buffer_capacity", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let buf = [0xABu8; 256];
        let mut total = 0usize;
        while let Ok(n) = PIPE_MANAGER.write(pid, &buf) {
            total += n;
        }
        test_true!(total > 0);
        let mut out = [0u8; 256];
        let mut read_total = 0usize;
        while let Ok(n) = PIPE_MANAGER.read(pid, &mut out) {
            if n == 0 { break; }
            read_total += n;
        }
        test_eq!(read_total, total);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_write_after_read_close", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.dec_read_ref(pid);
        let result = PIPE_MANAGER.write(pid, b"test");
        test_true!(result.is_err());
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_alloc_max", {
        let mut pipes = alloc::vec::Vec::new();
        while let Some(pid) = PIPE_MANAGER.alloc() {
            pipes.push(pid);
        }
        test_true!(pipes.len() <= 16);
        test_true!(!pipes.is_empty());
        test_eq!(PIPE_MANAGER.alloc(), None);
        for pid in pipes {
            PIPE_MANAGER.inc_read_ref(pid);
            PIPE_MANAGER.inc_write_ref(pid);
            PIPE_MANAGER.dec_read_ref(pid);
            PIPE_MANAGER.dec_write_ref(pid);
        }
    });

    test_case!("pipe_block_current_wake_kwait", {
        use crate::scheduler::ThreadState;
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        crate::object::pipe::block_current_for_pipe(pid);
        let expected_magic = 0x0001_0000u32 | pid as u32;
        let state = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state, ThreadState::Blocked { waiting_for: expected_magic });
        crate::kwait::kwait_wake(&crate::kwait::WaitReason::PipeRead { pipe_id: pid as u16 });
        let state2 = crate::hal::without_interrupts(|| {
            let s = crate::scheduler::current_scheduler();
            let lock = s.lock();
            lock.kthreads[0].as_ref().unwrap().state
        });
        test_eq!(state2, ThreadState::Ready);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });

    test_case!("pipe_two_commands", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let data = b"pipeline two commands";
        PIPE_MANAGER.write(pid, data).unwrap();
        PIPE_MANAGER.dec_write_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
        PIPE_MANAGER.inc_read_ref(pid);
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_read_ref(pid);
    });

    test_case!("pipe_chain_three", {
        let p1 = PIPE_MANAGER.alloc().unwrap();
        let p2 = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(p1); PIPE_MANAGER.inc_write_ref(p1);
        PIPE_MANAGER.inc_read_ref(p2); PIPE_MANAGER.inc_write_ref(p2);
        PIPE_MANAGER.inc_write_ref(p1);
        PIPE_MANAGER.write(p1, b"data1").unwrap();
        PIPE_MANAGER.dec_write_ref(p1);
        PIPE_MANAGER.dec_write_ref(p1);
        PIPE_MANAGER.inc_read_ref(p1);
        PIPE_MANAGER.inc_write_ref(p2);
        let mut tmp = [0u8; 16];
        let n = PIPE_MANAGER.read(p1, &mut tmp).unwrap();
        test_eq!(n, 5);
        test_eq!(&tmp[..n], b"data1");
        test_eq!(PIPE_MANAGER.read(p1, &mut tmp).unwrap(), 0);
        PIPE_MANAGER.write(p2, b"data2").unwrap();
        PIPE_MANAGER.dec_read_ref(p1);
        PIPE_MANAGER.dec_write_ref(p2);
        PIPE_MANAGER.dec_read_ref(p1);
        PIPE_MANAGER.dec_write_ref(p2);
        PIPE_MANAGER.inc_read_ref(p2);
        let mut out = [0u8; 16];
        let n2 = PIPE_MANAGER.read(p2, &mut out).unwrap();
        test_eq!(n2, 5);
        test_eq!(&out[..n2], b"data2");
        test_eq!(PIPE_MANAGER.read(p2, &mut out).unwrap(), 0);
        PIPE_MANAGER.dec_read_ref(p2);
        PIPE_MANAGER.dec_read_ref(p2);
    });

    test_case!("pipe_blocking_read", {
        let pid = PIPE_MANAGER.alloc().unwrap();
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let mut buf = [0u8; 16];
        test_true!(PIPE_MANAGER.read(pid, &mut buf).is_err());
        PIPE_MANAGER.write(pid, b"now data").unwrap();
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, 8);
        test_eq!(&buf[..n], b"now data");
        test_true!(PIPE_MANAGER.read(pid, &mut buf).is_err());
        PIPE_MANAGER.dec_write_ref(pid);
        let n2 = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n2, 0);
        PIPE_MANAGER.dec_read_ref(pid);
    });

    test_case!("pipe_ob_create_destroy", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc");
        let name = alloc::format!("OBPIPE{}", pid);
        let ob_id = crate::object::ob_create_object(
            crate::object::ObType::Pipe, &name, pid as u64, 0, Some(&crate::object::pipe::PIPE_OPS),
        ).expect("ob create");
        test_true!(ob_id > 0);
        let obj = crate::object::ob_lookup(ob_id).expect("ob lookup");
        test_eq!(obj.obj_type, crate::object::ObType::Pipe);
        test_eq!(obj.native_id, pid as u64);
        crate::object::ob_close_object(ob_id).unwrap();
        test_true!(crate::object::ob_lookup(ob_id).is_none());
        let pid2 = PIPE_MANAGER.alloc().expect("reuse after ob free");
        test_eq!(pid, pid2);
    });

    test_case!("pipe_ob_read_write", {
        let pid = PIPE_MANAGER.alloc().expect("pipe alloc");
        PIPE_MANAGER.inc_read_ref(pid);
        PIPE_MANAGER.inc_write_ref(pid);
        let data = b"OB-016 rw test";
        let n = PIPE_MANAGER.write(pid, data).unwrap();
        test_eq!(n, data.len());
        let mut buf = [0u8; 64];
        let n = PIPE_MANAGER.read(pid, &mut buf).unwrap();
        test_eq!(n, data.len());
        test_eq!(&buf[..n], data);
        PIPE_MANAGER.dec_read_ref(pid);
        PIPE_MANAGER.dec_write_ref(pid);
    });
}
