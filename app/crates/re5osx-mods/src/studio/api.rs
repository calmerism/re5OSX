//! What the Studio does: the mods as JSON, a file's diagnostics, the
//! natives' signatures and notes, `eval`, the guide.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{Value, json};

use crate::checker::{self, Checker};
use crate::{files, natives};

/// How the mods' API works (the Studio's guide).
pub const GUIDE: &str = r#"# re5OSX mods

A mod is a folder in the mods folder: `main.luau` runs when the mod starts,
other `.luau` files load with `require("./name")`, and `mod.json`, if there
is one, says what the mod is (the Studio shows it as the mod's page):
`{"name": "Trainer", "description": "...", "author": "...", "version":
"1.0", "autostart": false, "restartOnSave": true}`, every key optional;
`autostart` starts it with the game. Mods are Luau (typed Lua); start
files with `--!strict` to have every type checked.

## Running
- A mod runs in a script thread of the game's own, once the player is in
  the game. `main.luau` runs top to bottom: a mod that returns is done
  (one-shot); a mod that loops must call `wait()` in the loop.
- `wait()` gives the game its frame and goes on in the next one;
  `wait(ms)` waits that long. A mod that runs 2 s without waiting is
  stopped, and one frame never gets more than 2 ms of a mod.
- `print(...)` writes to the mod's log. An error stops the mod and logs the
  message with `file:line`.
- When a mod stops, what it created (vehicles, peds, objects) is released
  the way the game's own scripts' is.

## Natives
- The game's natives are functions in namespace tables, named in Luau's
  style: `CREATE_VEHICLE` is `vehicle.createVehicle`, `PLAYER_PED_ID` is
  `player.playerPedId`. Namespaces: entity, ped, vehicle, player, object,
  weapon, streaming, hud, graphics, pad, misc, camera, task, audio,
  network, ... (natives.d.luau has every signature).
- Handles have types: `Entity`, with `Ped`, `Vehicle`, `Object` under it,
  and `Player`, `Blip`, `Cam`, ... A native that returns a handle returns
  `nil` for none, so check before use (`if car then ... end`).
- Handles go stale: get the player's ped with `player.playerPedId()` when
  you need it rather than once at the start. A stale or made-up handle is
  an error, not a crash.
- Outputs come back as extra results: `local primary, secondary =
  vehicle.getVehicleColours(car)`.
- Positions are `vector`s: `vector.create(x, y, z)`, `a + b`, `v * 2`,
  `vector.magnitude(v)`, `v.x`.
- `Hash` parameters take a name too: `vehicle.createVehicle("adder", ...)`;
  `joaat("adder")` gives the hash.
- Models must be loaded before use: `streaming.requestModel(m)`, then
  `while not streaming.hasModelLoaded(m) do wait() end`.
- Controls: `pad.isControlJustPressed(0, control)`, with the game's control
  numbers (51 = E / context, 22 = jump, 21 = sprint).
- Names are typed from the installed game (re5osx://world.d.luau):
  `VehicleModel` ("adder", ...), `PedModel`, `WeaponName` ("WEAPON_PISTOL",
  ...). A vehicle's model parameter takes a `VehicleModel` or a hash, so a
  misspelt name is a type error; a name held in a plain `string` needs a
  cast (`name :: VehicleModel`).

## Tasks
- `task.spawn(f, ...)` runs `f` beside the main code; `task.delay(ms, f,
  ...)` later (game time); both may `wait()`. An error in a task is logged
  and the mod goes on.

## Menus
- `local m = ui.menu("Trainer")` makes a menu the app shows with its own
  widgets (touch and controllers too): the Mods page's MENUS, opened
  with F7 or, on touch screens, the button beside the settings button;
  the Studio's MENUS panel shows them too.
- Items: `m:toggle(label, on, [note], [onChange])`, `m:slider(label,
  value, min, max, [step], [onChange])`, `m:choice(label, {options},
  [index], [onChange])`, `m:button(label, [note], [onPress])`,
  `m:label(text)`. Each returns an object whose `value` follows the player
  (`:set(v)` changes it). Callbacks run as tasks: they may call natives
  and wait.
- `ui.notify(text)` shows a notification over the game.
- A mod with a menu keeps running after its main code returns, until it is
  stopped.

## World
- `world.vehicles()`, `world.peds()`, `world.objects()`: handles of the
  entities around. `world.vehicleModels()`, `world.pedModels()`,
  `world.weapons()`: the installed game's names.
- Natives that create entities are refused when the game's pool is nearly
  full; an entity of the wrong kind (a Ped where a Vehicle is wanted) is
  an error before the native runs.

## Data
- `json.encode(value, pretty?)` and `json.decode(text)`: tables with keys
  1..n (or none) are arrays, others objects; JSON's null is nil.
- `storage.get(key)`, `storage.set(key, value)` (nil removes),
  `storage.keys()`, `storage.clear()`: what the mod keeps between
  sessions, JSON values, 1 MB at most. Values are copies: change a table
  and `set` it again. Written within a couple of seconds and when the mod
  stops; kept for this player, not in the mod's folder.
- Luau's own libraries are there (math, string, table, coroutine, bit32,
  utf8, buffer, vector, os.clock/time/date); files and the network are
  not.
"#;

/// The mods with their state and files.
pub fn mods() -> Value {
    Value::Array(
        crate::mods()
            .into_iter()
            .map(|m| {
                json!({
                    "name": m.name,
                    "state": m.state.name(),
                    "error": m.error,
                    "waiting": m.waiting,
                    "cpuMs": (m.cpu_ms * 100.0).round() / 100.0,
                    "memory": m.memory,
                    "autostart": m.info.autostart,
                    "title": m.info.title,
                    "description": m.info.description,
                    "author": m.info.author,
                    "version": m.info.version,
                    "restartOnSave": m.info.restart_on_save,
                    "files": files::list(&m.name),
                })
            })
            .collect(),
    )
}

/// The mods' menus: `{mod, id, title, items}`, an item `{id, label, note,
/// kind, ...}` with `value` (toggle, slider, choice: 1-based), `min`,
/// `max`, `step` (slider), `options` (choice).
pub fn menus() -> Value {
    use crate::MenuKind;
    let item = |i: &crate::MenuItem| {
        let mut v = json!({ "id": i.id, "label": i.label, "note": i.note });
        let extra = match &i.kind {
            MenuKind::Toggle(on) => json!({ "kind": "toggle", "value": on }),
            MenuKind::Slider { value, min, max, step } => {
                json!({ "kind": "slider", "value": value, "min": min, "max": max, "step": step })
            }
            MenuKind::Choice { options, index } => json!({ "kind": "choice", "value": index + 1, "options": options }),
            MenuKind::Button => json!({ "kind": "button" }),
            MenuKind::Label => json!({ "kind": "label" }),
        };
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    };
    Value::Array(
        crate::menus()
            .iter()
            .map(|m| json!({ "mod": m.module, "id": m.id, "title": m.title, "items": m.items.iter().map(item).collect::<Vec<_>>() }))
            .collect(),
    )
}

/// Does to a menu's item what a player would: `value` sets a toggle
/// (true/false), a slider (a number) or a choice (1-based, or the option's
/// text); none presses a button.
pub fn menu_input(module: &str, item: u32, value: &Value) -> Result<(), String> {
    use crate::{MenuInput, MenuKind};
    let menus = crate::menus();
    let found = menus.iter().filter(|m| m.module == module).flat_map(|m| m.items.iter()).find(|i| i.id == item);
    let Some(found) = found else { return Err(format!("{module} has no menu item {item}")) };
    let input = match (&found.kind, value) {
        (MenuKind::Toggle(on), Value::Null) => MenuInput::Toggle(!on),
        (MenuKind::Toggle(_), Value::Bool(b)) => MenuInput::Toggle(*b),
        (MenuKind::Slider { .. }, Value::Number(n)) => MenuInput::Value(n.as_f64().unwrap_or(0.0)),
        (MenuKind::Choice { options, .. }, Value::Number(n)) => match n.as_u64() {
            Some(n) if (1..=options.len() as u64).contains(&n) => MenuInput::Choose(n as usize - 1),
            _ => return Err(format!("a choice from 1 to {}", options.len())),
        },
        (MenuKind::Choice { options, .. }, Value::String(o)) => match options.iter().position(|x| x == o) {
            Some(i) => MenuInput::Choose(i),
            None => return Err(format!("no option {o:?}; there are {options:?}")),
        },
        (MenuKind::Button, Value::Null) => MenuInput::Press,
        (MenuKind::Label, _) => return Err("a label does nothing".into()),
        (kind, v) => return Err(format!("{v} does not fit a {}", match kind {
            MenuKind::Toggle(_) => "toggle (true or false)",
            MenuKind::Slider { .. } => "slider (a number)",
            MenuKind::Choice { .. } => "choice (1-based or the option's text)",
            _ => "button (no value)",
        })),
    };
    crate::menu_input(module, item, input);
    Ok(())
}

/// What mod.json may hold, and of which JSON type.
const MOD_JSON_KEYS: &[(&str, &str)] = &[
    ("name", "text"),
    ("description", "text"),
    ("author", "text"),
    ("version", "text"),
    ("autostart", "true or false"),
    ("restartOnSave", "true or false"),
];

/// Diagnostics of a mod.json (`text`, or as saved): JSON, an object, the
/// keys it knows with values of their type.
fn mod_json(path: &str, text: Option<&str>) -> Value {
    let text = match text {
        Some(t) => t.to_owned(),
        None => files::read(path).unwrap_or_default(),
    };
    let diag = |line: usize, col: usize, len: usize, severity: &str, message: String| {
        json!({ "range": [line, col, line, col + len], "severity": severity, "code": "mod.json", "message": message })
    };
    // Where `"key"` is (0-based line and column).
    let at = |key: &str| {
        let quoted = format!("\"{key}\"");
        text.lines().enumerate().find_map(|(i, l)| l.find(&quoted).map(|c| (i, c, quoted.len()))).unwrap_or((0, 0, 1))
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let (line, col) = (e.line().saturating_sub(1), e.column().saturating_sub(1));
            return json!([diag(line, col, 1, "error", format!("not JSON: {e}"))]);
        }
    };
    let Some(map) = v.as_object() else {
        return json!([diag(0, 0, 1, "error", "mod.json is an object: {\"autostart\": true}".into())]);
    };
    let mut out = Vec::new();
    for (key, value) in map {
        let (line, col, len) = at(key);
        match MOD_JSON_KEYS.iter().find(|(k, _)| k == key) {
            Some((_, "true or false")) if !value.is_boolean() => {
                out.push(diag(line, col, len, "error", format!("{key} is true or false")));
            }
            Some((_, "text")) if !value.is_string() => {
                out.push(diag(line, col, len, "error", format!("{key} is text, in quotes")));
            }
            Some(_) => {}
            None => {
                let known: Vec<&str> = MOD_JSON_KEYS.iter().map(|(k, _)| *k).collect();
                out.push(diag(line, col, len, "warning", format!("unknown key {key}: mod.json knows {}", known.join(", "))));
            }
        }
    }
    Value::Array(out)
}

