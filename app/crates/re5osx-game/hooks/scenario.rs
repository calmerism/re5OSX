// Hooks for the runtime's `scenario.rs` (see ../hooks.rs).
&[
    // A scenario point's overlap list is asked for as many entities as its
    // array holds and walked to the first null: the host keeps the last
    // slot null (scenario.rs).
    ("is_position_clear_for_ped", "CPedPlacement::IsPositionClearForPed(rage::Vector3 const&, float, bool, int, CEntity**, bool, bool, bool)"),
]
