// Hooks for the runtime's `exit.rs` (see ../hooks.rs).
&[
    // The engine's way out: asked by the main loop whether to leave it and
    // shut down ("Exit Game"), and the first step of the restart some
    // graphics settings need. The shutdown never finishes in this port; the
    // host ends the process instead.
    ("want_to_exit", "CSystem::WantToExit()"),
    ("rl_shutdown", "rage::rlShutdown()"),
]
