//! A device reset (the window resized): what it frees while still in use.
//!
//! `grcDevice::DeviceLost` (render thread, between frames) destroys the
//! screen-sized render targets, with them the staging copies some keep for
//! the CPU, and releases the resource cache; new targets are made later, on
//! the main thread. Three things still point at what it frees:
//!
//! - A readback in flight. The game's D3D layer frees a staging texture's
//!   memory before waiting for the readback into it (the replay thumbnail's
//!   was seen in flight), and the GPU worker then writes the pixels
//!   into freed heap: a later `malloc` or `free` crashes. The engine's own
//!   GPU fence first lets every readback asked so far land.
//! - The 2D sprite shader's depth texture. `CVisualEffects::RenderCoronas`
//!   gives it the scene's depth target (`CSprite2d::SetDepthTexture`) only
//!   when it draws coronas a certain way, so the shader can hold it for a
//!   long time; every technique of that shader binds it, the minimap's
//!   among them, which then asks the destroyed target for its view. The
//!   shader is given no depth texture instead (an empty one is not bound)
//!   until the coronas give it the new one.
//! - SSAO's blur source. `SSAO::ProcessQS` draws the ambient occlusion,
//!   then blurs it between two targets of its own, setting the shader's
//!   `deferredLightTexture0P` to the source before each blur. The occlusion
//!   pass binds that variable too, so it binds what the previous frame's
//!   last blur left: after a reset, a target `SSAO::DeviceLost` has
//!   destroyed, whose view it asks for. On PC freed memory keeps the
//!   object's vtable and nothing shows; here the heap reuses it, and the
//!   call jumped to garbage (the game crashed when the window was resized
//!   with SSAO on). The variable is emptied until the next blur sets it.

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// SSAO's shader (a `grmShader*`; a shader starts with its instance data)
/// and its blur source variable's handle, at these addresses in this build.
const SSAO_SHADER: u64 = 21395544;
const SSAO_BLUR_SOURCE: u64 = 21395588;
const BLUR_SOURCE_NAME: &[u8] = b"deferredLightTexture0P\0";

/// `grmShader::m_Effect`.
const SHADER_EFFECT: u64 = 8;
/// `grcEffect`'s variables: an array of `grcParameter` and its count.
const EFFECT_VARS: u64 = 16;
const EFFECT_VAR_COUNT: u64 = 24;
/// A `grcParameter`'s size and where its name points.
const VAR_SIZE: u64 = 72;
const VAR_NAME: u64 = 16;

/// Hook for `rage::grcDevice::DeviceLost()`.
pub(crate) unsafe extern "C" fn device_lost(env: *mut Env) {
    unsafe {
        orig::gpu_fence(env);
        orig::device_lost(env);
        orig::sprite_depth_texture(env, 0);
        match ssao_blur_source(Mem::of(env), crate::rt().memory.size()) {
            Some((shader, var)) => orig::effect_set_texture(env, shader, var, 0),
            None => log::warn!("device reset: SSAO's blur source not found, left as it is"),
        }
    }
}

/// SSAO's shader and its blur source's handle, once the variable that
/// handle names is checked to be the blur source: a game build with other
/// addresses gets nothing written.
fn ssao_blur_source(mem: Mem, size: u64) -> Option<(u64, u32)> {
    let within = |addr: u64, len: u64| addr != 0 && addr.checked_add(len).is_some_and(|end| end <= size);
    let shader = mem.u64(SSAO_SHADER);
    let effect = within(shader, SHADER_EFFECT + 8).then(|| mem.u64(shader + SHADER_EFFECT))?;
    within(effect, EFFECT_VAR_COUNT + 2).then_some(())?;
    let var = mem.u32(SSAO_BLUR_SOURCE);
    if var == 0 || var > u32::from(mem.u16(effect + EFFECT_VAR_COUNT)) {
        return None;
    }
    let param = mem.u64(effect + EFFECT_VARS) + u64::from(var - 1) * VAR_SIZE;
    let name = within(param, VAR_SIZE).then(|| mem.u64(param + VAR_NAME))?;
    let len = BLUR_SOURCE_NAME.len();
    (within(name, len as u64) && mem.bytes(name, len) == BLUR_SOURCE_NAME).then_some((shader, var))
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.device_lost = Some(device_lost);
}
