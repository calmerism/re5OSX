//! Where the scene ends and the HUD begins, for the renderer's temporal
//! upscaling: post-processing (`PostFX::ProcessNonDepthFX`, render thread)
//! leaves the finished scene in the back buffer, and everything drawn after
//! it is HUD. A host command in the GPU ring marks the place.

use re5osx_game::{Env, orig};

use crate::gpu::op;

/// Hook for `PostFX::ProcessNonDepthFX()`.
pub(crate) unsafe extern "C" fn postfx_nondepth(env: *mut Env) {
    unsafe {
        orig::postfx_nondepth(env);
        orig::gpu_emit(env, op::SCENE_DONE, 0, 0);
    }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.postfx_nondepth = Some(postfx_nondepth);
}
