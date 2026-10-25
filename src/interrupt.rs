//! Abort checkpoints inside long builtin work (spec §8.3).
//!
//! Before running a builtin or operator, the runtime arms this thread-local
//! with the earliest real-time deadline of the current task's scope chain and
//! the memory quota. Long loops (JSON parse/dump, equality, schema checks,
//! collection building, splitting) call [`poll`] once per item; every 1024
//! polls it checks the clock and the heap. When tripped, loops may stop early;
//! the runtime discards the partial result and turns the trip into the abort.

use std::cell::Cell;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trip {
    Deadline,
    Memory,
}

thread_local! {
    static AT: Cell<Option<Instant>> = const { Cell::new(None) };
    static MEM: Cell<Option<(usize, u64)>> = const { Cell::new(None) };
    static COUNT: Cell<u32> = const { Cell::new(0) };
    static TRIPPED: Cell<Option<Trip>> = const { Cell::new(None) };
}

/// Arm checkpoints: optional deadline instant and (baseline, limit) memory quota.
pub fn arm(at: Option<Instant>, mem: Option<(usize, u64)>) {
    AT.with(|c| c.set(at));
    MEM.with(|c| c.set(mem));
    TRIPPED.with(|c| c.set(None));
}

/// Disarm and return the trip, if any.
pub fn disarm() -> Option<Trip> {
    AT.with(|c| c.set(None));
    MEM.with(|c| c.set(None));
    TRIPPED.with(|c| c.replace(None))
}

/// One unit of work. Returns true once tripped (callers may stop early).
#[inline]
pub fn poll() -> bool {
    let n = COUNT.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n
    });
    if n & 1023 == 0 {
        check();
    }
    tripped()
}

#[inline]
pub fn tripped() -> bool {
    TRIPPED.with(|c| c.get().is_some())
}

fn check() {
    if tripped() {
        return;
    }
    if let Some(at) = AT.with(|c| c.get()) {
        if Instant::now() >= at {
            TRIPPED.with(|c| c.set(Some(Trip::Deadline)));
            return;
        }
    }
    if let Some((base, limit)) = MEM.with(|c| c.get()) {
        if crate::alloc::live_bytes().saturating_sub(base) as u64 > limit {
            TRIPPED.with(|c| c.set(Some(Trip::Memory)));
        }
    }
}
