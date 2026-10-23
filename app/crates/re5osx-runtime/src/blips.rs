//! Map blips whose sprite the game data lacks. The scripts are newer than
//! the data and blip later updates' places (businesses, the casino, the
//! car meet...) with sprites the 2014 minimap movie does not have; the
//! minimap then draws a wrong or empty icon. Such a blip is hidden: shown
//! nowhere, whatever display the scripts give it, until its sprite is one
//! the data has, when the display the scripts want comes back.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// `CMiniMap`'s sprites, from the minimap movie: `{ i32 id, ConstString
/// name }`, 16 bytes each, sorted by id, which run from 0 without a gap.
const SPRITES: u64 = 19520936;
const SPRITE_COUNT: u64 = 19520944;
/// `eBLIP_PARAMS`.
const BLIP_DISPLAY: u32 = 1;
const BLIP_SPRITE: u32 = 3;
const DISPLAY_NONE: u32 = 0;
/// Hidden blips kept before those the game removed meanwhile are dropped.
const PRUNE_AT: usize = 256;

/// The blips hidden, each with the display the scripts want for it, and
/// the sprites reported.
#[derive(Default)]
struct Hidden {
    blips: HashMap<u32, u32>,
    reported: HashSet<u32>,
}

impl Hidden {
    /// `blip` gets a sprite the data lacks while showing `display`: hidden,
    /// that display kept (already hidden: the one kept stays).
    fn hide(&mut self, blip: u32, display: impl FnOnce() -> u32) {
        self.blips.entry(blip).or_insert_with(display);
    }

    /// The scripts give `blip` a display: kept if it is hidden (true).
    fn wanted(&mut self, blip: u32, display: u32) -> bool {
        self.blips.get_mut(&blip).map(|d| *d = display).is_some()
    }

    /// `blip` gets a sprite the data has: the display to give it back, if
    /// it was hidden showing something.
    fn show(&mut self, blip: u32) -> Option<u32> {
        self.blips.remove(&blip).filter(|&d| d != DISPLAY_NONE)
    }
}

static HIDDEN: Mutex<Option<Hidden>> = Mutex::new(None);

/// Hook for `CMiniMap::SetBlipParameter(eBLIP_PARAMS, int, int)`.
pub(crate) unsafe extern "C" fn blip_parameter(env: *mut Env, param: u32, blip: u32, value: u32) {
    match param {
        BLIP_SPRITE => {
            let m = unsafe { Mem::of(env) };
            let count = u32::from(m.u16(SPRITE_COUNT));
            let known = count == 0 || m.u64(SPRITES) == 0 || value < count;
            let mut hidden = HIDDEN.lock().unwrap();
            let h = hidden.get_or_insert_default();
            if known {
                let display = h.show(blip);
                drop(hidden);
                unsafe { orig::blip_parameter(env, param, blip, value) };
                if let Some(display) = display {
                    unsafe { orig::blip_parameter(env, BLIP_DISPLAY, blip, display) };
                }
                return;
            }
            if h.reported.insert(value) {
                log::info!("blips: sprite {value} is not in the game data, its blips hidden");
            }
            // Blips the game removed meanwhile (ids are not used again).
            if h.blips.len() >= PRUNE_AT {
                h.blips.retain(|&id, _| unsafe { orig::blip_from_id(env, id) } != 0);
            }
            h.hide(blip, || match unsafe { orig::blip_from_id(env, blip) } {
                0 => DISPLAY_NONE,
                b => unsafe { orig::blip_display(env, b) },
            });
            drop(hidden);
            unsafe { orig::blip_parameter(env, BLIP_DISPLAY, blip, DISPLAY_NONE) };
            return;
        }
        // Hidden: kept for when it shows again.
        BLIP_DISPLAY if HIDDEN.lock().unwrap().as_mut().is_some_and(|h| h.wanted(blip, value)) => return,
        _ => {}
    }
    unsafe { orig::blip_parameter(env, param, blip, value) }
}

pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.blip_parameter = Some(blip_parameter);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blip hidden for its sprite shows again with the display the
    /// scripts want once its sprite is known: the one it had, or one given
    /// while it was hidden.
    #[test]
    fn a_blip_shown_again_gets_its_display_back() {
        let mut h = Hidden::default();
        h.hide(7, || 2);
        assert_eq!(h.show(7), Some(2));
        h.hide(8, || 2);
        h.hide(8, || DISPLAY_NONE); // another unknown sprite: still 2
        assert!(h.wanted(8, 5));
        assert_eq!(h.show(8), Some(5));
        assert!(!h.wanted(8, 3), "no longer hidden");
        h.hide(9, || DISPLAY_NONE);
        assert_eq!(h.show(9), None, "it showed nothing");
        assert!(h.blips.is_empty());
    }
}
