//! The type checker as a language server over a WebSocket (LSP's JSON-RPC,
//! one message per WebSocket message): diagnostics as documents change,
//! completions, hovers. Documents are `<anything>/<mod>/<file>.luau` URIs
//! (`file:///mods/trainer/main.luau`); their text replaces the file on disk
//! for the checker while they are open.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Socket, api, receive, send};
use crate::checker::Checker;

/// Completions sent at most: the client filters as one types, and asks
/// again (`isIncomplete`), so a short list keeps phones quick.
const MAX_ITEMS: usize = 150;

/// An editor that only listens is pinged this often, and taken for gone
/// (a phone out of range, a laptop shut: no close comes) after hearing
/// nothing for `SILENT`; else its thread and the checker would stay.
const PING: Duration = Duration::from_secs(20);
const SILENT: Duration = Duration::from_secs(70);

/// Serves one editor until it goes or `stop` (see session::run).
pub fn run(mut ws: Socket, stop: &std::sync::atomic::AtomicBool) -> std::io::Result<()> {
    let mut docs: HashMap<String, String> = HashMap::new();
    let (mut heard, mut pinged) = (Instant::now(), Instant::now());
    // Only the stop and the ping to look after between messages: no need
    // to wake 20 times a second as a session does.
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(1)));
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            let _ = ws.close(None);
            let _ = ws.flush();
            break;
        }
        let text = match receive(&mut ws) {
            Ok(Some(t)) => {
                heard = Instant::now();
                t
            }
            Ok(None) => {
                if heard.elapsed() > SILENT {
                    break;
                }
                if pinged.elapsed() >= PING {
                    pinged = Instant::now();
                    if ws.send(tungstenite::Message::Ping(Default::default())).is_err() {
                        break;
                    }
                }
                continue;
            }
            Err(()) => break,
        };
        let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
        let method = msg["method"].as_str().unwrap_or("");
        let p = &msg["params"];
        let uri = p["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        let result = match method {
            "initialize" => Some(json!({
                "capabilities": {
                    "textDocumentSync": { "openClose": true, "change": 1 },
                    "completionProvider": { "triggerCharacters": [".", ":"] },
                    "hoverProvider": true,
                    "signatureHelpProvider": { "triggerCharacters": ["(", ","], "retriggerCharacters": [","] },
                },
                "serverInfo": { "name": "re5osx-luau", "version": env!("CARGO_PKG_VERSION") },
            })),
            "textDocument/didOpen" => {
                // An editor has a handful open; this one is not an editor.
                if docs.len() >= 64 && !docs.contains_key(&uri) {
                    break;
                }
                docs.insert(uri.clone(), p["textDocument"]["text"].as_str().unwrap_or("").to_string());
                publish(&mut ws, &uri, &docs)?;
                None
            }
            "textDocument/didChange" => {
                if let Some(text) = p["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                    docs.insert(uri.clone(), text.to_string());
                }
                publish(&mut ws, &uri, &docs)?;
                None
            }
            "textDocument/didClose" => {
                docs.remove(&uri);
                let module = module(&uri);
                if module.ends_with(".luau") {
                    let _ = Checker::with(|c| c.set(&module, crate::files::read(&module).ok().as_deref()));
                }
                None
            }
            "textDocument/completion" => Some(completion(&uri, p, &docs)),
            "textDocument/hover" => Some(hover(&uri, p, &docs)),
            "textDocument/signatureHelp" => Some(signature(&uri, p, &docs)),
            "shutdown" => Some(Value::Null),
            "exit" => break,
            _ => {
                if !msg["id"].is_null() {
                    let reply = json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": -32601, "message": "not supported" } });
                    if send(&mut ws, &reply).is_err() {
                        break;
                    }
                }
                None
            }
        };
        if let Some(result) = result
            && !msg["id"].is_null()
            && send(&mut ws, &json!({ "jsonrpc": "2.0", "id": msg["id"], "result": result })).is_err()
        {
            break;
        }
    }
    Ok(())
}

/// `file:///mods/trainer/main.luau` -> `trainer/main.luau`.
fn module(uri: &str) -> String {
    let path = uri.split_once("://").map_or(uri, |(_, p)| p).trim_start_matches('/');
    let path = path.strip_prefix("mods/").unwrap_or(path);
    // The last two components: `<mod>/<file>` (files in folders keep them).
    path.to_string()
}

/// Line `line` of `text`, as Luau counts them.
fn line_of(text: &str, line: u64) -> &str {
    text.split('\n').nth(line as usize).unwrap_or("")
}

