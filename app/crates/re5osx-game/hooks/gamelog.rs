// Hooks for the runtime's `gamelog.rs` (see ../hooks.rs).
&[
    // The network's logs, flushed by the session's shutdown (loading a
    // save, a new game): their spool thread runs only online.
    ("net_log_flush", "rage::netLogImpl::Flush(bool)"),
]
