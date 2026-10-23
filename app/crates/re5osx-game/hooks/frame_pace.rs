// Hooks for the runtime's `frame_pace.rs` (see ../hooks.rs).
&[
    // The end of a frame on the render thread, with the engine's frame
    // limiter: the host waits for the limiter's deadline itself.
    ("end_render", "CSystem::EndRender()"),
]
