//! Setup: checks a game folder (the GTA V web build: b/<build>/, data/) and
//! makes re5OSX's tile art and menu sounds from it, into the user
//! folder's `assets`. Nothing of the game ships with re5OSX; all of this is
//! made from the player's own copy.
//!
//! The tiles use the official GTA V wallpapers (Artwork-<Name>-GTAV.<jpg|png>),
//! found on their own: in re5OSX's own wallpapers folder, in a wallpapers
//! folder inside or next to the game folder, or in Downloads or Pictures (and
//! kept in re5OSX's folder from then on); the game's own loading-screen
//! paintings stand in for any missing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{RgbImage, imageops};
use regex::Regex;

use crate::gamefiles;
use crate::paths;

/// The game build's id: the folder under b/ that holds game.wasm.
pub fn find_build(game: &Path) -> Option<String> {
    let builds: Vec<String> = fs::read_dir(game.join("b"))
        .ok()?
        .flatten()
        .filter(|e| e.path().join("game.wasm").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    (builds.len() == 1).then(|| builds[0].clone())
}

/// (ok, build, what is missing) for a folder that should hold the game.
pub fn check_game(game: &Path) -> (bool, Option<String>, Vec<String>) {
    if !game.is_dir() {
        return (false, None, vec!["The folder does not exist.".into()]);
    }
    let build = find_build(game);
    let mut need: Vec<String> =
        ["data/x64/audio/sfx/RESIDENT.rpf", "data/common"]
            .map(String::from)
            .into();
    let mut missing = Vec::new();
    match &build {
        Some(b) => need.extend(
            ["shaders", "title/art"].map(|f| format!("b/{b}/{f}")),
        ),
        None => missing.push("b/<build>/game.wasm".into()),
    }
    missing.extend(need.into_iter().filter(|n| !game.join(n).exists()));
    (missing.is_empty(), build, missing)
}

// ---- menu sounds ---------------------------------------------------------------------------------------------------

// The pause menu's (HUD_FRONTEND_DEFAULT_SOUNDSET in sounds.dat54): its layers of RESIDENT.rpf's frontend.awc, with
// their pitch (cents), volume (dB) and speakers (1 left, 2 right, 3 both) as the sound tables give them.
const UI_01_L: &str = "CHARACTER_SWITCH_UI_01.L";
const UI_01_R: &str = "CHARACTER_SWITCH_UI_01.R";
const UI_02_L: &str = "CHARACTER_SWITCH_UI_02.L";

struct Sound {
    name: &'static str,
    pitch: f64,
    volume: f64,
    fade_ms: usize,
    layers: &'static [(&'static str, f64, f64, u8)],
}

const MENU_SOUNDS: &[Sound] = &[
    // NAV_UP_DOWN_MASTER: +4 dB, +100 cents, over UI_02.L released over 100 ms
    Sound { name: "nav", pitch: 100.0, volume: 4.0, fade_ms: 100, layers: &[(UI_02_L, 0.0, 0.0, 3)] },
    // MENU_SELECT_MASTER: UI_01 left and right, UI_02.L +5 dB
    Sound {
        name: "select",
        pitch: 0.0,
        volume: 0.0,
        fade_ms: 0,
        layers: &[(UI_01_L, 0.0, 0.0, 1), (UI_01_R, 0.0, 0.0, 2), (UI_02_L, 0.0, 5.0, 3)],
    },
    // MENU_BACK_MASTER: +300 cents, over UI_01 left and right and UI_02.L at -600 cents
    Sound {
        name: "back",
        pitch: 300.0,
        volume: 0.0,
        fade_ms: 0,
        layers: &[(UI_01_L, 0.0, 0.0, 1), (UI_01_R, 0.0, 0.0, 2), (UI_02_L, -600.0, 0.0, 3)],
    },
];

fn repitch(x: &[f32], cents: f64) -> Vec<f32> {
    let rate = 2f64.powf(cents / 1200.0);
    (0..(x.len() as f64 / rate) as usize)
        .map(|i| {
            let t = i as f64 * rate;
            let (j, f) = (t as usize, (t - t.floor()) as f32);
            x[j] * (1.0 - f) + x.get(j + 1).copied().unwrap_or(0.0) * f
        })
        .collect()
}

