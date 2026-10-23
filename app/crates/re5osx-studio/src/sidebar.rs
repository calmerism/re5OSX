//! The sidebar: the mods and their files, the natives' search.

use leptos::prelude::*;
use serde_json::{Value, json};

use crate::actions::{delete_file, import, insert_code, new_file, open_file, open_mod_page, rename_file};
use crate::icons;
use crate::net::request;
use crate::state::*;

pub(crate) fn state_class(m: &ModInfo) -> &'static str {
    match (m.state.as_str(), m.waiting) {
        ("running", false) => "state run",
        ("running", true) => "state wait",
        ("error", _) => "state err",
        _ => "state stop",
    }
}

pub(crate) fn meta(m: &ModInfo) -> String {
    match m.state.as_str() {
        "running" => format!("{:.1} ms", m.cpu_ms),
        "error" => "error".into(),
        _ if m.files.is_empty() => String::new(),
        _ => "stopped".into(),
    }
}

#[component]
pub(crate) fn ModsView(s: S) -> impl IntoView {
    let new_mod = move |_| {
        let Some(name) = web_sys::window().and_then(|w| w.prompt_with_message("New mod's name (lowercase, digits, - and _):").ok().flatten()) else { return };
        let name = name.trim().to_lowercase();
        let n = name.clone();
        request("mods.create", json!({ "name": name }), move |r| match r {
            Ok(_) => {
                s.expanded.update(|e| e.push(n.clone()));
                open_file(s, format!("{n}/main.luau"));
            }
            Err(e) => s.notice.set(Some(e)),
        });
    };
    let running = move || s.mods.get().into_iter().filter(|m| m.state == "running").collect::<Vec<_>>();
    view! {
        <div class="side-head">
            <h2>"MODS"</h2>
            <div class="tools">
                <button class="icon-btn" title="Import a mod (.zip)" on:click=move |_| import(s)><span class="ico" inner_html=icons::IMPORT></span></button>
                <button class="icon-btn" title="New mod" on:click=new_mod><span class="ico" inner_html=icons::PLUS></span></button>
                <button class="icon-btn" title="Stop all" on:click=move |_| {
                    for m in s.mods.get_untracked() { if m.state == "running" { request("mods.stop", json!({"name": m.name}), |_| {}); } }
                }><span class="ico" inner_html=icons::STOP></span></button>
            </div>
        </div>
        <div class="section">"In the game"<span class="count">{move || match s.mods.with(Vec::len) {
                _ if !s.listed.get() => String::new(),
                1 => "1 mod".to_string(),
                n => format!("{n} mods"),
            }}</span></div>
        <div class="tree">
            // Keyed by what a row shows but its time: rows stay (with their
            // hover and clicks) while the figures change.
            <For each=move || s.mods.get() key=|m| format!("{}|{}|{}|{}|{}|{}", m.name, m.state, m.waiting, m.files.join(","), m.title, m.description) let(m)>{{
                let name = m.name.clone();
                let figure = { let n = name.clone(); move || s.mods.with(|v| v.iter().find(|x| x.name == n).map(meta).unwrap_or_default()) };
                let open = { let n = name.clone(); move || s.expanded.with(|e| e.contains(&n)) };
                // The arrow opens or closes its files; the row opens its page (and its files).
                let toggle = { let n = name.clone(); move |ev: leptos::ev::MouseEvent| {
                    ev.stop_propagation();
                    s.expanded.update(|e| if let Some(i) = e.iter().position(|x| *x == n) { e.remove(i); } else { e.push(n.clone()); });
                }};
                let page = { let n = name.clone(); move |_| {
                    s.expanded.update(|e| if !e.contains(&n) { e.push(n.clone()); });
                    open_mod_page(s, &n);
                }};
                let selected = { let p = format!("{MOD_PAGE}{name}"); move || s.active.get().as_deref() == Some(p.as_str()) };
                let run = { let n = name.clone(); let running = m.state == "running"; move |ev: leptos::ev::MouseEvent| {
                    ev.stop_propagation();
                    request(if running { "mods.stop" } else { "mods.start" }, json!({ "name": n }), move |r| if let Err(e) = r { s.notice.set(Some(e)) });
                }};
                let files = m.files.clone();
                let open2 = open.clone();
                view! {
                    <div class=move || if selected() { "mod sel" } else { "mod" } role="button" tabindex="0" on:click=page
                        on:keydown={ let n = name.clone(); move |ev| if ev.key() == "Enter" { open_mod_page(s, &n) } }
                        title=m.error.clone().unwrap_or_else(|| m.description.clone())>
                        <span class="chev" on:click=toggle>{move || if open() { "▾" } else { "▸" }}</span>
                        <span class=state_class(&m)></span>
                        <span class="name">{m.title.clone()}{(m.title != m.name).then(|| view! { <small>{m.name.clone()}</small> })}</span>
                        <span class=if m.state == "error" { "meta err" } else { "meta" }>{figure}</span>
                        <button class="runbtn" title=if m.state == "running" { "Stop" } else { "Start" } on:click=run>
                            <span class="ico" inner_html=if m.state == "running" { icons::STOP } else { icons::PLAY }></span>
                        </button>
                    </div>
                    <Show when=open2>
                        {files.iter().filter(|f| shown_file(f)).map(|f| {
                            let path = f.clone();
                            let shown = f.split_once('/').map_or(f.as_str(), |(_, r)| r).to_string();
                            let p2 = path.clone();
                            let active = move || s.active.get().as_deref() == Some(p2.as_str());
                            let p3 = path.clone();
                            let errs = move || s.diags.with(|d| d.get(&p3).map_or(0, |v| v.iter().filter(|d| d.severity == "error").count()));
                            view! {
                                <div class=move || if active() { "file on" } else { "file" } role="button" tabindex="0"
                                    on:click={ let p = path.clone(); move |_| open_file(s, p.clone()) }
                                    on:keydown={ let p = path.clone(); move |ev| if ev.key() == "Enter" { open_file(s, p.clone()) } }>
                                    <span class="ico small" inner_html=icons::FILE></span>
                                    <span class="fname">{shown}</span>
                                    <span class="err-count">{move || match errs() { 0 => String::new(), n => n.to_string() }}</span>
                                    <span class="file-acts">
                                        <button class="icon-btn" title="Rename" on:click={ let p = path.clone(); move |ev: leptos::ev::MouseEvent| { ev.stop_propagation(); rename_file(s, &p) } }>
                                            <span class="ico" inner_html=icons::PENCIL></span>
                                        </button>
                                        <button class="icon-btn" title="Delete" on:click={ let p = path.clone(); move |ev: leptos::ev::MouseEvent| { ev.stop_propagation(); delete_file(s, &p) } }>
                                            <span class="ico" inner_html=icons::TRASH></span>
                                        </button>
                                    </span>
                                </div>
                            }
                        }).collect_view()}
                        <div class="file new" on:click={ let n = name.clone(); move |_| new_file(s, &n) }>
                            <span class="ico small" inner_html=icons::PLUS></span><span>"New file"</span>
                        </div>
                    </Show>
                }
            }}</For>
            <Show when=move || s.listed.get() && s.mods.with(Vec::is_empty)>
                <p class="empty">"No mods yet. Make one with +."</p>
            </Show>
        </div>
        <Show when=move || !running().is_empty()>
            <div class="section">"Running"<span class="count">{move || {
                let r = running();
                format!("{} · {:.1} ms/frame", r.len(), r.iter().map(|m| m.cpu_ms).fold(0.0, |a, b| a + b))
            }}</span></div>
            <div class="running">
                <For each=running key=|m| format!("{}|{}", m.name, m.waiting) let(m)>{{
                    let n1 = m.name.clone();
                    let n2 = m.name.clone();
                    let n3 = m.name.clone();
                    let cpu = Memo::new(move |_| s.mods.with(|v| v.iter().find(|x| x.name == n3).map_or(0.0, |x| x.cpu_ms)));
                    view! {
                        <div class="job">
                            <div class="job-top">
                                <span class=state_class(&m)></span>
                                <span class="name">{m.name.clone()}</span>
                                <span class="pill ok">{if m.waiting { "Waiting" } else { "Running" }}</span>
                            </div>
                            <div class="job-sub">
                                <span>{move || format!("{:.1} ms", cpu.get())}</span>
                                <div class="meter"><i style=move || format!("width:{}%", (cpu.get() / 2.0 * 100.0).min(100.0))></i></div>
                                <button class="icon-btn" title="Restart" on:click=move |_| request("mods.restart", json!({"name": n1}), |_| {})><span class="ico" inner_html=icons::RESTART></span></button>
                                <button class="icon-btn" title="Stop" on:click=move |_| request("mods.stop", json!({"name": n2}), |_| {})><span class="ico" inner_html=icons::STOP></span></button>
                            </div>
                        </div>
                    }
                }}</For>
            </div>
        </Show>
        <div class="side-foot" on:click=move |_| open_file(s, "natives.d.luau".into())>
            <span class="ico small" inner_html=icons::BOOK></span>
            <span>{move || s.hello.with(|h| format!("{} natives · {} namespaces", h["natives"], h["namespaces"]))}</span>
        </div>
    }
}

