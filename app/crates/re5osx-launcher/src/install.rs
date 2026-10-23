//! Making the game app on this Mac: it holds the game's code, so it is never
//! downloaded. The kit in re5OSX has everything else, built already;
//! `link-app` compiles the player's game.wasm with Apple's Command Line Tools
//! and links the two into the user folder, where re5OSX opens it from.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::paths;

/// Where Install is, as the page reads it (`/__re5osx/info`): state (idle,
/// running, done, failed), the step's text, progress from 0 to 1, and the
/// error when it failed.
static STATUS: Mutex<Option<Value>> = Mutex::new(None);

pub fn status() -> Value {
    STATUS.lock().unwrap().clone().unwrap_or_else(|| json!({"state": "idle", "step": "", "progress": 0.0, "error": null}))
}

/// Changes the status's fields in place.
fn update(f: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    let mut s = STATUS.lock().unwrap();
    let mut v = s.clone().unwrap_or_else(status);
    if let Some(map) = v.as_object_mut() {
        f(map);
    }
    *s = Some(v);
}

/// A new step; progress only ever moves forward.
fn step(text: &str, progress: Option<f64>) {
    update(|m| {
        m.insert("step".into(), text.into());
        if let Some(p) = progress {
            let now = m.get("progress").and_then(Value::as_f64).unwrap_or(0.0);
            m.insert("progress".into(), now.max(p).into());
        }
    });
}

/// Starts Install on a thread of its own (once at a time); the game opens
/// when it is done.
pub fn start(game: &Path) {
    if status()["state"] == "running" {
        return;
    }
    *STATUS.lock().unwrap() = Some(json!({"state": "running", "step": "Checking this Mac", "progress": 0.0, "error": null}));
    let game = game.to_path_buf();
    thread::spawn(move || {
        match install(&game) {
            Ok(()) => {
                update(|m| {
                    m.insert("state".into(), "done".into());
                    m.insert("step".into(), "Ready".into());
                    m.insert("progress".into(), 1.0.into());
                });
                // Made: the game opens, and this window has done its job.
                crate::open_game(&game);
            }
            Err(e) => update(|m| {
                m.insert("state".into(), "failed".into());
                m.insert("error".into(), format!("{e:#}").into());
            }),
        }
    });
    // The game's compile prints a line a part, minutes apart at first: the
    // bar keeps moving a little meanwhile, never past 90 %, so it does not
    // look stuck.
    thread::spawn(|| {
        while status()["state"] == "running" {
            thread::sleep(Duration::from_secs(5));
            update(|m| {
                let p = m.get("progress").and_then(Value::as_f64).unwrap_or(0.0);
                if p < 0.9 {
                    m.insert("progress".into(), (p + 0.002).min(0.9).into());
                }
            });
        }
    });
}

/// Apple's Command Line Tools (clang, ld): asks macOS to install them when they are missing.
fn command_line_tools() -> Result<()> {
    let ok = Command::new("/usr/bin/xcode-select").arg("-p").output().is_ok_and(|o| o.status.success());
    if ok {
        return Ok(());
    }
    let _ = Command::new("/usr/bin/xcode-select").arg("--install").spawn();
    bail!(
        "re5OSX needs Apple's Command Line Tools to build the game. Install them from the window that just opened (a few minutes), then press Install again."
    )
}

/// Runs the kit's link-app on the game folder, its progress turned into
/// steps ("[n/64]" lines are the game's 64 parts), then puts the app in place
/// with a stamp of what made it.
fn install(game: &Path) -> Result<()> {
    command_line_tools()?;
    let kit = paths::kit();
    let link = kit.join("link-app");
    if !link.exists() {
        bail!("this copy of re5OSX has no kit for re5OSX ({})", kit.display());
    }
    let work = paths::support().join("build");
    let built = work.join("re5OSX.app");
    let log_path = paths::support().join("install.log");
    let mut log = fs::File::create(&log_path)?;
    step("Starting", Some(0.02));
    let mut child = Command::new(&link)
        .arg(&kit)
        .arg(game)
        .arg(&work)
        .arg(&built)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting link-app")?;
    let errors = child.stderr.take().unwrap();
    let error_log = log.try_clone()?;
    let stderr = thread::spawn(move || {
        let mut log = error_log;
        let mut last = Vec::new();
        for line in BufReader::new(errors).lines().map_while(Result::ok) {
            let _ = writeln!(log, "{line}");
            last.push(line);
            if last.len() > 5 {
                last.remove(0);
            }
        }
        last
    });
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let _ = writeln!(log, "{line}");
        if let Some(text) = line.strip_prefix("step: ") {
            let (text, progress) = match text {
                t if t.starts_with("Compiling") => ("Compiling", 0.05),
                "Linking" => ("Putting it together", 0.9),
                "Making the app" => ("Almost done", 0.95),
                t => (t, 0.0),
            };
            step(text, Some(progress));
        } else if let Some(part) = line.strip_prefix('[')
            && let Some((n, rest)) = part.split_once('/')
            && let Some((total, _)) = rest.split_once(']')
            && let (Ok(n), Ok(total)) = (n.parse::<f64>(), total.parse::<f64>())
        {
            step(&format!("Compiling {n} of {total}"), Some(0.05 + 0.85 * n / total));
        }
    }
    let last = stderr.join().unwrap_or_default();
    if !child.wait()?.success() {
        bail!("The build stopped: {} (details in {})", last.join(" "), log_path.display());
    }
    step("Almost done", Some(0.97));
    let app = paths::game_app();
    fs::create_dir_all(app.parent().unwrap())?;
    let _ = fs::remove_dir_all(&app);
    fs::rename(&built, &app).context("putting the game app in place")?;
    fs::write(paths::support().join("game/made-by"), paths::kit_stamp())?;
    Ok(())
}
