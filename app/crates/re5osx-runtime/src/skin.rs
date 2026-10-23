//! The peds' skin pass at PostFX Ultra.
//!
//! `DeferredLighting::ExecuteSkinLightPass` copies the peds' lighting into
//! the scene with a pass of the `sss_skin` technique chosen from the
//! next-gen skin blur (on by default) and the PostFX level: at Ultra it asks
//! for a sixth pass, which the game's shaders do not have (five: the code is
//! a later revision than the data). Asked for a pass it does not have, the
//! engine skips the skin pass and peds stay black.
//!
//! The game's files stay as installed: instead, while the skin pass runs,
//! `CSettingsManager::GetSettings` on its thread returns a copy of the
//! settings with PostFX at Very High, so it draws the pass Very High draws
//! (the next-gen copy pass). The real settings, read by other threads
//! meanwhile, are never touched.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use re5osx_game::{Env, exports, orig};

use crate::memory::Mem;

/// `Settings::m_graphics.m_PostFX`: 0 Normal, 1 High, 2 Very High, 3 Ultra.
const POSTFX: u64 = 148;
const VERY_HIGH: u32 = 2;
/// Bytes of settings copied: the whole `Settings` and then some.
const SETTINGS_BYTES: u64 = 1024;

/// The settings manager (a singleton), seen at its first `GetSettings`.
static MANAGER: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The settings `GetSettings` returns on this thread instead (0: none).
    static VIEW: Cell<u64> = const { Cell::new(0) };
}

/// Hook for `CSettingsManager::GetSettings()`.
pub(crate) unsafe extern "C" fn get_settings(env: *mut Env, this: u64) -> u64 {
    MANAGER.store(this, Ordering::Relaxed);
    match VIEW.get() {
        0 => unsafe { orig::get_settings(env, this) },
        view => view,
    }
}

/// Hook for `DeferredLighting::ExecuteSkinLightPass(grcRenderTarget*, bool)`.
pub(crate) unsafe extern "C" fn skin_light_pass(env: *mut Env, target: u64, flag: u32) {
    let manager = MANAGER.load(Ordering::Relaxed);
    let mem = unsafe { Mem::of(env) };
    let settings = if manager != 0 { unsafe { orig::get_settings(env, manager) } } else { 0 };
    if settings == 0 || mem.u32(settings + POSTFX) <= VERY_HIGH {
        return unsafe { orig::skin_light_pass(env, target, flag) };
    }
    // The copy lives on the engine thread's wasm stack for the call.
    let Ok(top) = (unsafe { exports::emscripten_stack_get_current(env) }) else {
        return unsafe { orig::skin_light_pass(env, target, flag) };
    };
    let Ok(copy) = (unsafe { exports::_emscripten_stack_alloc(env, SETTINGS_BYTES) }) else {
        return unsafe { orig::skin_light_pass(env, target, flag) };
    };
    let len = SETTINGS_BYTES as usize;
    let real = mem.bytes(settings, len).to_vec();
    mem.bytes_mut(copy, len).copy_from_slice(&real);
    mem.set_u32(copy + POSTFX, VERY_HIGH);
    VIEW.set(copy);
    unsafe { orig::skin_light_pass(env, target, flag) };
    VIEW.set(0);
    let _ = unsafe { exports::_emscripten_stack_restore(env, top) };
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.skin_light_pass = Some(skin_light_pass);
    hooks.get_settings = Some(get_settings);
}
