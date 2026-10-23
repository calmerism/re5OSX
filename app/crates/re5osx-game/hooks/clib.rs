// Hooks for the runtime's `clib.rs` (see ../hooks.rs).
&[
    // The C library's decimal parser (the game data's numbers at boot,
    // Scaleform's string-to-number): a native one for plain decimals.
    ("strtod", "strtod"),
    // zlib 1.2.3's inflate (archive files, scripts, Scaleform movies, the
    // prologue's resources, saves): zlib-rs for its 32 KB-window streams.
    ("inflate", "inflate"),
    ("inflate_end", "inflateEnd"),
]