pub fn make_sounds(game: &Path, build: &str, out: &Path, log: &dyn Fn(String)) -> Result<()> {
    let sfx = game.join("data/x64/audio/sfx");
    log("Reading the sound files".into());
    let wasm = game.join("b").join(build).join("game.wasm");
    let wasm = if wasm.with_extension("wasm.orig").exists() { wasm.with_extension("wasm.orig") } else { wasm };
    let key = gamefiles::find_key(&wasm, &sfx.join("RESIDENT.rpf"))?;
    let awc = gamefiles::Rpf::open(&sfx.join("RESIDENT.rpf"), key)?.read("frontend.awc")?;
    let waves = gamefiles::awc_waves(&awc)?;
    let mut source = HashMap::new();
    for name in [UI_01_L, UI_01_R, UI_02_L] {
        let (samples, packed) = waves.get(&(gamefiles::joaat(name) & 0x1FFF_FFFF)).with_context(|| format!("{name} not found"))?;
        source.insert(name, gamefiles::decode_opus(packed, *samples as usize)?);
    }
    let mut mixes = Vec::new();
    for sound in MENU_SOUNDS {
        let (mut left, mut right): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
        for &(name, cents, db, speakers) in sound.layers {
            let mut x = source[name].clone();
            if sound.fade_ms > 0 {
                let n = 48 * sound.fade_ms;
                let len = x.len();
                for (i, v) in x.iter_mut().enumerate() {
                    *v *= ((len - i) as f32 / n as f32).min(1.0);
                }
            }
            let gain = 10f64.powf((db + sound.volume) / 20.0) as f32;
            let x = repitch(&x.iter().map(|v| v * gain).collect::<Vec<_>>(), cents + sound.pitch);
            for (channel, mask) in [(&mut left, 1), (&mut right, 2)] {
                if speakers & mask != 0 {
                    if channel.len() < x.len() {
                        channel.resize(x.len(), 0.0);
                    }
                    for (c, v) in channel.iter_mut().zip(&x) {
                        *c += v;
                    }
                }
            }
        }
        let n = left.len().max(right.len());
        left.resize(n, 0.0);
        right.resize(n, 0.0);
        mixes.push((sound.name, left, right));
    }
    // One gain for all three: the game's levels between them, the loudest at -1 dBFS.
    let peak = mixes.iter().flat_map(|(_, l, r)| l.iter().chain(r)).fold(0f32, |m, v| m.max(v.abs()));
    let gain = 10f32.powf(-1.0 / 20.0) / peak;
    fs::create_dir_all(out)?;
    for (name, l, r) in mixes {
        let mut frames = Vec::with_capacity(l.len() * 4);
        for (a, b) in l.iter().zip(&r) {
            for v in [a, b] {
                frames.extend(((v * gain * 32767.0) as i32).clamp(-32767, 32767).to_le_bytes()[..2].iter());
            }
        }
        fs::write(out.join(format!("{name}.wav")), wav(&frames))?;
    }
    log("Making the menu pictures".into());
    Ok(())
}

