// Hooks for the runtime's `stats.rs` (see ../hooks.rs).
&[
    // Stats the game's data lacks (the scripts are newer), kept by the host:
    // the engine's lookups, the packed stats and the scripts' stat commands.
    ("stat_find", "CStatsDataMgr::StatIterFind(StatId const&, rage::atInBinaryMap<sStatDataPtr, StatId>::Iterator&)"),
    ("packed_key", "packed_stats::GetStatKey(StatId&, int, int)"),
    ("packed_get", "packed_stats::GetPackedValue(int&, int, int)"),
    ("packed_set", "packed_stats::SetPackedValue(int, int, int)"),
    ("stat_get_int", "stats_commands::CommandStatGetInt(int, int&, int)"),
    ("stat_get_float", "stats_commands::CommandStatGetFloat(int, float&, int)"),
    ("stat_get_bool", "stats_commands::CommandStatGetBool(int, int&, int)"),
    ("stat_get_masked_int", "stats_commands::CommandStatGetMaskedInt(int, int&, int, int, int)"),
    ("stat_set_int", "stats_commands::CommandStatSetInt(int, int, bool)"),
    ("stat_set_float", "stats_commands::CommandStatSetFloat(int, float, bool)"),
    ("stat_set_bool", "stats_commands::CommandStatSetBool(int, bool, bool)"),
    ("stat_set_masked_int", "stats_commands::CommandStatSetMaskedInt(int, int, int, int, bool)"),
    ("stat_delete_slot", "stats_commands::CommandStatDeleteSlot(int)"),
]
