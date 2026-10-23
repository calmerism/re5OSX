//! The debug build's boot screen: when its render thread starts, the engine
//! draws one frame on dark blue ("Now loading Grand Theft Auto V", "x64
//! BankRelease", the workstation's IP, the command line), which stays on
//! screen until the legal screen's first frame. A retail build has none.
//! `CSystem::RenderThreadInit` draws it only when `CSystem::BeginRender`
//! starts a frame, so within that call the frame is not started: the block
//! is skipped whole, and the rest of the thread's start goes on as before.

use std::cell::Cell;

use re5osx_game::{Env, orig};

thread_local! {
    static IN_RENDER_THREAD_INIT: Cell<bool> = const { Cell::new(false) };
}

/// Hook for `CSystem::RenderThreadInit()`.
unsafe extern "C" fn render_thread_init(env: *mut Env) {
    IN_RENDER_THREAD_INIT.set(true);
    unsafe { orig::render_thread_init(env) };
    IN_RENDER_THREAD_INIT.set(false);
}

/// Hook for `CSystem::BeginRender()`: no frame for the boot screen.
unsafe extern "C" fn begin_render(env: *mut Env) -> u32 {
    if IN_RENDER_THREAD_INIT.get() {
        return 0;
    }
    unsafe { orig::begin_render(env) }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.render_thread_init = Some(render_thread_init);
    hooks.begin_render = Some(begin_render);
}
