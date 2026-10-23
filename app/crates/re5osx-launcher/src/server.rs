//! What the setup window's page asks for: its own files, the game folder and
//! its setup, and making the game app. Served on 127.0.0.1 only.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

use crate::http::{self, Body, Request, Response};
use crate::{install, paths, setup};

/// Setup's run (setup.rs) on a thread: idle, running, done or failed, and
/// the lines it has said so far.
#[derive(Default)]
struct Setup {
    state: &'static str,
    log: Vec<String>,
    error: Option<String>,
}

/// The setup window's state: the game folder (kept in launcher.json) and
/// setup's run. Install keeps its own (install.rs).
pub struct Server {
    game: Mutex<Option<PathBuf>>,
    setup: Arc<Mutex<Setup>>,
}

/// The game folder chosen before, if it still holds the game and setup has
/// made what it makes.
pub fn ready_game() -> Option<PathBuf> {
    let game = load_config().get("game").and_then(Value::as_str).filter(|g| !g.is_empty()).map(PathBuf::from)?;
    (setup::check_game(&game).0 && setup::assets_ready()).then_some(game)
}

fn load_config() -> serde_json::Map<String, Value> {
    fs::read(paths::config()).ok().and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

fn save_config(config: serde_json::Map<String, Value>) {
    let _ = fs::create_dir_all(paths::support());
    let _ = fs::write(paths::config(), serde_json::to_string_pretty(&Value::Object(config)).unwrap_or_default());
}

impl Server {
    pub fn new() -> Arc<Server> {
        let game = load_config().get("game").and_then(Value::as_str).filter(|g| !g.is_empty()).map(PathBuf::from);
        Arc::new(Server {
            game: Mutex::new(game),
            setup: Arc::new(Mutex::new(Setup { state: "idle", ..Default::default() })),
        })
    }

    fn game(&self) -> Option<PathBuf> {
        self.game.lock().unwrap().clone()
    }

    /// The folder, whether it holds the game, what it lacks, and whether
    /// setup has made the assets.
    fn game_status(&self) -> Value {
        let s = self.setup.lock().unwrap();
        let setup = json!({"state": s.state, "log": s.log, "error": s.error});
        drop(s);
        let Some(game) = self.game() else {
            return json!({"path": null, "ok": false, "missing": [], "ready": false, "setup": setup});
        };
        let (ok, _, missing) = setup::check_game(&game);
        let ready = ok && setup::assets_ready();
        json!({"path": game, "ok": ok, "missing": missing, "ready": ready, "setup": setup})
    }

    /// Takes a folder: kept if it holds the game, then set up.
    fn choose(&self, path: &str) -> Value {
        let path = expand(path);
        let path = path.canonicalize().unwrap_or(path);
        let (ok, _, missing) = setup::check_game(&path);
        if !ok {
            return json!({"ok": false, "path": path, "missing": missing});
        }
        *self.game.lock().unwrap() = Some(path.clone());
        let mut config = load_config();
        config.insert("game".into(), path.to_string_lossy().into());
        save_config(config);
        self.run_setup();
        json!({"ok": true, "path": path})
    }

    /// Runs setup on the folder on a thread of its own (once at a time).
    fn run_setup(&self) {
        let Some(game) = self.game() else { return };
        {
            let mut s = self.setup.lock().unwrap();
            if s.state == "running" {
                return;
            }
            *s = Setup { state: "running", ..Default::default() };
        }
        let state = self.setup.clone();
        thread::spawn(move || {
            let log_state = state.clone();
            let log = move |line: String| log_state.lock().unwrap().log.push(line);
            let result = setup::setup(&game, &log);
            let mut s = state.lock().unwrap();
            match result {
                Ok(_) => s.state = "done",
                Err(e) => (s.state, s.error) = ("failed", Some(format!("{e:#}"))),
            }
        });
    }

    /// Every request: GET for the page's files and `/__re5osx/info`, POST for
    /// what the page asks to be done.
    pub fn handle(self: &Arc<Self>, r: &Request) -> Response {
        if !r.local {
            return Response::error(403, "only from this Mac");
        }
        let response = match r.method.as_str() {
            "GET" | "HEAD" => self.get(r),
            "POST" => self.control(r),
            _ => Response::error(404, "not found"),
        };
        response.with("Cache-Control", "no-cache")
    }

    /// The state, or one of the page's own files.
    fn get(&self, r: &Request) -> Response {
        if r.path == "/__re5osx/info" {
            return Response::json(&json!({
                "made": paths::game_app_current(),
                "game": self.game_status(),
                "install": install::status(),
            }));
        }
        let rel = r.path.trim_start_matches('/');
        if rel.split('/').any(|p| p == "..") {
            return Response::error(404, "not found");
        }
        let file = paths::web().join(if rel.is_empty() { "index.html" } else { rel });
        match fs::File::open(&file) {
            Ok(f) => match f.metadata() {
                Ok(m) if m.is_file() => Response {
                    status: 200,
                    headers: vec![("Content-Type".into(), http::mime(&file).into())],
                    body: Body::File { file: f, start: 0, len: m.len() },
                },
                _ => Response::error(404, "not found"),
            },
            Err(_) => Response::error(404, "not found"),
        }
    }

    /// What the page asks for: a folder (typed or picked), setup again,
    /// wallpapers, Install, Play, or closing the window.
    fn control(&self, r: &Request) -> Response {
        let body: Value = serde_json::from_slice(&r.body).unwrap_or(json!({}));
        match r.path.as_str() {
            "/__re5osx/game" => Response::json(&self.choose(body["path"].as_str().unwrap_or(""))),
            "/__re5osx/choose" => match choose_folder("Choose your GTA V web build folder (the one with b and data inside)") {
                Some(chosen) => Response::json(&self.choose(&chosen)),
                None => Response::json(&json!({"ok": false, "cancelled": true})),
            },
            "/__re5osx/setup" => {
                if self.game_status()["ok"] != true {
                    return Response::json_status(400, &json!({"error": "Choose your game folder first."}));
                }
                self.run_setup();
                Response::json(&json!({"started": true}))
            }
            "/__re5osx/wallpapers" => {
                let Some(chosen) = choose_folder("Choose the folder with your GTA V wallpapers (Artwork-<Name>-GTAV.jpg)") else {
                    return Response::json(&json!({"cancelled": true}));
                };
                let count = setup::use_wallpapers(Path::new(&chosen)).unwrap_or(0);
                if count > 0 {
                    self.run_setup();
                }
                Response::json(&json!({"wallpapers": count}))
            }
            "/__re5osx/install" => match self.game() {
                Some(game) => {
                    install::start(&game);
                    Response::json(&install::status())
                }
                None => Response::json_status(400, &json!({"error": "Choose your game folder first."})),
            },
            "/__re5osx/play" => match self.game() {
                Some(game) if paths::game_app_current() => {
                    crate::open_game(&game);
                    Response::json(&json!({"playing": true}))
                }
                _ => Response::json_status(404, &json!({"error": "The game isn't installed yet."})),
            },
            "/__re5osx/quit" => {
                crate::QUIT.store(true, Ordering::Relaxed);
                Response::json(&json!({"quitting": true}))
            }
            _ => Response::error(404, "not found"),
        }
    }
}

fn expand(path: &str) -> PathBuf {
    match path.trim().strip_prefix("~/") {
        Some(rest) => paths::home().join(rest),
        None => PathBuf::from(path.trim()),
    }
}

/// macOS's folder picker: the folder's path, or None when cancelled.
fn choose_folder(prompt: &str) -> Option<String> {
    let script = format!("POSIX path of (choose folder with prompt \"{prompt}\")");
    let out = Command::new("/usr/bin/osascript").args(["-e", &script]).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}
