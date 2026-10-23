// Hooks for the runtime's `boot_screen.rs` (see ../hooks.rs).
&[
    // The render thread's start draws a frame of its own ("Now loading",
    // the build, the command line), between a BeginRender and an EndRender.
    ("render_thread_init", "CSystem::RenderThreadInit()"),
    ("begin_render", "CSystem::BeginRender()"),
]
