use crate::object::{ObOperations, ObId};
use crate::kwait::{self, WaitReason};
use spin::Mutex;

const MAX_SEMAPHORES: usize = 64;

#[derive(Debug, Clone, Copy)]
struct SemaphoreEntry {
    used: bool,
    count: i32,
    max_count: i32,
    ob_id: ObId,
}

impl SemaphoreEntry {
    const fn unused() -> Self {
        SemaphoreEntry {
            used: false,
            count: 0,
            max_count: 0,
            ob_id: 0,
        }
    }
}

pub struct SemaphoreManager {
    semaphores: [SemaphoreEntry; MAX_SEMAPHORES],
}

impl SemaphoreManager {
    const fn new() -> Self {
        SemaphoreManager {
            semaphores: [SemaphoreEntry::unused(); MAX_SEMAPHORES],
        }
    }

    pub fn alloc(&mut self, ob_id: ObId, initial_count: i32, max_count: i32) -> Option<u32> {
        if initial_count < 0 || max_count <= 0 || initial_count > max_count {
            return None;
        }
        for (i, s) in self.semaphores.iter_mut().enumerate() {
            if !s.used {
                s.used = true;
                s.count = initial_count;
                s.max_count = max_count;
                s.ob_id = ob_id;
                return Some(i as u32);
            }
        }
        None
    }

    pub fn free(&mut self, sem_id: u32) {
        if let Some(s) = self.semaphores.get_mut(sem_id as usize) {
            *s = SemaphoreEntry::unused();
        }
    }

    pub fn release(&mut self, sem_id: u32, release_count: i32) -> bool {
        if release_count <= 0 {
            return false;
        }
        if let Some(s) = self.semaphores.get_mut(sem_id as usize) {
            if !s.used {
                return false;
            }
            let was_zero = s.count == 0;
            s.count = (s.count + release_count).min(s.max_count);
            if was_zero && s.count > 0 {
                kwait::kwait_wake(&WaitReason::Semaphore { sem_id });
            }
            return true;
        }
        false
    }

    pub fn wait_decrement(&mut self, sem_id: u32) -> bool {
        if let Some(s) = self.semaphores.get_mut(sem_id as usize) {
            if !s.used {
                return false;
            }
            if s.count > 0 {
                s.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn is_used(&self, sem_id: u32) -> bool {
        self.semaphores.get(sem_id as usize).map_or(false, |s| s.used)
    }

    pub fn get_count(&self, sem_id: u32) -> i32 {
        self.semaphores.get(sem_id as usize).map_or(-1, |s| if s.used { s.count } else { -1 })
    }
}

static SEMAPHORE_MANAGER: Mutex<SemaphoreManager> = Mutex::new(SemaphoreManager::new());

pub struct SemaphoreObOps;

impl ObOperations for SemaphoreObOps {
    fn on_destroy(&self, _id: ObId, native_id: u64) {
        SEMAPHORE_MANAGER.lock().free(native_id as u32);
    }
}

pub static SEMAPHORE_OPS: SemaphoreObOps = SemaphoreObOps;

pub fn alloc_semaphore(ob_id: ObId, initial_count: i32, max_count: i32) -> Option<u32> {
    SEMAPHORE_MANAGER.lock().alloc(ob_id, initial_count, max_count)
}

pub fn free_semaphore(sem_id: u32) {
    SEMAPHORE_MANAGER.lock().free(sem_id);
}

pub fn release_semaphore(sem_id: u32, release_count: i32) -> bool {
    SEMAPHORE_MANAGER.lock().release(sem_id, release_count)
}

pub fn try_wait_semaphore(sem_id: u32) -> bool {
    SEMAPHORE_MANAGER.lock().wait_decrement(sem_id)
}

pub fn get_semaphore_count(sem_id: u32) -> i32 {
    SEMAPHORE_MANAGER.lock().get_count(sem_id)
}

pub fn register_semaphore_tests() {
    use crate::{test_case, test_eq, test_true};

    test_case!("semaphore_alloc_free", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 0, 5).unwrap();
        test_eq!(id, 0);
        let id2 = mgr.alloc(43, 3, 10).unwrap();
        test_eq!(id2, 1);
        mgr.free(id);
        let id3 = mgr.alloc(44, 1, 5).unwrap();
        test_eq!(id3, 0);
        mgr.free(id2);
        mgr.free(id3);
    });

    test_case!("semaphore_invalid_params", {
        let mut mgr = SemaphoreManager::new();
        test_true!(mgr.alloc(42, -1, 5).is_none());
        test_true!(mgr.alloc(42, 0, 0).is_none());
        test_true!(mgr.alloc(42, 5, 3).is_none());
    });

    test_case!("semaphore_wait_decrements", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 3, 5).unwrap();
        test_true!(mgr.wait_decrement(id));
        test_eq!(mgr.get_count(id), 2);
        test_true!(mgr.wait_decrement(id));
        test_eq!(mgr.get_count(id), 1);
        test_true!(mgr.wait_decrement(id));
        test_eq!(mgr.get_count(id), 0);
        mgr.free(id);
    });

    test_case!("semaphore_wait_when_zero", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 0, 5).unwrap();
        test_true!(!mgr.wait_decrement(id));
        mgr.free(id);
    });

    test_case!("semaphore_release_increments", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 0, 5).unwrap();
        test_true!(mgr.release(id, 1));
        test_eq!(mgr.get_count(id), 1);
        test_true!(mgr.release(id, 2));
        test_eq!(mgr.get_count(id), 3);
        mgr.free(id);
    });

    test_case!("semaphore_release_caps_at_max", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 3, 5).unwrap();
        test_true!(mgr.release(id, 10));
        test_eq!(mgr.get_count(id), 5);
        mgr.free(id);
    });

    test_case!("semaphore_release_negative_fails", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 0, 5).unwrap();
        test_true!(!mgr.release(id, 0));
        test_true!(!mgr.release(id, -1));
        mgr.free(id);
    });

    test_case!("semaphore_full_lifecycle", {
        let mut mgr = SemaphoreManager::new();
        let id = mgr.alloc(42, 2, 3).unwrap();
        // Consume both initial counts
        test_true!(mgr.wait_decrement(id));
        test_true!(mgr.wait_decrement(id));
        test_true!(!mgr.wait_decrement(id));
        // Release
        test_true!(mgr.release(id, 1));
        test_true!(mgr.wait_decrement(id));
        test_true!(!mgr.wait_decrement(id));
        mgr.free(id);
    });
}
