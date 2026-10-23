//! Waits the engine makes by polling with 1 ms sleeps (which Apple's and
//! Android's timers make 1.2-1.7 ms), made real waits: the thread goes on
//! as soon as what it waits for happens, and does not wake up a thousand
//! times a second meanwhile.
//!
//! - The main thread waits for the visibility scan's jobs while it builds
//!   the frame's render lists (`CViewportManager::BuildRenderLists`, "Wait
//!   for more PVS entries"): `if (!sysIpcPollEvent(scan)) sysIpcSleep(1)`,
//!   each job setting the event as it ends.
//! - The render thread waits for the GPU to finish an earlier frame before
//!   it draws the next (`grcSetup::BeginDraw`, "Wait on queued frames"):
//!   `while (IsFencePending(fence)) sysIpcSleep(1)`.
//! - The audio engine's thread waits for the render thread's next frame,
//!   15 ms after its own frame began at most (`audEngine::ThreadEntryProc`):
//!   `seen = frames; while (elapsed <= 15 - t) { sysIpcSleep(1); if (frames
//!   != seen) break; }`, 700-850 wakeups a second all game long.

use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use re5osx_game::{Env, futex, orig};

use crate::memory::Mem;

/// The visibility scan's event (a pointer to it): each `fwScanEntities` job
/// sets it as it ends. An address in this build.
const SCAN_EVENT: u64 = 11497192;
/// How long a wait lasts at most before the engine's loop looks again (as
/// its own sleep did).
const SCAN_WAIT: Duration = Duration::from_millis(1);
/// How long the render thread waits for the GPU here at most, before the
/// engine's own polling takes over.
const GPU_WAIT: Duration = Duration::from_millis(100);

/// Hook for `rage::sysIpcPollEvent(sysIpcEventTag*)`: for the scan's event,
/// waits for it (up to [`SCAN_WAIT`]) as `sysIpcWaitEvent` does. The event
/// is a state word (1: set; taken back to 0) and a count of waiters at +4,
/// for which `sysIpcSetEvent` wakes the word. It answers set when it did
/// not come either: the caller looks at its conditions again, and would
/// only have slept.
pub(crate) unsafe extern "C" fn poll_event(env: *mut Env, event: u64) -> u32 {
    let mem = unsafe { Mem::of(env) };
    if event == 0 || event != mem.u64(SCAN_EVENT) {
        return unsafe { orig::poll_event(env, event) };
    }
    let (state, waiters) = (mem.atomic_u32(event), mem.atomic_u32(event + 4));
    waiters.fetch_add(1, Ordering::SeqCst);
    let until = Instant::now() + SCAN_WAIT;
    while state.compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        let Some(left) = until.checked_duration_since(Instant::now()) else { break };
        futex::wait32(state, 0, Some(left));
    }
    waiters.fetch_sub(1, Ordering::SeqCst);
    1
}

thread_local! {
    /// The render thread is in `grcSetup::BeginDraw`.
    static BEGIN_DRAW: Cell<bool> = const { Cell::new(false) };
}

/// Hook for `rage::grcSetup::BeginDraw(bool)`.
pub(crate) unsafe extern "C" fn begin_draw(env: *mut Env, setup: u64, flag: u32) -> u32 {
    BEGIN_DRAW.set(true);
    let r = unsafe { orig::begin_draw(env, setup, flag) };
    BEGIN_DRAW.set(false);
    r
}

/// Hook for `rage::grcDevice::IsFencePending(__grcFenceHandle*, unsigned)`:
/// in `BeginDraw`, waits for the fence (the GPU's completions wake it, see
/// `gpu::store_query`) rather than answering pending to a 1 ms sleep.
/// Elsewhere (a poll each frame) it answers at once.
pub(crate) unsafe extern "C" fn is_fence_pending(env: *mut Env, fence: u64) -> u32 {
    if !BEGIN_DRAW.get() {
        return unsafe { orig::is_fence_pending(env, fence) };
    }
    let until = Instant::now() + GPU_WAIT;
    loop {
        let seen = crate::gpu::COMPLETED.load(Ordering::Acquire);
        let pending = unsafe { orig::is_fence_pending(env, fence) };
        let left = until.checked_duration_since(Instant::now());
        match left {
            Some(left) if pending != 0 => {
                // (A completion missed between the two is caught within 1 ms.)
                futex::wait32(&crate::gpu::COMPLETED, seen, Some(left.min(Duration::from_millis(1))));
            }
            _ => return pending,
        }
    }
}

