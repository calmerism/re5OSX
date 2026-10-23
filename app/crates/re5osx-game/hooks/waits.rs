// Hooks for the runtime's `waits.rs` (see ../hooks.rs).
&[
    // Waits the engine polls with 1 ms sleeps: for the visibility scan's
    // jobs, and for the GPU before a frame's drawing.
    ("poll_event", "rage::sysIpcPollEvent(rage::sysIpcEventTag*)"),
    ("begin_draw", "rage::grcSetup::BeginDraw(bool)"),
    ("is_fence_pending", "rage::grcDevice::IsFencePending(rage::__grcFenceHandle*, unsigned int)"),
    // The audio engine's thread waits for the render thread's next frame
    // (15 ms at most) by polling with 1 ms sleeps: its thread, the time its
    // frame began, the sleep, and the frame's end.
    ("audio_engine_thread", "rage::audEngine::ThreadEntryProc(void*)"),
    ("timer_ms", "rage::sysPerformanceTimer::GetTimeMS()"),
    ("ipc_sleep", "rage::sysIpcSleep(unsigned int)"),
    ("end_draw", "rage::grcSetup::EndDraw()"),
]
