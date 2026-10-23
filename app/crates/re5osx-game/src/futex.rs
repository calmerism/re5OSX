//! `memory.atomic.wait32/wait64/notify`, shared by the module and host threads.
//!
//! Waiters park on the host address of the watched word, so a host thread
//! (GPU, I/O, audio) waiting on a word in wasm memory and a wasm thread
//! notifying it (or the reverse) meet in the same queue.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot_core::{DEFAULT_PARK_TOKEN, DEFAULT_UNPARK_TOKEN, FilterOp, ParkResult};

use crate::Memory;

/// Result of a wait, with the values wasm uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Wait {
    Woken = 0,
    NotEqual = 1,
    TimedOut = 2,
}

/// Waits while `*word == expected`, up to `timeout` (`None` = forever).
pub fn wait32(word: &AtomicU32, expected: u32, timeout: Option<Duration>) -> Wait {
    park(word as *const _ as usize, || word.load(Ordering::SeqCst) == expected, timeout)
}

/// Waits while `*word == expected`, up to `timeout` (`None` = forever).
pub fn wait64(word: &AtomicU64, expected: u64, timeout: Option<Duration>) -> Wait {
    park(word as *const _ as usize, || word.load(Ordering::SeqCst) == expected, timeout)
}

/// Wakes up to `count` waiters on `word`; returns how many were woken.
pub fn notify<T>(word: &T, count: u32) -> u32 {
    let key = word as *const T as usize;
    let mut left = count;
    let result = unsafe {
        parking_lot_core::unpark_filter(
            key,
            |_| {
                if left == 0 {
                    FilterOp::Stop
                } else {
                    left -= 1;
                    FilterOp::Unpark
                }
            },
            |_| DEFAULT_UNPARK_TOKEN,
        )
    };
    result.unparked_threads as u32
}

fn park(key: usize, validate: impl Fn() -> bool, timeout: Option<Duration>) -> Wait {
    // Windows waits in whole milliseconds (WaitOnAddress rounds up, then
    // the timer's tick): the engine's 20 µs pauses (`sysIpcYield`, as the
    // main thread waits for its jobs, many times a frame) took 1-2 ms
    // each. Those few yield to other threads instead, until the deadline
    // or a change (a notify that changes nothing just ends them later).
    #[cfg(windows)]
    if let Some(t) = timeout.filter(|t| *t < Duration::from_micros(100)) {
        // (The value first, as wasm's wait: a changed word is not-equal
        // even with no time to wait.)
        let deadline = Instant::now() + t;
        loop {
            if !validate() {
                return Wait::NotEqual;
            }
            if Instant::now() >= deadline {
                return Wait::TimedOut;
            }
            std::thread::yield_now();
        }
    }
    let deadline = timeout.map(|t| Instant::now() + t);
    match unsafe { parking_lot_core::park(key, validate, || {}, |_, _| {}, DEFAULT_PARK_TOKEN, deadline) } {
        ParkResult::Unparked(_) => Wait::Woken,
        ParkResult::Invalid => Wait::NotEqual,
        ParkResult::TimedOut => Wait::TimedOut,
    }
}

fn timeout(ns: i64) -> Option<Duration> {
    (ns >= 0).then(|| Duration::from_nanos(ns as u64))
}

unsafe fn word<T>(memory: *mut Memory, addr: u64) -> &'static T {
    unsafe { &*((*memory).data.add(addr as usize) as *const T) }
}

// Called by the generated code (through re5osx.c).
pub(crate) unsafe extern "C" fn wasm_wait32(memory: *mut Memory, addr: u64, expected: u32, ns: i64) -> u32 {
    wait32(unsafe { word(memory, addr) }, expected, timeout(ns)) as u32
}

pub(crate) unsafe extern "C" fn wasm_wait64(memory: *mut Memory, addr: u64, expected: u64, ns: i64) -> u32 {
    wait64(unsafe { word(memory, addr) }, expected, timeout(ns)) as u32
}

pub(crate) unsafe extern "C" fn wasm_notify(memory: *mut Memory, addr: u64, count: u32) -> u32 {
    notify::<AtomicU32>(unsafe { word(memory, addr) }, count)
}
