//! The Studio's WebSocket: requests `{id, method, params}` answered with
//! `{id, result}` or `{id, error}`; the server sends `{method: "mods"}`
//! when a mod's state changes, `{method: "menus"}` when the mods' menus do,
//! `{method: "log"}` with new log lines, and `{method: "hello"}` first.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Socket, api, receive, send};
use crate::files;

/// Serves one page until it goes or `stop` (the server stopped: the code
/// changed, the Studio was turned off).
pub fn run(mut ws: Socket, stop: &std::sync::atomic::AtomicBool) -> std::io::Result<()> {
    let hello = json!({
        "method": "hello",
        "params": {
            "build": re5osx_runtime::GAME_BUILD,
            "natives": crate::natives::NATIVES.len(),
            "namespaces": crate::natives::NAMESPACES.len(),
            "device": device(),
            "tainted": crate::tainted(),
        }
    });
    if send(&mut ws, &hello).is_err() {
        return Ok(());
    }
    let mut generation = u64::MAX;
    let (_, mut cursor) = crate::logs(u64::MAX);
    // The log kept so far, then what comes.
    let (lines, _) = api::logs(0, None);
    if !lines.is_empty() && send(&mut ws, &json!({ "method": "log", "params": lines })).is_err() {
        return Ok(());
    }
    let (_, mut changes) = crate::changes(u64::MAX);
    let mut figures = Instant::now();
    let mut menus = Value::Null;
    // Evaluations run on their own threads; their answers come back here.
    let (done_tx, done_rx) = mpsc::channel::<Value>();
    // Evaluations running for this page (a few at a time).
    let mut evaluating = 0;
    loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ok(());
        }
        match receive(&mut ws) {
            Ok(Some(text)) => {
                let Ok(msg) = serde_json::from_str::<Value>(&text) else { continue };
                let id = msg["id"].clone();
                let method = msg["method"].as_str().unwrap_or("").to_string();
                if method == "eval" {
                    if evaluating >= 4 {
                        let _ = send(&mut ws, &json!({ "id": id, "error": "the game is still busy with the last ones" }));
                        continue;
                    }
                    evaluating += 1;
                    let code = msg["params"]["code"].as_str().unwrap_or("").to_string();
                    let tx = done_tx.clone();
                    std::thread::spawn(move || {
                        re5osx_runtime::exit::panic_recoverable();
                        let reply = match api::eval(&code, Duration::from_secs(10)) {
                            Ok(v) => json!({ "id": id, "result": v }),
                            Err(e) => json!({ "id": id, "error": e }),
                        };
                        let _ = tx.send(reply);
                    });
                    continue;
                }
                let reply = match handle(&method, &msg["params"]) {
                    Ok(result) => json!({ "id": id, "result": result }),
                    Err(e) => json!({ "id": id, "error": e }),
                };
                if send(&mut ws, &reply).is_err() {
                    return Ok(());
                }
                // A change shows at once rather than at the next poll.
                generation = generation.wrapping_sub(1);
            }
            Ok(None) => {}
            Err(()) => return Ok(()),
        }
        for reply in done_rx.try_iter() {
            evaluating -= 1;
            if send(&mut ws, &reply).is_err() {
                return Ok(());
            }
        }
        // The mods: when their state changes, and every second for the
        // time and memory they take.
        let now = crate::generation();
        if now != generation || figures.elapsed() > Duration::from_secs(1) {
            generation = now;
            figures = Instant::now();
            if send(&mut ws, &json!({ "method": "mods", "params": api::mods() })).is_err() {
                return Ok(());
            }
            let now = api::menus();
            if now != menus {
                if send(&mut ws, &json!({ "method": "menus", "params": now })).is_err() {
                    return Ok(());
                }
                menus = now;
            }
        }
        let (paths, next) = crate::changes(changes);
        changes = next;
        if !paths.is_empty() && send(&mut ws, &json!({ "method": "files", "params": paths })).is_err() {
            return Ok(());
        }
        let (lines, next) = api::logs(cursor, None);
        cursor = next;
        if !lines.is_empty() && send(&mut ws, &json!({ "method": "log", "params": lines })).is_err() {
            return Ok(());
        }
    }
}

