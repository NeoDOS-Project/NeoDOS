use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use crate::scheduler::{self, ThreadState};
use crate::kobj::{self, KObjType};

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
        for i in 0..to_read {
            buf[i] = self.buf[(self.head + i) % PIPE_BUF_SIZE];
        }
        self.head = (self.head + to_read) % PIPE_BUF_SIZE;
        to_read
    }

    fn write_from(&mut self, buf: &[u8]) -> usize {
        let free = self.free();
        let to_write = core::cmp::min(free, buf.len());
        for i in 0..to_write {
            self.buf[(self.tail + i) % PIPE_BUF_SIZE] = buf[i];
        }
        self.tail = (self.tail + to_write) % PIPE_BUF_SIZE;
        to_write
    }
}

// ── Pipe Manager (dynamic, v0.41) ──

pub struct PipeManager {
    pipes: Mutex<Vec<Option<Mutex<PipeInner>>>>,
    kobj_ids: Mutex<Vec<Option<kobj::KObjId>>>,
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
        let mut kobj = self.kobj_ids.lock();
        if idx >= kobj.len() {
            kobj.resize_with(idx + 1, || None);
        }
    }

    fn set_kobj_id(&self, idx: usize, id: Option<kobj::KObjId>) {
        self.ensure_idx(idx);
        self.kobj_ids.lock()[idx] = id;
    }

    fn get_kobj_id(&self, idx: usize) -> Option<kobj::KObjId> {
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
                                if let Ok(kid) = kobj::kobj_register(KObjType::Pipe, &name, i as u64) {
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
        if let Ok(kid) = kobj::kobj_register(KObjType::Pipe, &name, i as u64) {
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
                kobj::kobj_unregister(kobj_id);
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

// ── Blocking support ──

fn wake_pipe_readers(pipe_id: u8) {
    let magic = 0xFFFF_0000u32 | (pipe_id as u32);
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let s = scheduler::current_scheduler();
    let mut scheduler = s.lock();
    scheduler.wake_blocked_on_magic(magic);
    crate::syscall::set_need_resched();
    drop(scheduler);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
}

pub fn block_current_for_pipe(pipe_id: u8) {
    let magic = 0xFFFF_0000u32 | (pipe_id as u32);
    let old_irql = unsafe { crate::hal::irql::raise_irql(crate::hal::irql::DISPATCH_LEVEL) };
    let mut lock = scheduler::current_scheduler().lock();
    if let Some(k) = lock.current_kthread_mut() {
        k.state = ThreadState::Blocked { waiting_for: magic };
        k.waiting_for = Some(magic);
    }
    crate::syscall::set_need_resched();
    drop(lock);
    unsafe { crate::hal::irql::lower_irql(old_irql) };
}

lazy_static::lazy_static! {
    pub static ref PIPE_MANAGER: PipeManager = PipeManager::new();
}
