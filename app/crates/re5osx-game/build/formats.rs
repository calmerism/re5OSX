//! The engine's Windows-only printf sizes, made standard.
//!
//! About 450 of the engine's messages print a size with MSVC's `I` modifier
//! (`%Iu`, `%Id`, `%Ix`...). The game's libc (musl) stops at a conversion it
//! does not know, so those messages came out cut there ("... -- 0 + ").
//! In the module's read-only data, each such `I` becomes `z`, the standard
//! modifier for `size_t`: the same length, and the same 64-bit size in this
//! wasm64 build as on Windows x64.

use std::fs;
use std::path::PathBuf;

use anyhow::{Result, ensure};

/// How a read-only data segment's array starts in the wasm2c output.
const SEGMENT: &str = "\nconst u8 data_segment_data_w2c_game_0x2Erodata";

/// Rewrites the conversions in `texts` (each source's text, possibly already
/// patched), keyed like `sources`.
pub fn apply(sources: &[PathBuf], texts: &mut [Option<String>]) -> Result<()> {
    let mut fixed = 0;
    for (source, text) in sources.iter().zip(texts.iter_mut()) {
        let current = match text {
            Some(t) => std::mem::take(t),
            None => fs::read_to_string(source)?,
        };
        if !current.contains(SEGMENT) {
            if text.is_some() {
                *text = Some(current);
            }
            continue;
        }
        let mut bytes = current.into_bytes();
        let mut at = 0;
        while let Some(found) = find(&bytes[at..], SEGMENT.as_bytes()) {
            let start = at + found;
            let end = start + find(&bytes[start..], b"\n};").unwrap_or(bytes.len() - start);
            fixed += fix_segment(&mut bytes[start..end]);
            at = end;
        }
        *text = Some(String::from_utf8(bytes)?);
        // wasm2c writes all the data segments into one file.
        break;
    }
    ensure!(fixed > 0, "no printf conversion with MSVC's I size found: the data changed");
    Ok(())
}

/// Fixes one segment's array text in place; returns how many conversions.
fn fix_segment(text: &mut [u8]) -> usize {
    // Where each byte's `0x..` token is in the text, and the bytes.
    let body = text.iter().position(|&c| c == b'{').map_or(0, |i| i + 1);
    let tokens: Vec<usize> =
        (body..text.len().saturating_sub(3)).filter(|&i| text[i] == b'0' && text[i + 1] == b'x').collect();
    let data: Vec<u8> = tokens.iter().map(|&i| hex(text[i + 2]) << 4 | hex(text[i + 3])).collect();
    let mut fixed = 0;
    let mut i = 0;
    while i < data.len() {
        if data[i] != b'%' {
            i += 1;
            continue;
        }
        if data.get(i + 1) == Some(&b'%') {
            i += 2;
            continue;
        }
        // Flags, width, precision, then the size and the conversion.
        let mut j = i + 1;
        while j < data.len() && b"-+ #0".contains(&data[j]) {
            j += 1;
        }
        while j < data.len() && data[j].is_ascii_digit() {
            j += 1;
        }
        if data.get(j) == Some(&b'.') {
            j += 1;
            while j < data.len() && data[j].is_ascii_digit() {
                j += 1;
            }
        }
        if data.get(j) == Some(&b'I') && data.get(j + 1).is_some_and(|c| b"udixX".contains(c)) {
            let t = tokens[j];
            text[t + 2..t + 4].copy_from_slice(b"7a");
            fixed += 1;
        }
        i = j.max(i + 1);
    }
    fixed
}

fn hex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        _ => (c | 0x20) - b'a' + 10,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