fn handle(method: &str, p: &Value) -> Result<Value, String> {
    let s = |k: &str| p[k].as_str().ok_or_else(|| format!("{method}: missing {k}"));
    Ok(match method {
        "mods.list" => api::mods(),
        "mods.create" => {
            files::create_mod(s("name")?)?;
            Value::Null
        }
        "mods.start" => {
            if let Some(why) = crate::cannot_start() {
                return Err(why.into());
            }
            crate::start(s("name")?);
            Value::Null
        }
        "mods.stop" => {
            crate::stop(s("name")?);
            Value::Null
        }
        "mods.restart" => {
            if let Some(why) = crate::cannot_start() {
                return Err(why.into());
            }
            crate::restart(s("name")?);
            Value::Null
        }
        "mods.autostart" => {
            crate::set_autostart(s("name")?, p["on"].as_bool().unwrap_or(false))?;
            Value::Null
        }
        // The mod's page: its info, what it stores.
        "mods.info" => {
            let name = s("name")?;
            let info = files::info(name);
            let stored = crate::stored(name);
            let keys = serde_json::from_str::<serde_json::Map<String, Value>>(&stored).map_or(0, |m| m.len());
            json!({
                "title": info.title, "description": info.description, "author": info.author, "version": info.version,
                "autostart": info.autostart, "restartOnSave": info.restart_on_save,
                "stored": { "keys": keys, "bytes": if keys == 0 { 0 } else { stored.len() } },
            })
        }
        "mods.setInfo" => {
            let name = s("name")?;
            let text = |k: &str| p["info"][k].as_str().unwrap_or("").to_string();
            let info = crate::ModInfo {
                title: text("title"),
                description: text("description"),
                author: text("author"),
                version: text("version"),
                autostart: p["info"]["autostart"].as_bool().unwrap_or(false),
                restart_on_save: p["info"]["restartOnSave"].as_bool().unwrap_or(true),
            };
            files::set_info(name, &info)?;
            Value::Null
        }
        "mods.delete" => {
            let name = s("name")?;
            // Stopped first (it would write its storage again as it ends).
            if crate::mods().iter().any(|m| m.name == name && m.state == crate::ModState::Running) {
                crate::stop(name);
                let until = Instant::now() + Duration::from_secs(3);
                while Instant::now() < until && crate::mods().iter().any(|m| m.name == name && m.state == crate::ModState::Running) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            files::delete_mod(name)?;
            Value::Null
        }
        "files.create" => {
            files::create(s("path")?)?;
            Value::Null
        }
        "storage.view" => Value::String(crate::stored(s("mod")?)),
        "storage.clear" => {
            crate::clear_stored(s("mod")?);
            Value::Null
        }
        "files.read" => json!({ "text": files::read(s("path")?)? }),
        "files.write" => {
            let path = s("path")?;
            files::write(path, s("text")?)?;
            json!({ "restarted": crate::saved(path) })
        }
        "files.delete" => {
            files::delete(s("path")?)?;
            Value::Null
        }
        "files.rename" => {
            files::rename(s("from")?, s("to")?)?;
            Value::Null
        }
        "menus.input" => {
            let item = p["item"].as_u64().ok_or("menus.input: missing item")? as u32;
            api::menu_input(s("mod")?, item, &p["value"])?;
            Value::Null
        }
        "natives.search" => Value::Array(api::search(s("query")?, p["limit"].as_u64().unwrap_or(30) as usize)),
        "natives.doc" => api::find(s("name")?).map(|n| api::describe(n, true)).ok_or("no such native")?,
        _ => return Err(format!("unknown method {method}")),
    })
}

/// What the device is, for the Studio's title bar.
fn device() -> String {
    let os = match std::env::consts::OS {
        "macos" => "Mac",
        "ios" => "iPhone or iPad",
        "tvos" => "Apple TV",
        "android" => "Android",
        "windows" => "Windows PC",
        "linux" => "Linux PC",
        other => other,
    };
    std::env::var("RE5OSX_DEVICE_NAME").unwrap_or_else(|_| os.to_string())
}
