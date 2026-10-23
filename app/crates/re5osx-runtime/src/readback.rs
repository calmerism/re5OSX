//! Readbacks land before the memory they land in is freed.
//!
//! A readback copies a GPU resource into a staging resource's CPU memory
//! (`wasm_null_d3d::GpuReadback`), and the GPU worker writes it there once
//! the GPU is done, frames later; the staging resource's flag word says
//! when. The game's D3D layer destroys a resource (`NullResource::
//! ~NullResource`, every resource type, called when its last reference
//! goes) by freeing that memory first and only then waiting for the flag
//! (`HasGpuId::WaitReadback`). A staging resource released with a readback
//! in flight therefore had its pixels written into freed heap: on an iPhone
//! 16 Pro the free list of malloc's 64-80 byte chunks took the same eight
//! bytes in every run, and a later `malloc` (the streaming thread placing a
//! texture) crashed on them about two minutes into the game. The host now
//! waits first; the destructor's own wait then finds nothing to wait for.

use std::sync::atomic::Ordering;

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// Where `HasGpuId` (the readback flag's owner) sits in every `NullResource`.
const HAS_GPU_ID: u64 = 24;
/// The flag in `HasGpuId`: 1 while a readback is in flight.
const IN_FLIGHT: u64 = 68;

/// Waits for the readback into resource `this`, if one is in flight.
unsafe fn wait(env: *mut Env, this: u64) {
    let mem = unsafe { Mem::of(env) };
    if mem.atomic_u32(this + HAS_GPU_ID + IN_FLIGHT).load(Ordering::Acquire) == 1 {
        log::debug!("readback: resource {this:#x} destroyed with a readback in flight: waiting for it");
        unsafe { orig::wait_readback(env, this + HAS_GPU_ID) };
    }
}

macro_rules! destructors {
    ($($dtor:ident, $delete:ident;)*) => {$(
        /// Hook for a `NullResource` destructor.
        pub(crate) unsafe extern "C" fn $dtor(env: *mut Env, this: u64) -> u64 {
            unsafe {
                wait(env, this);
                orig::$dtor(env, this)
            }
        }

        /// Hook for a `NullResource` deleting destructor.
        pub(crate) unsafe extern "C" fn $delete(env: *mut Env, this: u64) {
            unsafe {
                wait(env, this);
                orig::$delete(env, this)
            }
        }
    )*};
}

destructors! {
    buffer_dtor, buffer_delete;
    texture1d_dtor, texture1d_delete;
    texture2d_dtor, texture2d_delete;
    texture3d_dtor, texture3d_delete;
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.buffer_dtor = Some(buffer_dtor);
    hooks.buffer_delete = Some(buffer_delete);
    hooks.texture1d_dtor = Some(texture1d_dtor);
    hooks.texture1d_delete = Some(texture1d_delete);
    hooks.texture2d_dtor = Some(texture2d_dtor);
    hooks.texture2d_delete = Some(texture2d_delete);
    hooks.texture3d_dtor = Some(texture3d_dtor);
    hooks.texture3d_delete = Some(texture3d_delete);
}
