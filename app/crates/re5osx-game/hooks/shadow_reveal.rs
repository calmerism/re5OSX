// Hooks for the runtime's `shadow_reveal.rs` (see ../hooks.rs).
&[
    // The cascade shadows' cloud shadows: the code sets globals the game's
    // `cascadeshadows_rendering` effect has as locals (see
    // shadow_reveal.rs; it sets the textures with device_reset's
    // `effect_set_texture`).
    ("lookup_global_var", "rage::grcEffect::LookupGlobalVar(char const*, bool)"),
    ("set_global_texture", "rage::grcEffect::SetGlobalVar(rage::grcEffectGlobalVar__, rage::grcTexture const*)"),
    ("set_global_floats", "rage::grcEffect::SetGlobalFloatCommon(int, float*, int, rage::grcEffect::VarType)"),
    ("shadow_reveal_set_vars", "ShadowRevealSetup::SetVars(CCSMGlobals const&, rage::grcTexture const*, rage::grcTexture const*, rage::Vector4 const&) const"),
    ("effect_lookup_var", "rage::grcEffect::LookupVar(char const*, bool) const"),
    ("effect_set_local", "rage::grcEffect::SetLocalCommon(rage::grcInstanceData&, rage::grcEffectVar__, void const*, int, int, bool) const"),
]
