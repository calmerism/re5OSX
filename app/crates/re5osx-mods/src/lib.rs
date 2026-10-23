//! Mods: Luau scripts, each in a script thread of the game's own, calling
//! the game's natives.
//!
//! A mod is a folder in the mods folder with a `main.luau` and, optionally,
//! a `mod.json` (`{"autostart": true}`). The application calls [`install`]
//! before the game starts, then [`start`], [`stop`], [`mods`] and [`logs`]
//! from any thread; the mods themselves run on the engine's script update
//! (engine.rs). [`studio`] serves re5OSX Studio.

mod checker;
mod engine;
pub mod files;
mod json;
mod menus;
pub mod natives;
mod storage;
mod zip;
pub mod studio;
mod vm;
pub mod world;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModState {
    Stopped,
    Running,
    Error,
}

impl ModState {
    pub fn name(self) -> &'static str {
        match self {
            ModState::Stopped => "stopped",
            ModState::Running => "running",
            ModState::Error => "error",
        }
    }
}

/// What a mod says of itself: its `mod.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModInfo {
    /// As people see it (the folder's name when it has none).
    pub title: String,
    pub description: String,
    pub author: String,
    pub version: String,
    /// It starts with the game.
    pub autostart: bool,
    /// Saving one of its files restarts it while it runs.
    pub restart_on_save: bool,
}

/// A mod, as the application shows it.
#[derive(Clone, Debug)]
pub struct Mod {
    pub name: String,
    pub info: ModInfo,
    pub state: ModState,
    /// Why it stopped, when it failed.
    pub error: Option<String>,
    /// Time it takes per frame (smoothed), in milliseconds.
    pub cpu_ms: f32,
    /// Memory of its VM, in bytes.
    pub memory: usize,
    /// It waits (the engine counts the time).
    pub waiting: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    /// Increasing: what a reader has seen is what is below its cursor.
    pub seq: u64,
    pub time: SystemTime,
    pub module: String,
    pub level: LogLevel,
    pub text: String,
}

/// What `eval` gives back: the values as text, or the error.
pub type EvalResult = Result<String, String>;

enum Command {
    Start(String),
    Stop(String),
    Eval(String, Sender<EvalResult>),
    /// Empties a running mod's storage.
    ClearStorage(String),
}

struct State {
    mods: Vec<Mod>,
    commands: Vec<Command>,
    log: VecDeque<LogLine>,
    next_seq: u64,
    /// Files written, deleted or renamed, with a sequence number.
    changes: VecDeque<(u64, String)>,
    next_change: u64,
    /// The mods' menus, what was done with them (for the mods), what they
    /// notify (for the application).
    menus: Vec<Menu>,
    inputs: Vec<(String, u32, MenuInput)>,
    notices: VecDeque<(String, String)>,
    next_menu_id: u32,
}

struct Shared {
    dir: OnceLock<PathBuf>,
    state: Mutex<State>,
    /// Commands are waiting for the engine.
    pending: AtomicBool,
    /// Changes when a mod's state does (for readers that push it).
    generation: AtomicU64,
}

static SHARED: Shared = Shared {
    dir: OnceLock::new(),
    state: Mutex::new(State {
        mods: Vec::new(),
        commands: Vec::new(),
        log: VecDeque::new(),
        next_seq: 1,
        changes: VecDeque::new(),
        next_change: 1,
        menus: Vec::new(),
        inputs: Vec::new(),
        notices: VecDeque::new(),
        next_menu_id: 1,
    }),
    pending: AtomicBool::new(false),
    generation: AtomicU64::new(0),
};

const LOG_LINES: usize = 2000;
/// The longest log line kept (a mod may print megabytes): past it the
/// line is cut.
const LOG_LINE: usize = 8 << 10;

/// The mod whose turn it is (empty between turns), for the failure note.
static RUNNING_MOD: Mutex<String> = Mutex::new(String::new());

