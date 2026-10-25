//! Process-wide counting allocator used for the `memory_bytes` quota.
//!
//! Accounting strategy (published per spec §14.1): the runtime measures live
//! heap bytes of the whole process at evaluation checkpoints (every 512
//! semantic steps and inside long builtin loops) and attributes an excess over
//! the baseline recorded at runtime creation to the root/request evaluation
//! scope that is running when the excess is observed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            LIVE.fetch_add(new_size, Ordering::Relaxed);
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        p
    }
}

/// Live heap bytes (0 when the counting allocator is not installed).
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}