/// LSP counts a line's columns in UTF-16 units, Luau in bytes: the same
/// only up to the first character past ASCII.
fn to_bytes(line: &str, utf16: u64) -> u64 {
    let mut units = 0;
    for (at, c) in line.char_indices() {
        if units >= utf16 {
            return at as u64;
        }
        units += c.len_utf16() as u64;
    }
    line.len() as u64 + utf16.saturating_sub(units)
}

fn to_utf16(line: &str, bytes: u64) -> u64 {
    let mut end = (bytes as usize).min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].encode_utf16().count() as u64 + bytes.saturating_sub(line.len() as u64)
}

/// The request's position, as the checker counts (`text`: the document).
fn position(p: &Value, text: Option<&str>) -> (u32, u32) {
    let line = p["position"]["line"].as_u64().unwrap_or(0);
    let character = p["position"]["character"].as_u64().unwrap_or(0);
    let character = text.map_or(character, |t| to_bytes(line_of(t, line), character));
    (line as u32, character as u32)
}

/// A checker's range (`[line, column, line, column]`) as LSP's.
fn lsp_range(r: &Value, text: Option<&str>) -> Value {
    let n = |i: usize| r.get(i).and_then(Value::as_u64).unwrap_or(0);
    let column = |l: u64, c: u64| text.map_or(c, |t| to_utf16(line_of(t, l), c));
    let (l0, l1) = (n(0), n(2));
    json!({ "start": { "line": l0, "character": column(l0, n(1)) }, "end": { "line": l1, "character": column(l1, n(3)) } })
}