#[component]
pub(crate) fn NativesView(s: S) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<Value>::new());
    let run = move |q: String| {
        query.set(q.clone());
        if q.trim().len() < 2 {
            results.set(Vec::new());
            return;
        }
        let asked = q.clone();
        request("natives.search", json!({ "query": q, "limit": 60 }), move |r| {
            if let Ok(Value::Array(list)) = r
                && query.get_untracked() == asked
            {
                results.set(list);
            }
        });
    };
    view! {
        <div class="side-head"><h2>"NATIVES"</h2></div>
        <label class="side-search">
            <span class="ico" inner_html=icons::SEARCH></span>
            <input placeholder="vehicle colour, health, teleport…" spellcheck="false"
                prop:value=move || query.get() on:input=move |ev| run(event_target_value(&ev))/>
        </label>
        <div class="natives">
            {move || results.get().into_iter().map(|n| {
                let name = n["name"].as_str().unwrap_or("").to_string();
                let insert = format!("{name}(");
                let detail = RwSignal::new(None::<String>);
                let n2 = name.clone();
                view! {
                    <div class="native" on:click=move |_| {
                        if detail.get_untracked().is_some() { detail.set(None); return; }
                        request("natives.doc", json!({"name": n2}), move |r| if let Ok(v) = r {
                            detail.set(Some(v["notes"].as_str().unwrap_or("No notes.").to_string()));
                        });
                    }>
                        <div class="sig">{n["signature"].as_str().unwrap_or("").to_string()}</div>
                        <div class="native-foot">
                            <small>{n["native"].as_str().unwrap_or("").to_string()}</small>
                            <button class="chip" on:click=move |ev| { ev.stop_propagation(); insert_code(s, &insert); }>"Insert"</button>
                        </div>
                        {move || detail.get().map(|d| view! { <p class="notes">{d}</p> })}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}
