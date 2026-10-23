// Hooks for the runtime's `upscale.rs` (see ../hooks.rs).
&[
    // Post-processing has drawn the scene into the back buffer; the HUD
    // comes after. The host marks the place in the GPU ring.
    ("postfx_nondepth", "PostFX::ProcessNonDepthFX()"),
    ("gpu_emit", "wasm_gpu::Emit(unsigned int, unsigned int const*, unsigned int)"),
]