/// Diagnostics of `path` as the checker sees it; `text` replaces the file
/// on disk (an editor's unsaved text). The mod's other files are read from
/// disk when the checker has not got them.
pub fn diagnostics(path: &str, text: Option<&str>) -> Value {
    let module = path.trim_start_matches('/');
    if module.ends_with("/mod.json") {
        return mod_json(module, text);
    }
    if !module.ends_with(".luau") {
        // Other files are the mod's own business.
        return json!([]);
    }
    let result = Checker::with(|c| {
        match text {
            Some(t) => c.set(module, Some(t)),
            // As saved.
            None => c.set(module, files::read(module).ok().as_deref()),
        }
        sync_mod(c, module);
        c.check(module)
    });
    let mut found =
        result.unwrap_or_else(|e| json!([{ "range": [0, 0, 0, 0], "severity": "error", "code": "checker", "message": e }]));
    if let Some(list) = found.as_array_mut() {
        for d in list {
            let message = d["message"].as_str().unwrap_or("");
            d["message"] = name_message(message).unwrap_or_else(|| crate::world::shorten(message)).into();
        }
    }
    found
}

/// A misspelt name against the catalog's union (hundreds of names in the
/// checker's message): which kind of name, and the closest one.
fn name_message(message: &str) -> Option<String> {
    let expected = message.strip_prefix("Expected this to be")?.trim_start().trim_start_matches('\'');
    if !expected.starts_with('"') {
        return None;
    }
    let got = message.rsplit("but got '\"").next()?.split('"').next()?;
    let c = crate::world::catalog()?;
    let (kind, names) = if expected.starts_with("\"WEAPON_") {
        ("a weapon", &c.weapons)
    } else if c.vehicles.first().is_some_and(|v| expected.starts_with(&format!("\"{v}\""))) {
        ("a vehicle model", &c.vehicles)
    } else if c.peds.first().is_some_and(|p| expected.starts_with(&format!("\"{p}\""))) {
        ("a ped model", &c.peds)
    } else {
        return None;
    };
    let closest = names.iter().min_by_key(|n| distance(&n.to_ascii_lowercase(), &got.to_ascii_lowercase()))?;
    Some(format!("\"{got}\" is not {kind} of the installed game; did you mean \"{closest}\"?"))
}

