//! What the buttons and keys do: open, close, save, evaluate.

use leptos::prelude::*;
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::net::{fetch_text, request};
use crate::state::*;

/// A mod's page, in a tab of its own.
pub(crate) fn open_mod_page(s: S, name: &str) {
    s.screen.set(Screen::Editor);
    s.drawer.set(false);
    let path = format!("{MOD_PAGE}{name}");
    if !s.tabs.with_untracked(|t| t.iter().any(|t| t.path == path)) {
        s.tabs.update(|t| t.push(Tab { path: path.clone(), dirty: false, read_only: true }));
    }
    s.active.set(Some(path));
}

/// Closes the tabs of mod `name` (its page, its files): it went away.
pub(crate) fn close_mod(s: S, name: &str) {
    let page = format!("{MOD_PAGE}{name}");
    let prefix = format!("{name}/");
    let gone: Vec<String> = s.tabs.with_untracked(|t| {
        t.iter().filter(|t| t.path == page || t.path.starts_with(&prefix)).map(|t| t.path.clone()).collect()
    });
    for path in &gone {
        forget_tab(s, path);
    }
}

/// A new file in mod `name`, asked for by name, then opened.
pub(crate) fn new_file(s: S, name: &str) {
    let Some(file) = web_sys::window()
        .and_then(|w| w.prompt_with_message_and_default("New file's name:", "module.luau").ok().flatten())
    else {
        return;
    };
    let file = file.trim().to_string();
    if file.is_empty() {
        return;
    }
    let path = format!("{name}/{file}");
    request("files.create", json!({ "path": path }), move |r| match r {
        Ok(_) => open_file(s, path),
        Err(e) => s.notice.set(Some(e)),
    });
}

/// Renames a file, asked for the new name; its tab follows.
pub(crate) fn rename_file(s: S, path: &str) {
    let Some((name, file)) = path.split_once('/') else { return };
    let Some(new) = web_sys::window().and_then(|w| w.prompt_with_message_and_default("Rename to:", file).ok().flatten()) else {
        return;
    };
    let new = new.trim().to_string();
    if new.is_empty() || new == file {
        return;
    }
    let (from, to) = (path.to_string(), format!("{name}/{new}"));
    // Unsaved changes go first (the renamed file opens from disk).
    if s.tabs.with_untracked(|t| t.iter().any(|t| t.path == from && t.dirty)) {
        let mut text = String::new();
        with_editor(|e| text = e.text(&from));
        let (f, t) = (from.clone(), to.clone());
        request("files.write", json!({ "path": from, "text": text }), move |r| match r {
            Ok(_) => rename_now(s, f, t),
            Err(e) => s.notice.set(Some(e)),
        });
        return;
    }
    rename_now(s, from, to);
}

fn rename_now(s: S, from: String, to: String) {
    request("files.rename", json!({ "from": from, "to": to }), move |r| match r {
        Ok(_) => {
            if s.tabs.with_untracked(|t| t.iter().any(|t| t.path == from)) {
                forget_tab(s, &from);
                open_file(s, to.clone());
            }
        }
        Err(e) => s.notice.set(Some(e)),
    });
}

/// Deletes a file, once confirmed.
pub(crate) fn delete_file(s: S, path: &str) {
    let ok = web_sys::window()
        .and_then(|w| w.confirm_with_message(&format!("Delete {path}? It cannot be undone.")).ok())
        .unwrap_or(false);
    if !ok {
        return;
    }
    let p = path.to_string();
    request("files.delete", json!({ "path": p }), move |r| match r {
        Ok(_) => forget_tab(s, &p),
        Err(e) => s.notice.set(Some(e)),
    });
}

/// A mod from a .zip the person picks; its page opens.
pub(crate) fn import(s: S) {
    let token = TOKEN.with(|t| t.borrow().clone());
    let done = Closure::<dyn FnMut(JsValue)>::new(move |name: JsValue| {
        if let Some(name) = name.as_string() {
            s.expanded.update(|e| e.push(name.clone()));
            open_mod_page(s, &name);
        }
    });
    let failed = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
        s.notice.set(Some(format!("Import: {}", e.as_string().unwrap_or_else(|| "it failed".into()))));
    });
    let _ = crate::editor::import_mod(&format!("/import?token={token}")).then2(&done, &failed);
    done.forget();
    failed.forget();
}

pub(crate) fn open_file(s: S, path: String) {
    open_file_at(s, path, None);
}

