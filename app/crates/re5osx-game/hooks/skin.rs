// Hooks for the runtime's `skin.rs` (see ../hooks.rs).
&[
    // At PostFX Ultra the skin pass asks for a pass the game's shaders do not
    // have; it is shown settings with PostFX at Very High (see skin.rs).
    ("skin_light_pass", "DeferredLighting::ExecuteSkinLightPass(rage::grcRenderTarget*, bool)"),
    ("get_settings", "CSettingsManager::GetSettings()"),
]