/// Checks the document and sends its diagnostics.
fn publish(ws: &mut Socket, uri: &str, docs: &HashMap<String, String>) -> std::io::Result<()> {
    let module = module(uri);
    let text = docs.get(uri).map(String::as_str);
    let found = api::diagnostics(&module, text);
    let diagnostics: Vec<Value> = found
        .as_array()
        .map(|a| {
            a.iter()
                .map(|d| {
                    json!({
                        "range": lsp_range(&d["range"], text),
                        "severity": if d["severity"] == "warning" { 2 } else { 1 },
                        "code": d["code"],
                        "source": "luau",
                        "message": d["message"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let note = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics },
    });
    send(ws, &note).map_err(|()| std::io::Error::other("closed"))
}

/// The identifier being typed before the cursor (`character` in bytes).
fn prefix(text: &str, line: u32, character: u32) -> String {
    let l = line_of(text, line.into());
    let before = l.get(..character as usize).unwrap_or(l);
    before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect()
}

fn completion(uri: &str, p: &Value, docs: &HashMap<String, String>) -> Value {
    let module = module(uri);
    if !module.ends_with(".luau") {
        return json!({ "isIncomplete": false, "items": [] });
    }
    let (line, character) = position(p, docs.get(uri).map(String::as_str));
    let typed = docs.get(uri).map(|t| prefix(t, line, character)).unwrap_or_default().to_ascii_lowercase();
    let items = Checker::with(|c| c.complete(&module, line, character)).unwrap_or(Value::Null);
    let mut items: Vec<&Value> = items
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| {
                    let label = i["label"].as_str().unwrap_or("").to_ascii_lowercase();
                    typed.is_empty() || label.starts_with(&typed) || (typed.len() >= 3 && label.contains(&typed))
                })
                .collect()
        })
        .unwrap_or_default();
    // Prefix matches, then what fits the expected type, then the shortest.
    items.sort_by_key(|i| {
        let label = i["label"].as_str().unwrap_or("");
        (!label.to_ascii_lowercase().starts_with(&typed), i["correct"] != true, label.len(), label.to_string())
    });
    let incomplete = items.len() > MAX_ITEMS;
    let items: Vec<Value> = items
        .into_iter()
        .take(MAX_ITEMS)
        .enumerate()
        .map(|(rank, i)| {
            // The signature without the name the label already shows.
            let label = i["label"].as_str().unwrap_or("");
            let detail = i["detail"].as_str().unwrap_or("");
            let detail = crate::world::shorten(detail.strip_prefix(label).unwrap_or(detail));
            let kind = match i["kind"].as_u64().unwrap_or(0) {
                0 if detail.contains('(') => 2, // Method
                0 => 10,                        // Property
                1 if detail.contains('(') => 3, // Function
                1 => 6,                         // Variable
                2 => 14,                        // Keyword
                3 => 12,                        // Value
                4 => 7,                         // Class
                5 | 7 => 9,                     // Module
                _ => 1,
            };
            let mut item = json!({
                "label": i["label"],
                "kind": kind,
                "detail": detail,
                "sortText": format!("{rank:04}"),
                "deprecated": i["deprecated"],
            });
            // The details beside the list: the whole signature (the list cuts
            // it), then the notes.
            let doc = i["doc"].as_str().filter(|d| !d.is_empty()).and_then(api::documentation);
            if !detail.is_empty() || doc.is_some() {
                let mut value = String::new();
                // (Not a namespace's whole table.)
                if !detail.is_empty() && detail.len() <= 600 {
                    let sep = if detail.starts_with('(') { "" } else { ": " };
                    value.push_str(&format!("```luau\n{label}{sep}{detail}\n```"));
                }
                if let Some(doc) = doc {
                    value.push_str("\n\n");
                    value.push_str(&doc);
                }
                item["documentation"] = json!({ "kind": "markdown", "value": value });
            }
            item
        })
        .collect();
    json!({ "isIncomplete": incomplete, "items": items })
}

fn hover(uri: &str, p: &Value, docs: &HashMap<String, String>) -> Value {
    let module = module(uri);
    if !module.ends_with(".luau") {
        return Value::Null;
    }
    let text = docs.get(uri).map(String::as_str);
    let (line, character) = position(p, text);
    let h = Checker::with(|c| c.hover(&module, line, character)).unwrap_or(Value::Null);
    let Some(ty) = h["type"].as_str() else { return Value::Null };
    let mut value = format!("```luau\n{}\n```", crate::world::shorten(ty));
    if let Some(doc) = h["doc"].as_str().filter(|d| !d.is_empty()).and_then(api::documentation) {
        value.push_str("\n\n");
        value.push_str(&doc);
    }
    json!({ "contents": { "kind": "markdown", "value": value }, "range": lsp_range(&h["range"], text) })
}

/// The parameters of the call being typed, the current one marked.
fn signature(uri: &str, p: &Value, docs: &HashMap<String, String>) -> Value {
    let module = module(uri);
    if !module.ends_with(".luau") {
        return Value::Null;
    }
    let (line, character) = position(p, docs.get(uri).map(String::as_str));
    let sig = Checker::with(|c| c.signature(&module, line, character)).unwrap_or(Value::Null);
    let Some(params) = sig["params"].as_array() else { return Value::Null };
    // The label with the catalog's names short; each parameter by its offsets
    // in it (UTF-16, as LSP counts).
    let mut label = format!("{}(", sig["name"].as_str().unwrap_or(""));
    let mut spans = Vec::new();
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let start = label.encode_utf16().count();
        let name = param["name"].as_str().unwrap_or("");
        let ty = crate::world::shorten(param["type"].as_str().unwrap_or("any"));
        label.push_str(&if name == "..." { format!("...{ty}") } else { format!("{name}: {ty}") });
        spans.push(json!({ "label": [start, label.encode_utf16().count()] }));
    }
    label.push_str(&format!("): {}", crate::world::shorten(sig["ret"].as_str().unwrap_or("()"))));
    let mut signature = json!({ "label": label, "parameters": spans });
    if let Some(doc) = sig["doc"].as_str().filter(|d| !d.is_empty()).and_then(api::documentation) {
        signature["documentation"] = json!({ "kind": "markdown", "value": doc });
    }
    let active = sig["active"].as_u64().unwrap_or(0).min(params.len().saturating_sub(1) as u64);
    json!({ "signatures": [signature], "activeSignature": 0, "activeParameter": active })
}

#[cfg(test)]
mod tests {
    #[test]
    fn columns() {
        // "à" is 2 bytes, 1 UTF-16 unit; "😀" 4 bytes, 2 units.
        let line = "x = \"à😀\" .. y";
        assert_eq!(super::to_bytes(line, 5), 5);
        assert_eq!(super::to_bytes(line, 6), 7);
        assert_eq!(super::to_bytes(line, 8), 11);
        assert_eq!(super::to_utf16(line, 11), 8);
        assert_eq!(super::to_utf16(line, 7), 6);
        // Past the end (a range ending after the last character).
        assert_eq!(super::to_utf16(line, line.len() as u64 + 1), line.encode_utf16().count() as u64 + 1);
        assert_eq!(super::to_bytes(line, line.encode_utf16().count() as u64 + 1), line.len() as u64 + 1);
    }
}
