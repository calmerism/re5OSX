//! wasm2c's name mangling (wabt src/c-writer.cc, CWriter::Mangle), so the
//! build can name generated symbols without parsing the C.

/// Symbol of a defined function or export of the module named `game`.
pub fn game_symbol(name: &str) -> String {
    format!("w2c_game_{}", mangle(name, false))
}

/// Symbol of an imported function.
pub fn import_symbol(module: &str, name: &str) -> String {
    format!("w2c_{}_{}", mangle(module, true), mangle(name, false))
}

pub fn mangle(name: &str, double_underscores: bool) -> String {
    #[derive(PartialEq)]
    enum State {
        Any,
        Zero,
        ZeroX,
        ZeroXHexDigit,
    }
    let bytes = name.as_bytes();
    let mut out = String::new();
    let mut state = State::Any;
    let mut last_was_underscore = false;
    for (i, &ch) in bytes.iter().enumerate() {
        state = match state {
            State::Any | State::ZeroXHexDigit if ch == b'0' => State::Zero,
            State::Zero if ch == b'x' => State::ZeroX,
            // wabt's hex digits are capitals only: "0xff" stays as it is.
            State::ZeroX if ch.is_ascii_digit() || (b'A'..=b'F').contains(&ch) => State::ZeroXHexDigit,
            _ => State::Any,
        };
        let escape = state == State::ZeroXHexDigit
            || (ch == b'_' && (i == 0 || i + 1 == bytes.len() || last_was_underscore))
            || !(ch.is_ascii_alphanumeric() || ch == b'_');
        if escape {
            out += &format!("0x{ch:02X}");
            last_was_underscore = false;
            state = State::Any;
        } else if double_underscores && ch == b'_' {
            out += "__";
            last_was_underscore = true;
        } else {
            out.push(ch as char);
            last_was_underscore = ch == b'_';
        }
    }
    out
}

#[test]
fn matches_wasm2c() {
    assert_eq!(game_symbol("__wasm_call_ctors"), "w2c_game_0x5F_wasm_call_ctors");
    assert_eq!(game_symbol("_emscripten_tls_init"), "w2c_game_0x5Femscripten_tls_init");
    assert_eq!(game_symbol("rage::ioPad::Update(bool)"), "w2c_game_rage0x3A0x3AioPad0x3A0x3AUpdate0x28bool0x29");
    assert_eq!(import_symbol("wasi_snapshot_preview1", "fd_write"), "w2c_wasi__snapshot__preview1_fd_write");
    assert_eq!(import_symbol("env", "__syscall_openat"), "w2c_env_0x5F_syscall_openat");
    assert_eq!(game_symbol("f0xab"), "w2c_game_f0xab");
    assert_eq!(game_symbol("f0xAB"), "w2c_game_f0x0x41B");
}