/// 16-bit stereo 48 kHz.
fn wav(frames: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + frames.len());
    out.extend(b"RIFF");
    out.extend((36 + frames.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(48000u32.to_le_bytes());
    out.extend((48000u32 * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((frames.len() as u32).to_le_bytes());
    out.extend(frames);
    out
}

// ---- tile art ------------------------------------------------------------------------------------------------------

/// The pictures re5OSX shows, by name (the official artwork's names).
const ART: &[&str] = &[
    "vinewood",
    "thedesert",
    "thehunt",
    "trevorfranklinmichael",
    "michaelcarbinerifle",
    "franklinandchop",
    "trevor",
    "blitzplay",
    "michaelvangelico",
    "franklinvinewoodhills",
    "thetrunk",
    "cashandcarry-byland",
    "stopandfrisk",
    "franklintrevormichael",
    "starlet",
    "buzzard",
    "preparation",
];
/// The tiles' pictures: wide, as the big tile.
const ASPECT: f64 = 2400.0 / 1110.0;

fn wallpapers_in(folder: &Path) -> HashMap<String, PathBuf> {
    let name = Regex::new(r"(?i)^Artwork-(.+)-GTAV\.(jpe?g|png|webp)$").unwrap();
    let mut found = HashMap::new();
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if let Some(m) = name.captures(&file) {
            found.insert(m[1].to_lowercase(), entry.path());
        }
    }
    found
}

/// An image by what it holds, not its name: wallpapers named `.jpg` are
/// often WebP.
fn open_image(path: &Path) -> image::ImageResult<image::DynamicImage> {
    image::ImageReader::open(path)?.with_guessed_format()?.decode()
}

/// The game's loading-screen paintings (their backgrounds), cut from the tilted frames they sit in, in order.
fn loading_screens(game: &Path, build: &str) -> Vec<RgbImage> {
    let art = game.join("b").join(build).join("title/art");
    (0..32)
        .filter_map(|i| open_image(&art.join(format!("ls{i}_background.webp"))).ok())
        .map(|im| {
            let im = im.to_rgb8();
            let (w, h) = (im.width() as f64, im.height() as f64);
            let (x0, y0) = ((w * 0.08) as u32, (h * 0.12) as u32);
            imageops::crop_imm(&im, x0, y0, (w * 0.92) as u32 - x0, (h * 0.90) as u32 - y0).to_image()
        })
        .collect()
}

/// Copies the official wallpapers found in `folder` to re5OSX's own wallpapers folder: how many.
pub fn use_wallpapers(folder: &Path) -> Result<usize> {
    let found = wallpapers_in(folder);
    let own = paths::wallpapers();
    fs::create_dir_all(&own)?;
    for path in found.values() {
        fs::copy(path, own.join(path.file_name().unwrap()))?;
    }
    Ok(found.len())
}

/// Gathers the official wallpapers from where they usually are into re5OSX's own folder.
fn find_wallpapers(game: &Path) -> HashMap<String, PathBuf> {
    let own = paths::wallpapers();
    let home = paths::home();
    let places = [game.join("wallpapers"), game.parent().unwrap_or(game).join("wallpapers"), home.join("Downloads"), home.join("Pictures")];
    for place in places {
        if place.is_dir() && place != own {
            let _ = use_wallpapers(&place);
        }
    }
    wallpapers_in(&own)
}

pub fn make_art(game: &Path, build: &str, out: &Path, log: &dyn Fn(String)) -> Result<()> {
    let found = find_wallpapers(game);
    let mut pictures: Vec<(&str, RgbImage)> = Vec::new();
    for name in ART {
        if let Some(path) = found.get(*name) {
            match open_image(path) {
                Ok(im) => pictures.push((name, im.to_rgb8())),
                Err(e) => log(format!("Couldn't read {}: {e}", path.file_name().unwrap_or_default().to_string_lossy())),
            }
        }
    }
    if !found.is_empty() {
        log(format!("Using {} of your wallpapers", pictures.len()));
    }
    let missing: Vec<&str> = ART.iter().copied().filter(|n| !pictures.iter().any(|(p, _)| p == n)).collect();
    if !missing.is_empty() {
        let screens = loading_screens(game, build);
        if screens.is_empty() {
            bail!("no loading-screen art in b/{build}/title/art");
        }
        for (i, name) in missing.into_iter().enumerate() {
            pictures.push((name, screens[i % screens.len()].clone()));
        }
    }
    let tiles = out.join("tiles");
    fs::create_dir_all(&tiles)?;
    let count = pictures.len();
    for (name, im) in pictures {
        // Cropped to the tiles' shape from the top (faces sit high; the artwork's logos are at the bottom).
        let h = (im.height() as f64).min((im.width() as f64 / ASPECT).round());
        let w = (h * ASPECT).round() as u32;
        let im = imageops::crop_imm(&im, (im.width().saturating_sub(w)) / 2, 0, w.min(im.width()), h as u32).to_image();
        let width = 1280;
        let sized = imageops::resize(&im, width, (width as f64 / ASPECT).round() as u32, imageops::FilterType::Lanczos3);
        sized.save(tiles.join(format!("{name}.png")))?;
    }
    log(format!("{count} pictures made"));
    Ok(())
}

/// What setup makes is there.
pub fn assets_ready() -> bool {
    let sounds = ["nav.wav", "select.wav", "back.wav"].iter().all(|f| paths::assets().join("sounds").join(f).exists());
    sounds && paths::assets().join("tiles").is_dir()
}

// ---- all of it -----------------------------------------------------------------------------------------------------

pub fn setup(game: &Path, log: &dyn Fn(String)) -> Result<String> {
    let (ok, build, missing) = check_game(game);
    if !ok {
        bail!("not a game folder: missing {}", missing.join(", "));
    }
    let build = build.unwrap();
    make_sounds(game, &build, &paths::assets().join("sounds"), log)?;
    make_art(game, &build, &paths::assets(), log)?;
    Ok(build)
}