/// Levenshtein distance.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Puts the files of `module`'s mod the checker has not got, from disk.
pub(crate) fn sync_mod(c: &mut Checker, module: &str) {
    let Some((name, _)) = module.split_once('/') else { return };
    for file in files::list(name) {
        if file.ends_with(".luau")
            && !c.has(&file)
            && let Ok(text) = files::read(&file)
        {
            c.set(&file, Some(&text));
        }
    }
}

/// Signatures by `namespace.name`, from the definitions.
fn signatures() -> &'static HashMap<String, String> {
    static SIGS: OnceLock<HashMap<String, String>> = OnceLock::new();
    SIGS.get_or_init(|| {
        let mut map = HashMap::new();
        let mut space = "";
        for line in natives::DEFINITIONS.lines() {
            if let Some(rest) = line.strip_prefix("declare ") {
                space = rest.split(':').next().unwrap_or("");
            } else if let Some(entry) = line.strip_prefix('\t')
                && let Some((name, sig)) = entry.split_once(": ")
            {
                map.insert(format!("{space}.{name}"), format!("{space}.{name}{}", sig.trim_end_matches(',')));
            }
        }
        map
    })
}

/// The natives whose names hold every word of `query` (case aside), best
/// first: name then notes.
pub fn search(query: &str, limit: usize) -> Vec<Value> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_ascii_lowercase().replace('_', "")).collect();
    let mut hits: Vec<(u8, &natives::Native)> = Vec::new();
    for n in natives::NATIVES {
        let name = format!("{}.{}", natives::NAMESPACES[n.ns as usize], n.lua).to_ascii_lowercase();
        let doc = n.doc.to_ascii_lowercase();
        let score = if words.iter().all(|w| name.contains(w.as_str())) {
            0
        } else if words.iter().all(|w| name.contains(w.as_str()) || doc.contains(w.as_str())) {
            1
        } else {
            continue;
        };
        hits.push((score, n));
    }
    hits.sort_by_key(|(s, n)| (*s, n.lua.len()));
    hits.into_iter().take(limit).map(|(_, n)| describe(n, false)).collect()
}