/// Mods kept from starting with the game: they ran when it last stopped.
static HELD_BACK: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The game has stopped working: which mods ran, and what a mod was doing
/// then (none: no mod was running). Saved, so that the next start does not
/// start them by itself.
pub fn note_failure() -> Option<String> {
    let dir = dir()?;
    let running: Vec<String> =
        mods().into_iter().filter(|m| m.state == ModState::Running).map(|m| m.name).collect();
    let during = RUNNING_MOD.try_lock().map(|m| m.clone()).unwrap_or_default();
    let native = match natives::RUNNING.load(Ordering::Relaxed) {
        0 => None,
        i => natives::NATIVES.get(i - 1).map(natives::qualified),
    };
    if running.is_empty() && during.is_empty() {
        return None;
    }
    let mut names = running.clone();
    if !during.is_empty() && !names.contains(&during) {
        names.push(during.clone());
    }
    let note = serde_json::json!({ "mods": names, "during": during, "native": native });
    let _ = std::fs::create_dir_all(dir.join(".re5osx"));
    let _ = std::fs::write(dir.join(".re5osx/failed.json"), note.to_string());
    Some(match (during.is_empty(), native) {
        (false, Some(n)) => format!("Mod {during} was calling {n}."),
        (false, None) => format!("Mod {during} was running."),
        _ => format!("Mods running: {}.", running.join(", ")),
    })
}

/// The mods kept from starting with the game this time, and why.
pub fn held_back() -> Vec<String> {
    crate::lock(&HELD_BACK).clone()
}

/// The mods folder `dir` (made if missing): its mods listed, their files
/// open to the Studio. Before the game too; they run once [`install`]ed.
pub fn open(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    if SHARED.dir.set(dir.to_path_buf()).is_err() {
        return;
    }
    world::load_saved(dir);
    refresh();
}

/// Mods run with the game about to start (the folder [`open`]ed if it was
/// not): the mods marked autostart start once the game runs scripts. Call
/// before `re5osx_runtime::start`.
pub fn install(dir: &Path) {
    open(dir);
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return;
    }
    let dir = self::dir().unwrap_or(dir);
    re5osx_runtime::add_hooks(engine::hooks);
    // The mods' script threads get stacks of their own.
    re5osx_runtime::overlay::more_script_stacks(32);
    log::info!("mods: {} ({})", dir.display(), natives::SUMMARY);
    // The game stopped while these ran: not by themselves this time.
    let failed = dir.join(".re5osx/failed.json");
    if let Ok(text) = std::fs::read_to_string(&failed) {
        let _ = std::fs::remove_file(&failed);
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let names: Vec<String> =
            v["mods"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
        for n in &names {
            log_line(n, LogLevel::Error, "the game stopped while this mod ran: it does not start by itself this time");
        }
        if !names.is_empty() {
            let text = format!("{} did not start: the game stopped while it ran last time.", names.join(", "));
            crate::lock(&SHARED.state).notices.push_back(("mods".into(), text));
        }
        *crate::lock(&HELD_BACK) = names;
    }
    let held = held_back();
    let autostart: Vec<String> =
        mods().into_iter().filter(|m| m.info.autostart && !held.contains(&m.name)).map(|m| m.name).collect();
    for name in autostart {
        start(&name);
    }
}

/// A mutex's value, also after a panic while it was held (the panic was
/// caught and its mod stopped; what it guards stays usable).
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The mods folder, once open.
pub fn dir() -> Option<&'static Path> {
    SHARED.dir.get().map(PathBuf::as_path)
}

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether mods run with the game (it started with them).
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Why a mod cannot be started now, if it cannot.
pub fn cannot_start() -> Option<&'static str> {
    (!installed()).then_some("mods run with the game, and it is not running with them: start it (mods on), or mark the mod to start with the game")
}

/// A mod's file was saved (`path`), from the Studio or the files: the
/// mod restarts with it when it runs and its page says so ("Restart when a
/// file is saved"). Whether it restarts.
pub fn saved(path: &str) -> bool {
    let Some((name, file)) = path.split_once('/') else { return false };
    if !file.ends_with(".luau") {
        return false;
    }
    let restarts = mods().iter().any(|m| m.name == name && m.state == ModState::Running && m.info.restart_on_save);
    if restarts {
        restart(name);
    }
    restarts
}

/// What mod `name` keeps in its storage, as JSON (`{}` when nothing).
pub fn stored(name: &str) -> String {
    if !files::valid_name(name) {
        return "{}".into();
    }
    dir()
        .and_then(|d| std::fs::read_to_string(d.join(".re5osx/storage").join(format!("{name}.json"))).ok())
        .unwrap_or_else(|| "{}".into())
}

/// Empties mod `name`'s storage (a running mod's too).
pub fn clear_stored(name: &str) {
    if !files::valid_name(name) {
        return;
    }
    if let Some(d) = dir() {
        let _ = std::fs::remove_file(d.join(".re5osx/storage").join(format!("{name}.json")));
    }
    command(Command::ClearStorage(name.to_string()));
}

