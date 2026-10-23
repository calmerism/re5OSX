//! The player names over other players' heads (GTA Online's gamer tags).
//!
//! `CMultiplayerGamerTagHud` draws each tag with the `mp_gamer_info` movie
//! and lays its parts (the name, the crew tag, the icons...) out itself, in
//! a row centred over the player: once, it sorts the parts the movie has by
//! depth into a table of 30, and each layout walks all 30 entries, adding
//! up the visible parts' widths, then placing them from minus half the
//! total. The engine knows 30 parts; the game data's movie has 15 (the log
//! says "eMP_TAG 15 is not a display object" up to 29), so the sort fills
//! 15 entries and the rest stay 0, which is the name: the name counted 16
//! times, placed 16 times, the last one some 330 movie pixels to the right
//! of the player. Here the table's tail gets the parts the movie lacks,
//! which every layout skips as on a movie that has them.

use re5osx_game::{Env, orig};

use crate::memory::Mem;

/// The parts' layout order (`int[30]`), the flag the sort sets once it has
/// filled it, and the `CMultiplayerGamerTagHud*`, at these addresses in
/// this build.
const ORDER: u64 = 19506224;
const SORTED: u64 = 19506092;
const HUD: u64 = 19561688;
/// The parts a tag can have (`eMP_TAG`).
const PARTS: usize = 30;
/// The tags (one a player, then the fake ones), their size, and where a
/// tag keeps "lay my parts out again".
const TAGS: u64 = 52;
const TAG_SIZE: u64 = 672;
const TAG_RELAYOUT: u64 = 264;

/// Hook for `CMultiplayerGamerTagHud::Render()`.
pub(crate) unsafe extern "C" fn render(env: *mut Env) {
    unsafe { orig::gamer_tag_render(env) };
    let mem = unsafe { Mem::of(env) };
    if mem.u8(SORTED) == 0 {
        return;
    }
    let mut order = [0i32; PARTS];
    for (i, part) in order.iter_mut().enumerate() {
        *part = mem.i32(ORDER + 4 * i as u64);
    }
    let Some(changed) = complete(&mut order) else { return };
    for i in changed {
        mem.set_i32(ORDER + 4 * i as u64, order[i]);
    }
    // The sort and the first tag's layout come in the same frame: the
    // tags laid out with the table as it was are laid out again.
    let hud = mem.u64(HUD);
    let size = crate::rt().memory.size();
    if hud == 0 || hud + TAGS * TAG_SIZE > size {
        return;
    }
    for tag in 0..TAGS {
        mem.set_u8(hud + tag * TAG_SIZE + TAG_RELAYOUT, 1);
    }
}

/// Gives the entries that repeat an earlier one the parts the table lacks,
/// in order; the indices it changed (`None`: nothing to change, or not a
/// table of parts, as in another build).
fn complete(order: &mut [i32; PARTS]) -> Option<Vec<usize>> {
    let mut seen = [false; PARTS];
    let mut repeats = Vec::new();
    for (i, &part) in order.iter().enumerate() {
        let part = usize::try_from(part).ok().filter(|&p| p < PARTS)?;
        if seen[part] {
            repeats.push(i);
        }
        seen[part] = true;
    }
    let mut missing = (0..PARTS).filter(|&p| !seen[p]);
    for &i in &repeats {
        order[i] = missing.next()? as i32;
    }
    (!repeats.is_empty()).then_some(repeats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_the_order() {
        // As the game data's movie leaves it: 15 parts sorted, then zeros.
        let mut order = [0; PARTS];
        order[..15].copy_from_slice(&[1, 3, 13, 0, 11, 6, 5, 10, 8, 9, 14, 2, 4, 7, 12]);
        assert_eq!(complete(&mut order), Some((15..30).collect()));
        assert_eq!(order[..15], [1, 3, 13, 0, 11, 6, 5, 10, 8, 9, 14, 2, 4, 7, 12]);
        assert_eq!(order[15..], (15..30).collect::<Vec<_>>()[..]);
        // Complete: nothing to change.
        assert_eq!(complete(&mut order), None);
        // Not a table of parts.
        let mut other = [7; PARTS];
        other[0] = 1000;
        assert_eq!(complete(&mut other), None);
    }
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.gamer_tag_render = Some(render);
}