/// A native by `namespace.name` or the game's `NAME`.
pub fn find(name: &str) -> Option<&'static natives::Native> {
    if let Some((space, lua)) = name.split_once('.') {
        let ns = natives::NAMESPACES.iter().position(|s| *s == space)?;
        return natives::find(ns, lua).map(|i| &natives::NATIVES[i]);
    }
    natives::NATIVES.iter().find(|n| n.name.eq_ignore_ascii_case(name))
}

/// A native's signature, game name and (with `doc`) notes.
pub fn describe(n: &natives::Native, doc: bool) -> Value {
    let key = format!("{}.{}", natives::NAMESPACES[n.ns as usize], n.lua);
    let mut v = json!({
        "name": key,
        "native": n.name,
        "signature": signatures().get(&key).cloned().unwrap_or_else(|| key.clone()),
    });
    if doc && !n.doc.is_empty() {
        v["notes"] = n.doc.into();
    }
    v
}

/// The notes on a documentation symbol, as Markdown.
pub fn documentation(symbol: &str) -> Option<String> {
    checker::documentation(symbol)
}

/// Runs `code` in the game and waits for the result (`timeout` at most).
pub fn eval(code: &str, timeout: Duration) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::eval(code, tx);
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("no result within {} s (is the game paused or loading?)", timeout.as_secs()))
        }
        Err(_) => Err("the evaluation stopped (an error ended it, or it ran too long without wait())".into()),
    }
}

/// Log lines as JSON, from `since` (and only mod `module`'s).
pub fn logs(since: u64, module: Option<&str>) -> (Vec<Value>, u64) {
    let (lines, next) = crate::logs(since);
    let lines = lines
        .into_iter()
        .filter(|l| module.is_none_or(|m| m == l.module))
        .map(|l| {
            let ms = l.time.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            json!({
                "seq": l.seq,
                "time": ms,
                "mod": l.module,
                "level": if l.level == crate::LogLevel::Error { "error" } else { "info" },
                "text": l.text,
            })
        })
        .collect();
    (lines, next)
}