/// Whether mod `name` starts with the game.
pub fn set_autostart(name: &str, on: bool) -> Result<(), String> {
    let mut info = files::info(name);
    info.autostart = on;
    files::set_info(name, &info)
}

/// Reads the mods folder again (new or removed mods).
pub fn refresh() {
    let Some(dir) = dir() else { return };
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("main.luau").is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                found.push((name.clone(), files::info_at(&path, &name)));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mut s = crate::lock(&SHARED.state);
    let before = s.mods.len();
    s.mods.retain(|m| m.state != ModState::Stopped || found.iter().any(|(n, _)| *n == m.name));
    let mut changed = before != s.mods.len();
    for (name, info) in found {
        match s.mods.iter_mut().find(|m| m.name == name) {
            Some(m) => {
                changed |= m.info != info;
                m.info = info;
            }
            None => {
                changed = true;
                s.mods.push(Mod {
                    name,
                    state: ModState::Stopped,
                    error: None,
                    cpu_ms: 0.0,
                    memory: 0,
                    info,
                    waiting: false,
                })
            }
        }
    }
    // As people read them: by the name shown, numbers as numbers.
    s.mods.sort_by(|a, b| natural(&a.info.title, &b.info.title).then_with(|| a.name.cmp(&b.name)));
    if changed {
        SHARED.generation.fetch_add(1, Ordering::Relaxed);
    }
}