/// `grcSetup::EndDraw`'s frame counter, the audio thread's copy of it, and
/// the audio engine's paused flag (addresses in this build).
const RENDER_FRAMES: u64 = 6638636;
const AUDIO_SEEN: u64 = 12046712;
const AUDIO_PAUSED: u64 = 12045880;
/// The audio thread's frame, at most (ms).
const AUDIO_FRAME: f32 = 15.0;
/// After the engine's deadline: its timer starts just after our clock.
const AUDIO_MARGIN: Duration = Duration::from_micros(20);

thread_local! {
    /// The audio engine's thread.
    static AUDIO: Cell<bool> = const { Cell::new(false) };
    /// When its frame began: the timer's ms then, and our clock.
    static AUDIO_BEGAN: Cell<Option<(f32, Instant)>> = const { Cell::new(None) };
}

/// Hook for `rage::audEngine::ThreadEntryProc(void*)`: the thread's loop.
pub(crate) unsafe extern "C" fn audio_engine_thread(env: *mut Env, arg: u64) {
    AUDIO.set(true);
    unsafe { orig::audio_engine_thread(env, arg) }
}

/// Hook for `rage::sysPerformanceTimer::GetTimeMS()`: on the audio thread,
/// where its frame's wait begins.
pub(crate) unsafe extern "C" fn timer_ms(env: *mut Env, timer: u64) -> f32 {
    let ms = unsafe { orig::timer_ms(env, timer) };
    if AUDIO.get() {
        AUDIO_BEGAN.set(Some((ms, Instant::now())));
    }
    ms
}

/// Hook for `rage::sysIpcSleep(unsigned int)`: the audio thread's 1 ms
/// polls for the next render frame wait for it (or for the engine's
/// deadline) instead; its loop then finds the same: the counter changed,
/// or the time up. Other sleeps sleep.
pub(crate) unsafe extern "C" fn ipc_sleep(env: *mut Env, ms: u32) {
    if ms == 1 && AUDIO.get() {
        let mem = unsafe { Mem::of(env) };
        if let Some((t, at)) = AUDIO_BEGAN.take()
            && mem.u8(AUDIO_PAUSED) != 1
        {
            let until = at + Duration::from_secs_f32((AUDIO_FRAME - t).clamp(0.0, AUDIO_FRAME) / 1000.0) + AUDIO_MARGIN;
            if let Some(left) = until.checked_duration_since(Instant::now()) {
                futex::wait32(mem.atomic_u32(RENDER_FRAMES), mem.u32(AUDIO_SEEN), Some(left));
                return;
            }
        }
    }
    unsafe { orig::ipc_sleep(env, ms) }
}

/// Hook for `rage::grcSetup::EndDraw()`: a frame drawn, the counter moved.
pub(crate) unsafe extern "C" fn end_draw(env: *mut Env, setup: u64) {
    unsafe { orig::end_draw(env, setup) };
    futex::notify(unsafe { Mem::of(env) }.atomic_u32(RENDER_FRAMES), u32::MAX);
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.audio_engine_thread = Some(audio_engine_thread);
    hooks.timer_ms = Some(timer_ms);
    hooks.ipc_sleep = Some(ipc_sleep);
    hooks.end_draw = Some(end_draw);
    hooks.poll_event = Some(poll_event);
    hooks.begin_draw = Some(begin_draw);
    hooks.is_fence_pending = Some(is_fence_pending);
}