/// Opens a file, and puts the cursor at (line, column) once it shows (not
/// before: on a slow link the file is read later).
pub(crate) fn open_file_at(s: S, path: String, at: Option<(u32, u32)>) {
    s.screen.set(Screen::Editor);
    s.drawer.set(false);
    let reveal = move || {
        if let Some((line, column)) = at {
            with_editor(|e| e.reveal(line, column));
        }
    };
    if s.tabs.with_untracked(|t| t.iter().any(|t| t.path == path)) {
        s.active.set(Some(path.clone()));
        with_editor(|e| e.open(&path, "", false));
        reveal();
        return;
    }
    let read_only = path == "natives.d.luau";
    let p = path.clone();
    let shown = move |text: String| {
        // Asked twice (a double tap) before the first answer: one tab.
        if !s.tabs.with_untracked(|t| t.iter().any(|t| t.path == p)) {
            s.tabs.update(|t| t.push(Tab { path: p.clone(), dirty: false, read_only }));
        }
        with_editor(|e| e.open(&p, &text, read_only));
        s.active.set(Some(p.clone()));
        reveal();
    };
    if read_only {
        // The definitions, as served (not a mod's file).
        let token = TOKEN.with(|t| t.borrow().clone());
        let url = format!("/natives.d.luau?token={token}");
        fetch_text(url, shown);
    } else {
        request("files.read", json!({ "path": path }), move |r| match r {
            Ok(v) => shown(v["text"].as_str().unwrap_or("").to_string()),
            Err(e) => s.notice.set(Some(e)),
        });
    }
}

/// A tab goes (closed, its file deleted or renamed, its mod gone): out of
/// the editor and the problems; the last tab left shows instead.
pub(crate) fn forget_tab(s: S, path: &str) {
    if mod_page(path).is_none() {
        with_editor(|e| e.close(path));
    }
    s.tabs.update(|t| t.retain(|t| t.path != path));
    s.diags.update(|d| {
        d.remove(path);
    });
    if s.active.get_untracked().as_deref() == Some(path) {
        let next = s.tabs.with_untracked(|t| t.last().map(|t| t.path.clone()));
        s.active.set(next.clone());
        if let Some(n) = next.filter(|n| mod_page(n).is_none()) {
            with_editor(|e| e.open(&n, "", false));
        }
    }
}

pub(crate) fn close_tab(s: S, path: &str) {
    let dirty = s.tabs.with_untracked(|t| t.iter().any(|t| t.path == path && t.dirty));
    if dirty
        && !web_sys::window()
            .and_then(|w| w.confirm_with_message(&format!("{path} has unsaved changes. Close it?")).ok())
            .unwrap_or(false)
    {
        return;
    }
    forget_tab(s, path);
}

/// Code into the open file at the cursor; with no file open (or a mod's
/// page shown), it says so rather than typing into nothing.
pub(crate) fn insert_code(s: S, code: &str) {
    let writable = active_file(s).filter(|p| s.tabs.with_untracked(|t| t.iter().any(|t| &t.path == p && !t.read_only)));
    if writable.is_none() {
        s.notice.set(Some("Open one of the mod's files first: it goes in at the cursor".into()));
        return;
    }
    s.screen.set(Screen::Editor);
    with_editor(|e| e.insert(code));
}

/// The open file, when the active tab is one (not a mod's page).
pub(crate) fn active_file(s: S) -> Option<String> {
    s.active.get_untracked().filter(|p| mod_page(p).is_none())
}

pub(crate) fn save(s: S, path: String) {
    // The definitions and mods' pages are not files to write.
    if mod_page(&path).is_some() || s.tabs.with_untracked(|t| t.iter().any(|t| t.path == path && t.read_only)) {
        return;
    }
    let mut text = String::new();
    with_editor(|e| text = e.text(&path));
    let t = text.clone();
    request("files.write", json!({ "path": path, "text": text }), move |r| match r {
        Ok(_) => with_editor(|e| e.saved(&path, &t)),
        Err(e) => s.notice.set(Some(e)),
    });
}

pub(crate) fn eval(s: S, code: String) {
    if code.trim().is_empty() {
        return;
    }
    push_row(s, Row::Input(code.clone()));
    request("eval", json!({ "code": code }), move |r| match r {
        Ok(v) => push_row(s, Row::Value(v.as_str().unwrap_or("").to_string())),
        Err(e) => push_row(s, Row::Failure(e)),
    });
}