/// Text compared as people expect: case aside, digits as numbers
/// (`filler-2` before `filler-10`).
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = 0u64;
                    while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                        n = n.saturating_mul(10).saturating_add(u64::from(d));
                        it.next();
                    }
                    n
                };
                let ord = number(&mut a).cmp(&number(&mut b));
                if ord.is_ne() {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord.is_ne() {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The mods and their state.
pub fn mods() -> Vec<Mod> {
    crate::lock(&SHARED.state).mods.clone()
}

/// Changes whenever [`mods`] would give something else (but the time and
/// memory figures).
pub fn generation() -> u64 {
    SHARED.generation.load(Ordering::Relaxed)
}

/// Starts mod `name` (again, if it ran before).
pub fn start(name: &str) {
    // A folder of the mods folder, nothing else.
    if files::valid_name(name) {
        command(Command::Start(name.to_string()));
    }
}

/// Stops mod `name`: its thread ends, with what it created.
pub fn stop(name: &str) {
    if files::valid_name(name) {
        command(Command::Stop(name.to_string()));
    }
}

/// Stops mod `name` if it runs, then starts it.
pub fn restart(name: &str) {
    stop(name);
    start(name);
}

/// Runs Luau `code` in the game (in a script thread of its own, as a mod),
/// as an expression if it is one. `reply` gets the values as text.
pub fn eval(code: &str, reply: Sender<EvalResult>) {
    if !installed() {
        let _ = reply.send(Err("the game is not running with mods: they run from the next start".into()));
        return;
    }
    command(Command::Eval(code.to_string(), reply));
}

fn command(c: Command) {
    if !installed() {
        return;
    }
    crate::lock(&SHARED.state).commands.push(c);
    SHARED.pending.store(true, Ordering::Release);
}

/// The log lines after `seq` (0: all kept), and the cursor to pass next.
pub fn logs(seq: u64) -> (Vec<LogLine>, u64) {
    let s = crate::lock(&SHARED.state);
    // From the end: the lines are in order, and callers ask for the newest.
    let new = s.log.iter().rev().take_while(|l| l.seq > seq).count();
    let lines: Vec<LogLine> = s.log.iter().skip(s.log.len() - new).cloned().collect();
    (lines, s.next_seq - 1)
}

/// The last `n` log lines.
pub fn recent_logs(n: usize) -> Vec<LogLine> {
    let s = crate::lock(&SHARED.state);
    s.log.iter().skip(s.log.len().saturating_sub(n)).cloned().collect()
}

/// The files changed after `seq` (by any client), and the cursor to pass next.
pub fn changes(seq: u64) -> (Vec<String>, u64) {
    let s = crate::lock(&SHARED.state);
    let paths = s.changes.iter().filter(|(n, _)| *n > seq).map(|(_, p)| p.clone()).collect();
    (paths, s.next_change - 1)
}

pub(crate) fn changed(path: &str) {
    let mut s = crate::lock(&SHARED.state);
    if s.changes.len() == 256 {
        s.changes.pop_front();
    }
    let n = s.next_change;
    s.next_change += 1;
    s.changes.push_back((n, path.to_string()));
}

/// A mod's menu: what the application shows for it (`ui.menu` in Luau).
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub module: String,
    /// The run of the mod that made it (a restarted mod's menus are its
    /// new run's; the old run's go with it).
    pub instance: u64,
    pub id: u32,
    pub title: String,
    pub items: Vec<MenuItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
    /// A line under the label.
    pub note: Option<String>,
    pub kind: MenuKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuKind {
    Toggle(bool),
    Slider { value: f64, min: f64, max: f64, step: f64 },
    Choice { options: Vec<String>, index: usize },
    Button,
    Label,
}

/// What a player did with a menu's item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuInput {
    Toggle(bool),
    Value(f64),
    Choose(usize),
    Press,
}

/// The mods' menus.
pub fn menus() -> Vec<Menu> {
    crate::lock(&SHARED.state).menus.clone()
}

/// Whether a mod has a menu.
pub fn has_menus() -> bool {
    !crate::lock(&SHARED.state).menus.is_empty()
}

/// The player changed or pressed an item: the menu shows it at once, the
/// mod hears of it at its next turn.
pub fn menu_input(module: &str, item: u32, input: MenuInput) {
    let mut s = crate::lock(&SHARED.state);
    // What the mod hears is what the menu shows: the value checked against
    // the item, nothing for an item that is gone or of another kind.
    let mut checked = None;
    for m in s.menus.iter_mut().filter(|m| m.module == module) {
        for i in m.items.iter_mut().filter(|i| i.id == item) {
            checked = match (&mut i.kind, input) {
                (MenuKind::Toggle(on), MenuInput::Toggle(v)) => {
                    *on = v;
                    Some(input)
                }
                (MenuKind::Slider { value, min, max, .. }, MenuInput::Value(v)) if v.is_finite() => {
                    *value = v.clamp(*min, *max);
                    Some(MenuInput::Value(*value))
                }
                (MenuKind::Choice { options, index }, MenuInput::Choose(v)) if v < options.len() => {
                    *index = v;
                    Some(input)
                }
                (MenuKind::Button, MenuInput::Press) => Some(input),
                _ => None,
            };
        }
    }
    let Some(input) = checked else { return };
    s.inputs.push((module.to_string(), item, input));
    drop(s);
    SHARED.pending.store(true, Ordering::Release);
    SHARED.generation.fetch_add(1, Ordering::Relaxed);
}

/// What the mods notified since the last call: (mod, text).
pub fn take_notices() -> Vec<(String, String)> {
    crate::lock(&SHARED.state).notices.drain(..).collect()
}

/// Whether a native trapped: the game may now misbehave.
pub fn tainted() -> bool {
    natives::TAINTED.load(Ordering::Relaxed)
}

pub(crate) fn log_line(module: &str, level: LogLevel, text: &str) {
    let cut;
    let text = if text.len() > LOG_LINE {
        let head = &text[..text.floor_char_boundary(LOG_LINE)];
        cut = format!("{head}… ({} bytes more)", text.len() - head.len());
        &cut
    } else {
        text
    };
    match level {
        LogLevel::Info => log::info!("mod {module}: {text}"),
        LogLevel::Error => log::error!("mod {module}: {text}"),
    }
    let mut s = crate::lock(&SHARED.state);
    if s.log.len() == LOG_LINES {
        s.log.pop_front();
    }
    let seq = s.next_seq;
    s.next_seq += 1;
    s.log.push_back(LogLine { seq, time: SystemTime::now(), module: module.to_string(), level, text: text.to_string() });
}

#[cfg(test)]
mod tests {
    /// A mod that prints megabytes leaves a line of 8 KB and a note of how
    /// much was cut.
    #[test]
    fn long_log_lines_are_cut() {
        let text = format!("{}tail", "é".repeat(10_000));
        super::log_line("log-test", super::LogLevel::Info, &text);
        let line = super::recent_logs(super::LOG_LINES).into_iter().rev().find(|l| l.module == "log-test").unwrap();
        assert!(line.text.len() < super::LOG_LINE + 64, "{}", line.text.len());
        assert!(line.text.ends_with(&format!("… ({} bytes more)", text.len() - super::LOG_LINE)), "{}", &line.text[line.text.len() - 40..]);
        super::log_line("log-test", super::LogLevel::Info, "short");
        assert_eq!(super::recent_logs(super::LOG_LINES).into_iter().rev().find(|l| l.module == "log-test").unwrap().text, "short");
    }
}
