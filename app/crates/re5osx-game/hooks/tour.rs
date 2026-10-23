// Hooks for the runtime's `tour.rs` (see ../hooks.rs).
&[
    // The map tour of automated runs (tour.rs): the warp manager's update,
    // once a frame on the main thread, moves the player with the engine's
    // teleport and sets the hour and the weather.
    ("warp_update", "CWarpManager::Update()"),
    ("find_player_ped", "FindPlayerPed()"),
    ("warp", "CWarpManager::SetWarp(rage::Vector3 const&, rage::Vector3 const&, float, bool, bool, float, bool)"),
    ("clock_set_time", "CClock::SetTime(int, int, int)"),
    ("weather_index", "CWeather::GetTypeIndex(unsigned int)"),
    ("weather_force_now", "CWeather::ForceTypeNow(int, unsigned int)"),
    // The highest point of the map at x, y (buildings included; the sea's
    // is below its surface): stops given without a height start above it.
    ("height_max", "CGameWorldHeightMap::GetMaxHeightFromWorldHeightMap(float, float)"),
]
