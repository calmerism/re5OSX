// Hooks for the runtime's `blips.rs` (see ../hooks.rs).
&[
    // A blip's parameters, its sprite and display among them.
    ("blip_parameter", "CMiniMap::SetBlipParameter(eBLIP_PARAMS, int, int)"),
    // A blip by its id, and its display (kept while it is hidden).
    ("blip_from_id", "CMiniMap::GetBlip(int)"),
    ("blip_display", "CMiniMap::GetBlipDisplayValue(CMiniMapBlip*)"),
]
