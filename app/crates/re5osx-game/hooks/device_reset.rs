// Hooks for the runtime's `device_reset.rs` (see ../hooks.rs).
&[
    // A device reset (the window resized) frees staging memory a readback
    // may still be writing and render targets shaders may still hold (the
    // 2D sprite shader's depth, SSAO's blur source): the host waits for the
    // GPU first and takes those textures away after (device_reset.rs).
    ("device_lost", "rage::grcDevice::DeviceLost()"),
    ("gpu_fence", "wasm_gpu::Fence()"),
    ("sprite_depth_texture", "CSprite2d::SetDepthTexture(rage::grcTexture*)"),
    ("effect_set_texture", "rage::grcEffect::SetVar(rage::grcInstanceData&, rage::grcEffectVar__, rage::grcTexture const*)"),
]
