use spin::Mutex;
use crate::scheduler::{self, ThreadState};
use crate::kobj::{self, KObjType};

pub const PIPE_BUF_SIZE: usize = 4096;
pub const MAX_PIPES: usize = 16;

// ── Pipe buffer ──

struct PipeInner {
    buf: [u8; PIPE_BUF_SIZE],
    head: usize,
    tail: usize,
    write_closed: bool,
    read_closed: bool,
    in_use: bool,
    read_refs: u8,
    write_refs: u8,
}

impl PipeInner {
    const fn new() -> Self {
        PipeInner {
            buf: [0; PIPE_BUF_SIZE],
            head: 0,
            tail: 0,
            write_closed: false,
            read_closed: false,
            in_use: false,
            read_refs: 0,
            write_refs: 0,
        }
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

// ── Pipe Manager ──

pub struct PipeManager {
    pipes: [Mutex<PipeInner>; MAX_PIPES],
    kobj_ids: Mutex<[Option<kobj::KObjId>; MAX_PIPES]>,
}

const PIPE_INIT: Mutex<PipeInner> = Mutex::new(PipeInner::new());
const KOBJ_ID_NONE: Option<kobj::KObjId> = None;

impl PipeManager {
    pub const fn new() -> Self {
        PipeManager {
            pipes: [PIPE_INIT; MAX_PIPES],
            kobj_ids: Mutex::new([KOBJ_ID_NONE; MAX_PIPES]),
        }
    }

    fn set_kobj_id(&self, idx: usize, id: Option<kobj::KObjId>) {
        self.kobj_ids.lock()[idx] = id;
    }

    fn get_kobj_id(&self, idx: usize) -> Option<kobj::KObjId> {
        self.kobj_ids.lock()[idx]
    }

    pub fn alloc(&self) -> Option<u8> {
        for i in 0..MAX_PIPES {
            let mut pipe = self.pipes[i].lock();
            if !pipe.in_use {
                pipe.in_use = true;
                pipe.head = 0;
                pipe.tail = 0;
                pipe.write_closed = false;
                pipe.read_closed = false;
                pipe.read_refs = 0;
                pipe.write_refs = 0;
                drop(pipe);
                let name = alloc::format!("pipe/{}", i);
                if let Ok(kid) = kobj::kobj_register(KObjType::Pipe, &name, i as u64) {
                    self.set_kobj_id(i, Some(kid));
                }
                return Some(i as u8);
            }
        }
        None
    }

    pub fn inc_read_ref(&self, pipe_id: u8) {
        if (pipe_id as usize) < MAX_PIPES {
            let mut pipe = self.pipes[pipe_id as usize].lock();
            pipe.read_refs = pipe.read_refs.saturating_add(1);
        }
    }

    pub fn inc_write_ref(&self, pipe_id: u8) {
        if (pipe_id as usize) < MAX_PIPES {
            let mut pipe = self.pipes[pipe_id as usize].lock();
            pipe.write_refs = pipe.write_refs.saturating_add(1);
        }
    }

    pub fn dec_read_ref(&self, pipe_id: u8) {
        if (pipe_id as usize) < MAX_PIPES {
            let mut pipe = self.pipes[pipe_id as usize].lock();
            if pipe.read_refs > 0 {
                pipe.read_refs -= 1;
            }
            pipe.read_closed = true;
            if pipe.read_refs == 0 && pipe.write_refs == 0 {
                pipe.in_use = false;
                let idx = pipe_id as usize;
                if let Some(kid) = self.get_kobj_id(idx) {
                    kobj::kobj_unregister(kid);
                    self.set_kobj_id(idx, None);
                }
            }
        }
    }

    pub fn dec_write_ref(&self, pipe_id: u8) {
        if (pipe_id as usize) < MAX_PIPES {
            let mut pipe = self.pipes[pipe_id as usize].lock();
            if pipe.write_refs > 0 {
                pipe.write_refs -= 1;
            }
            pipe.write_closed = true;
            if pipe.read_refs == 0 && pipe.write_refs == 0 {
                pipe.in_use = false;
                let idx = pipe_id as usize;
                if let Some(kid) = self.get_kobj_id(idx) {
                    kobj::kobj_unregister(kid);
                    self.set_kobj_id(idx, None);
                }
            }
        }
    }

    pub fn read(&self, pipe_id: u8, buf: &mut [u8]) -> Result<usize, ()> {
        if (pipe_id as usize) >= MAX_PIPES {
            return Err(());
        }
        let mut pipe = self.pipes[pipe_id as usize].lock();
        if pipe.used() > 0 {
            let n = pipe.read_into(buf);
            drop(pipe);
            wake_pipe_readers(pipe_id);
            Ok(n)
        } else if pipe.write_closed {
            Ok(0)
        } else {
            Err(())
        }
    }

    pub fn write(&self, pipe_id: u8, buf: &[u8]) -> Result<usize, ()> {
        if (pipe_id as usize) >= MAX_PIPES {
            return Err(());
        }
        let mut pipe = self.pipes[pipe_id as usize].lock();
        if pipe.read_closed {
            return Err(());
        }
        if pipe.free() == 0 {
            return Err(());
        }
        let n = pipe.write_from(buf);
        drop(pipe);
        wake_pipe_readers(pipe_id);
        Ok(n)
    }
}

// ── Blocking support ──

fn wake_pipe_readers(pipe_id: u8) {
    let magic = 0xFFFF_0000u32 | (pipe_id as u32);
    crate::hal::without_interrupts(|| {
        let s = scheduler::current_scheduler();
        let mut scheduler = s.lock();
        scheduler.wake_blocked_on_magic(magic);
        crate::syscall::set_need_resched();
    });
}

pub fn block_current_for_pipe(pipe_id: u8) {
    let magic = 0xFFFF_0000u32 | (pipe_id as u32);
    crate::hal::without_interrupts(|| {
        let mut lock = scheduler::current_scheduler().lock();
        if let Some(k) = lock.current_kthread_mut() {
            k.state = ThreadState::Blocked { waiting_for: magic };
            k.waiting_for = Some(magic);
        }
        crate::syscall::set_need_resched();
    });
}

lazy_static::lazy_static! {
    pub static ref PIPE_MANAGER: PipeManager = PipeManager::new();
}
