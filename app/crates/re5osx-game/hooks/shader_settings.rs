// Hooks for the runtime's `shader_settings.rs` (see ../hooks.rs).
&[
    // MSAA stays off and shader quality at High or above: the game's shaders
    // cannot draw the rest (see shader_settings.rs). The settings loaded from
    // the file and those the menu asks for; the manager, for the host.
    ("settings_load", "CSettingsManager::Load()"),
    ("settings_request", "CSettingsManager::RequestNewSettings(Settings)"),
    ("settings_instance", "CSettingsManager::GetInstance()"),
]
