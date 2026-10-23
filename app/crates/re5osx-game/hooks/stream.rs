// Hooks for the runtime's `stream.rs` (see ../hooks.rs).
&[
    // A read of a /game file from the engine's HTTP file device (the browser
    // build's; the runtime serves it from the stream's copy).
    ("httpfs_pread", "wasm_httpfs_pread"),
]
