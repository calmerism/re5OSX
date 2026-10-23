//! MSAA stays off and shader quality at High or above.
//!
//! The game's shaders are its DirectX 10 set (`win32_40`), whose multisampled
//! effects are an older revision than the code that drives them: the code
//! asks `imMS`'s depth resolve for a fourth pass it does not have, indexes
//! `postfxMS`'s technique as `postfx`'s (ten passes longer), and dispatches a
//! multisampled depth reduction of the tiled lighting that has no
//! translation. The game's D3D11 layer also makes x2 targets single-sampled
//! (WebGPU has no x2). With MSAA on, `grcEffect::BeginPass` traps on the
//! missing pass and the render thread stops at the first frame of a scene;
//! past that, post-processing would draw the wrong passes into a black image.
//!
//! Shader quality Normal loads the effects from `win32_40_lq`, older still:
//! its `postfx` lacks `DistortionParams`, which the engine requires (it
//! stops at its first frames), two effects it preloads are missing, and only
//! 3,961 of its 10,762 programs have a translation (all but 30 of its 377
//! effects lack some; `win32_40`: 10,246 of 10,941).
//!
//! The engine turns MSAA off itself on the DirectX 10 feature set (`-DX10`).
//! Here the settings it loads and those its menu asks for have MSAA off and
//! shader quality at High at least. The menu saves a change that needs a
//! restart (shader quality) as picked, then restarts: the next load raises
//! it. The game's files stay as installed.

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// `Settings::m_graphics.m_MSAA`: 0 off, else the sample count.
const MSAA: u64 = 60;
/// `Settings::m_graphics.m_ShaderQuality`: 0 Normal, 1 High, 2 Very High.
const SHADER_QUALITY: u64 = 92;
const HIGH: u32 = 1;

fn limit(env: *mut Env, settings: u64) {
    let mem = unsafe { Mem::of(env) };
    let samples = mem.u32(settings + MSAA);
    if samples != 0 {
        log::info!("game settings: MSAA x{samples} off (the game's shaders cannot draw it)");
        mem.set_u32(settings + MSAA, 0);
    }
    if mem.u32(settings + SHADER_QUALITY) < HIGH {
        log::info!("game settings: shader quality Normal -> High (the game's shaders lack Normal's)");
        mem.set_u32(settings + SHADER_QUALITY, HIGH);
    }
}

/// Hook for `CSettingsManager::Load()`: the settings file's.
pub(crate) unsafe extern "C" fn settings_load(env: *mut Env) {
    unsafe { orig::settings_load(env) };
    let manager = unsafe { orig::settings_instance(env) };
    limit(env, unsafe { orig::get_settings(env, manager) });
}

/// Hook for `CSettingsManager::RequestNewSettings(Settings)`: the menu's.
pub(crate) unsafe extern "C" fn settings_request(env: *mut Env, settings: u64) {
    limit(env, settings);
    unsafe { orig::settings_request(env, settings) }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.settings_load = Some(settings_load);
    hooks.settings_request = Some(settings_request);
}
