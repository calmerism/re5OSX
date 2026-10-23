//! The editor's column: tabs, toolbar, the welcome page.

use leptos::prelude::*;
use serde_json::json;

use crate::actions::{close_tab, open_file, save};
use crate::sidebar::state_class;
use crate::icons;
use crate::net::request;
use crate::state::*;

#[component]
pub(crate) fn Tabs(s: S) -> impl IntoView {
    view! {
        <div class="tabs">
            {move || s.tabs.get().into_iter().map(|t| {
                let p = t.path.clone();
                let p2 = t.path.clone();
                let p3 = t.path.clone();
                let active = move || s.active.get().as_deref() == Some(p.as_str());
                let page = mod_page(&t.path).map(str::to_string);
                let (file, module) = match (&page, t.path.split_once('/')) {
                    // Its name as it changes (on its page, elsewhere).
                    (Some(m), _) => {
                        let m = m.clone();
                        let title = move || s.mods.with(|ms| ms.iter().find(|x| x.name == m).map(|x| x.title.clone())).unwrap_or_else(|| m.clone());
                        ((move || format!("⚙ {}", title())).into_any(), "mod".to_string())
                    }
                    (None, Some((m, f))) => (f.to_string().into_any(), m.to_string()),
                    (None, None) => (t.path.clone().into_any(), String::new()),
                };
                let read_only = t.read_only && page.is_none();
                view! {
                    <div class=move || if active() { "tab on" } else { "tab" }
                        on:click=move |_| {
                            s.active.set(Some(p2.clone()));
                            if mod_page(&p2).is_none() { with_editor(|e| e.open(&p2, "", false)); }
                        }>
                        <span>{file}</span>
                        <em>{module}</em>
                        {if read_only { view! { <span class="ico lock" inner_html=icons::LOCK></span> }.into_any() }
                         else if t.dirty { view! { <span class="unsaved"></span> }.into_any() }
                         else { ().into_any() }}
                        <button class="x" aria-label="Close" title="Close" on:click=move |ev| { ev.stop_propagation(); close_tab(s, &p3); }>"×"</button>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

#[component]
pub(crate) fn Toolbar(s: S) -> impl IntoView {
    let current = move || s.active.get().filter(|p| p.contains('/') && mod_page(p).is_none());
    let page = move || s.active.get().and_then(|p| mod_page(&p).map(str::to_string));
    let module = move || current().map(|p| mod_of(&p).to_string()).or_else(page);
    let info = move || module().and_then(|m| s.mods.with(|mods| mods.iter().find(|x| x.name == m).cloned()));
    let toggle_run = move |_| {
        let Some(m) = info() else { return };
        // Unsaved changes go first.
        if let Some(p) = current() && s.tabs.with_untracked(|t| t.iter().any(|t| t.path == p && t.dirty)) {
            save(s, p);
        }
        let method = if m.state == "running" { "mods.stop" } else { "mods.start" };
        request(method, json!({ "name": m.name }), move |r| if let Err(e) = r { s.notice.set(Some(e)) });
    };
    view! {
        <div class="toolbar">
            <div class="crumbs">
                {move || current().map(|p| {
                    let (m, f) = p.split_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
                    view! { <span>{m}</span><span class="sep">"›"</span><span class="here">{f}</span> }.into_any()
                }).or_else(|| page().map(|m| view! { <span>{m}</span><span class="sep">"›"</span><span class="here">"Mod"</span> }.into_any()))}
            </div>
            <div class="grow"></div>
            {move || info().map(|m| {
                let running = m.state == "running";
                let name = m.name.clone();
                view! {
                    <div class="runstate">
                        <span class=state_class(&m)></span>
                        <b>{match m.state.as_str() { "running" if m.waiting => "Waiting", "running" => "Running", "error" => "Error", _ => "Stopped" }}</b>
                    </div>
                    {running.then(|| view! {
                        <button class="btn" on:click=move |_| request("mods.restart", json!({"name": name}), |_| {})>
                            <span class="ico small" inner_html=icons::RESTART></span>"Restart"
                        </button>
                    })}
                    <button class="btn primary" on:click=toggle_run>
                        <span class="ico small" inner_html=if running { icons::STOP } else { icons::PLAY }></span>
                        {if running { "Stop" } else { "Start" }}
                    </button>
                }
            })}
            <Show when=move || current().is_some()>
                <button class="btn ghost" title="Save (Ctrl/Cmd+S)" on:click=move |_| if let Some(p) = current() { save(s, p) }>"Save"</button>
            </Show>
        </div>
    }
}

#[component]
pub(crate) fn Welcome(s: S) -> impl IntoView {
    view! {
        <div class="welcome">
            <h1>"RE5OSX STUDIO"</h1>
            <p>"Mods are Luau scripts that run in the game. Pick a file in Mods, or make a mod with +."</p>
            <div class="welcome-actions">
                <button class="btn" on:click=move |_| { s.side.set(Side::Mods); s.screen.set(Screen::Mods); }>"Mods"</button>
                <button class="btn" on:click=move |_| open_file(s, "natives.d.luau".into())>"Every native's type"</button>
            </div>
        </div>
    }
}
