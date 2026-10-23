//! The mods' menus (`ui.menu` in Luau), usable from the browser as in the
//! game: what is done here goes to the mod, which hears it at its next turn.

use leptos::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::net::request;
use crate::state::*;

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub(crate) struct MenuInfo {
    #[serde(rename = "mod")]
    pub(crate) module: String,
    pub(crate) id: u32,
    pub(crate) title: String,
    pub(crate) items: Vec<MenuItem>,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub(crate) struct MenuItem {
    id: u32,
    label: String,
    note: Option<String>,
    kind: String,
    #[serde(default)]
    value: Value,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    #[serde(default)]
    options: Vec<String>,
}

fn send(s: S, module: String, item: u32, value: Value) {
    request("menus.input", json!({ "mod": module, "item": item, "value": value }), move |r| {
        if let Err(e) = r {
            s.notice.set(Some(e));
        }
    });
}

#[component]
pub(crate) fn Menus(s: S) -> impl IntoView {
    view! {
        <div class="menus">
            <Show when=move || s.menus.with(Vec::is_empty)>
                <p class="empty">"No menus. A mod makes one with ui.menu(\"Title\"): it shows here and in the game (F7)."</p>
            </Show>
            <For each=move || s.menus.get() key=|m| m.id let(menu)>
                <MenuCard s=s id=menu.id module=menu.module.clone() title=menu.title.clone()/>
            </For>
        </div>
    }
}

/// A menu's card; its items follow the menu as the game sends it.
#[component]
fn MenuCard(s: S, id: u32, module: String, title: String) -> impl IntoView {
    let items = move || s.menus.with(|m| m.iter().find(|m| m.id == id).map(|m| m.items.clone()).unwrap_or_default());
    view! {
        <section class="mcard">
            // Its mod, unless the title says it already.
            <header><b>{title.clone()}</b>{(!title.eq_ignore_ascii_case(&module)).then(|| view! { <span>{module.clone()}</span> })}</header>
            <Show when=move || items().is_empty()>
                <p class="mempty">"Nothing in it yet"</p>
            </Show>
            // Keyed by what the item shows, so a change from the mod redraws it.
            <For each=items key=|i| format!("{}:{:?}:{}:{:?}", i.id, i.value, i.label, i.note) let(item)>
                {row(s, module.clone(), item)}
            </For>
        </section>
    }
}

fn row(s: S, module: String, i: MenuItem) -> impl IntoView {
    let (id, name) = (i.id, i.label.clone());
    let control = match i.kind.as_str() {
        "toggle" => {
            let on = i.value == true;
            view! {
                <label class="switch">
                    <input type="checkbox" aria-label=name prop:checked=on on:change=move |ev| send(s, module.clone(), id, json!(event_target_checked(&ev)))/>
                    <i></i>
                </label>
            }
            .into_any()
        }
        "slider" => {
            let (min, max, step) = (i.min.unwrap_or(0.0), i.max.unwrap_or(1.0), i.step.unwrap_or(0.01));
            let value = i.value.as_f64().unwrap_or(min);
            let decimals = if step.fract() == 0.0 { 0 } else { 2 };
            // Shown while dragging; sent when let go.
            let shown = RwSignal::new(value);
            view! {
                <div class="mslider">
                    <input type="range" aria-label=name min=min max=max step=step prop:value=value
                        on:input=move |ev| shown.set(event_target_value(&ev).parse().unwrap_or(value))
                        on:change=move |ev| send(s, module.clone(), id, json!(event_target_value(&ev).parse::<f64>().unwrap_or(value)))/>
                    <span>{move || format!("{:.*}", decimals, shown.get())}</span>
                </div>
            }
            .into_any()
        }
        "choice" => {
            let chosen = i.value.as_u64().unwrap_or(1) as usize;
            view! {
                <select class="mchoice" aria-label=name on:change=move |ev| send(s, module.clone(), id, json!(event_target_value(&ev).parse::<u64>().unwrap_or(1)))>
                    {i.options.iter().enumerate().map(|(n, o)| view! { <option value=n + 1 selected=n + 1 == chosen>{o.clone()}</option> }).collect_view()}
                </select>
            }
            .into_any()
        }
        "button" => view! { <button class="btn small" aria-label=name on:click=move |_| send(s, module.clone(), id, Value::Null)>"Run"</button> }.into_any(),
        _ => ().into_any(),
    };
    let label = i.kind == "label";
    view! {
        <div class=if label { "mitem label" } else { "mitem" }>
            <div class="mtext"><span>{i.label}</span>{i.note.map(|n| view! { <small>{n}</small> })}</div>
            {control}
        </div>
    }
}
