//! Where things are: the user folder (`~/Library/Application Support/re5OSX`)
//! and what ships in the app (`Contents/Resources`: the launcher's page and
//! the kit that makes the game app).

use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

pub fn support() -> PathBuf {
    home().join("Library/Application Support/re5OSX")
}

pub fn assets() -> PathBuf {
    support().join("assets")
}

pub fn wallpapers() -> PathBuf {
    support().join("wallpapers")
}

pub fn config() -> PathBuf {
    support().join("launcher.json")
}

/// The app's resources: `Contents/Resources` in the bundle; from the source
/// tree (`cargo run`), RE5OSX_RESOURCES or the repository's folders.
fn resources() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("RE5OSX_RESOURCES") {
        return Some(dir.into());
    }
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.join("Resources");
    bundle.join("web").is_dir().then_some(bundle)
}

/// The launcher's page.
pub fn web() -> PathBuf {
    resources()
        .map(|r| r.join("web"))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"))
}

/// The kit for re5OSX (`mise run macos:download`): the app without the game,
/// the game's compiler and link-app.
pub fn kit() -> PathBuf {
    resources()
        .map(|r| r.join("kit"))
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/download/re5OSX.app/Contents/Resources/kit"))
}

/// The game as Install makes it on this Mac (it holds the game's code, so
/// it never ships): kept in the user folder and opened by re5OSX itself, so
/// re5OSX is one app.
pub fn game_app() -> PathBuf {
    support().join("game/re5OSX.app")
}

/// What made the game app: this re5OSX's version and its kit's library. A
/// different re5OSX makes it again.
pub fn kit_stamp() -> String {
    let size = std::fs::metadata(kit().join("libre5osx_app.a")).map(|m| m.len()).unwrap_or(0);
    format!("{} {size}", env!("CARGO_PKG_VERSION"))
}

/// The game app is there, made by this re5OSX.
pub fn game_app_current() -> bool {
    game_app().join("Contents/MacOS/re5osx").exists()
        && std::fs::read_to_string(support().join("game/made-by")).is_ok_and(|s| s == kit_stamp())
}
