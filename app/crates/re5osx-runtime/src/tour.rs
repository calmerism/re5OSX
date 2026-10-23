//! A tour of the map, for automated runs (`mise run shaders:collect`): the
//! player goes from stop to stop, each at its hour and weather, and stays
//! there a while, so the world around draws what it has. The host turns the
//! camera meanwhile.
//!
//! The engine's own teleport does the moves (`CWarpManager`, as the scripts'
//! START_PLAYER_TELEPORT: the scene around the stop loads before the player
//! appears there), from its update, on the game's main thread once a frame.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use re5osx_game::{Env, exports, orig};

use crate::memory::Mem;

/// A place to visit: where, at what hour, in what weather (a name of the
/// game's weather types: EXTRASUNNY, CLEAR, CLOUDS, SMOG, FOGGY, OVERCAST,
/// RAIN, THUNDER, CLEARING, NEUTRAL, SNOW, BLIZZARD, SNOWLIGHT, XMAS).
#[derive(Clone, Debug)]
pub struct Stop {
    pub x: f32,
    pub y: f32,
    /// `None`: on the ground there, or skipped at sea.
    pub z: Option<f32>,
    pub hour: u32,
    pub weather: String,
    pub name: String,
}

struct Tour {
    stops: Vec<Stop>,
    stay: Duration,
    next: usize,
    /// When to move on (`None`: not before the player is there).
    until: Option<Instant>,
}

static TOUR: Mutex<Option<Tour>> = Mutex::new(None);

/// Starts the tour once the game is running: `stay` at each stop.
pub fn start(stops: Vec<Stop>, stay: Duration) {
    *TOUR.lock().unwrap() = Some(Tour { stops, stay, next: 0, until: None });
}

/// Whether the last stop's stay is over.
pub fn finished() -> bool {
    let tour = TOUR.lock().unwrap();
    tour.as_ref().is_some_and(|t| t.next == t.stops.len() && t.until.is_some_and(|u| Instant::now() >= u))
}

/// Parses a tour: one stop a line, `x y z hour weather`, then an optional
/// name; `#` starts a comment. `ground` as z: on the ground there.
pub fn parse(text: &str) -> anyhow::Result<Vec<Stop>> {
    let mut stops = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let (line, comment) = line.split_once('#').unwrap_or((line, ""));
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        let stop = (|| {
            let [x, y, z, hour, weather, ..] = words[..] else { anyhow::bail!("expected x y z hour weather") };
            Ok(Stop {
                x: x.parse()?,
                y: y.parse()?,
                z: if z == "ground" { None } else { Some(z.parse()?) },
                hour: hour.parse()?,
                weather: weather.to_uppercase(),
                name: comment.trim().to_string(),
            })
        })();
        stops.push(stop.map_err(|e: anyhow::Error| e.context(format!("tour line {}", n + 1)))?);
    }
    Ok(stops)
}

/// Hook for `CWarpManager::Update()`.
pub(crate) unsafe extern "C" fn warp_update(env: *mut Env) {
    unsafe { orig::warp_update(env) };
    let mut tour = TOUR.lock().unwrap();
    let Some(tour) = tour.as_mut() else { return };
    let Some(until) = tour.until else {
        // The game's start scripts place the player in the first moments
        // (the sandbox's, moving them where it starts): the tour waits
        // for one stay before its first move.
        if unsafe { orig::find_player_ped(env) } != 0 {
            tour.until = Some(Instant::now() + tour.stay);
        }
        return;
    };
    if Instant::now() < until {
        return;
    }
    let Some(stop) = tour.stops.get(tour.next) else { return };
    let (n, of) = (tour.next + 1, tour.stops.len());
    let Some(pos) = (unsafe { place(env, stop) }) else {
        log::info!("tour: stop {n}/{of} at sea, skipped");
        tour.next += 1;
        return;
    };
    if !unsafe { visit(env, pos, stop) } {
        return; // the warp manager is busy: next frame
    }
    log::info!("tour: stop {n}/{of}, {} ({} h, {})", stop.name, stop.hour, stop.weather);
    tour.next += 1;
    tour.until = Some(Instant::now() + tour.stay);
}

/// Where to take the player for a stop: one without a height starts above
/// the highest point there (the warp puts the player on the ground below);
/// `None` at sea (the highest point under the surface).
unsafe fn place(env: *mut Env, stop: &Stop) -> Option<[f32; 3]> {
    let z = match stop.z {
        Some(z) => z,
        None => {
            let top = unsafe { orig::height_max(env, stop.x, stop.y) };
            if top <= 0.0 {
                return None;
            }
            top + 2.0
        }
    };
    Some([stop.x, stop.y, z])
}

/// Asks the warp manager to take the player to `pos` for the stop, then
/// sets its hour and weather. False if it could not start.
unsafe fn visit(env: *mut Env, pos: [f32; 3], stop: &Stop) -> bool {
    let Ok(top) = (unsafe { exports::emscripten_stack_get_current(env) }) else { return false };
    // Position and velocity, two rage::Vector3 (16 bytes each).
    let Ok(at) = (unsafe { exports::_emscripten_stack_alloc(env, 32) }) else { return false };
    let mem = unsafe { Mem::of(env) };
    for (i, v) in [pos[0], pos[1], pos[2], 0.0, 0.0, 0.0, 0.0, 0.0].iter().enumerate() {
        mem.set_u32(at + 4 * i as u64, v.to_bits());
    }
    // Heading, without a vehicle, snapped to the ground, the scene's radius
    // from the distance, no fade.
    let started = unsafe { orig::warp(env, at, at + 16, 0.0, 0, 1, 0.0, 0) } != 0;
    let _ = unsafe { exports::_emscripten_stack_restore(env, top) };
    if started {
        unsafe { orig::clock_set_time(env, stop.hour, 0, 0) };
        let index = unsafe { orig::weather_index(env, hash(&stop.weather)) } as i32;
        if index >= 0 {
            unsafe { orig::weather_force_now(env, index as u32, 0) };
        } else {
            log::warn!("tour: no weather type {}", stop.weather);
        }
    }
    started
}

/// `rage::atStringHash`: Jenkins' one-at-a-time hash of the lower-case name.
fn hash(name: &str) -> u32 {
    let mut h: u32 = 0;
    for c in name.bytes() {
        let c = match c {
            b'\\' => b'/',
            c => c.to_ascii_lowercase(),
        };
        h = h.wrapping_add(c as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_as_the_engine() {
        // The weather types' hashes as the game's scripts use them.
        assert_eq!(hash("EXTRASUNNY"), 0x97aa0a79);
        assert_eq!(hash("RAIN"), 0x54a69840);
    }

    #[test]
    fn parses_stops() {
        let text = "# comment\n\n-813.6 179.5 72.2 22 rain # Michael's house\n1 2 3 4 CLEAR\n5 6 ground 7 RAIN\n";
        let stops = parse(text).unwrap();
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[2].z, None);
        let s = &stops[0];
        assert_eq!(([s.x, s.y], s.z, s.hour, s.weather.as_str()), ([-813.6, 179.5], Some(72.2), 22, "RAIN"));
        assert_eq!(stops[0].name, "Michael's house");
        assert!(parse("1 2 3").is_err());
    }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.warp_update = Some(warp_update);
}
