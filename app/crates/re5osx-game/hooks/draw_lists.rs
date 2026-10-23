// Hooks for the runtime's `draw_lists.rs` (see ../hooks.rs).
&[
    // Asked of every entity before its draw handler adds it to a draw list
    // (peds, vehicles, fragments, objects): the host also skips skinned ones
    // when the frame's budgets for skeletons are spent.
    ("draw_skip_entity", "CEntityDrawHandler::ShouldSkipEntity(CEntity const*)"),
]
