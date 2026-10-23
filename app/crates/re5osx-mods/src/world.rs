//! The game world for mods, read from the engine's own tables:
//!
//! - the entities in the ped, vehicle and object pools, as script handles
//!   (`world.vehicles()`...);
//! - how much room the pools have left (natives that create entities are
//!   refused before the engine would fail);
//! - the catalog of the installed game's names: vehicle and ped models,
//!   weapons. The engine keeps the names it hashed
//!   (`atHashStringNamespaceSupport`), so they come from the assets the
//!   game loaded, whatever their version: types for the checker
//!   (`world.d.luau`), lists for mods.

use std::sync::Mutex;

use re5osx_game::{Env, orig};
use re5osx_runtime::memory::Mem;

/// fwBasePool* of the peds and of the objects.
const PED_POOL: u64 = 20159656;
const OBJECT_POOL: u64 = 19670000;
/// The vehicles' pool allocator: u64 at u64 here; base +0, size +8, free
/// +16, element size +24, used bits +56 (a pointer per slot).
const VEHICLE_POOL: u64 = 26005240;
/// The script handles' pool (fwBasePool*).
const GUID_POOL: u64 = 12318120;
/// The archetype store (atPoolBase): storage +0, size +8, element size
/// +24, used bits +56; archetype: name hash +24, model type +157 (& 31:
/// 5 vehicle, 6 ped).
const ARCHETYPES: u64 = 10872112;
/// CWeaponInfoManager's sorted item infos: u64 array, u16 count at +8;
/// an info's name hash at +16.
const ITEM_INFOS: u64 = 28496576;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pool {
    Peds,
    Vehicles,
    Objects,
}

/// The entities alive in an fwBasePool (slot flags: bit 7 = free).
fn base_pool(mem: Mem, at: u64) -> Vec<u64> {
    let pool = mem.u64(at);
    if pool == 0 {
        return Vec::new();
    }
    let (storage, flags) = (mem.u64(pool + 16), mem.u64(pool + 24));
    let (size, item) = (mem.i32(pool + 32).max(0) as u64, mem.i32(pool + 36).max(0) as u64);
    (0..size).filter(|&i| (mem.u8(flags + i) as i8) >= 0).map(|i| storage + item * i).collect()
}

/// How many more an fwBasePool takes.
fn base_free(mem: Mem, at: u64) -> u64 {
    let pool = mem.u64(at);
    if pool == 0 {
        return 0;
    }
    let size = mem.i32(pool + 32).max(0) as u64;
    let used = (((mem.u32(pool + 48) << 2) as i32) >> 2).max(0) as u64;
    size.saturating_sub(used)
}

fn vehicle_allocator(mem: Mem) -> u64 {
    match mem.u64(VEHICLE_POOL) {
        0 => 0,
        p => mem.u64(p),
    }
}

/// The vehicles (the allocator's used slots hold pointers to them).
fn vehicles(mem: Mem) -> Vec<u64> {
    let p = vehicle_allocator(mem);
    if p == 0 {
        return Vec::new();
    }
    let (base, size, elem, bits) = (mem.u64(p), mem.u64(p + 8), mem.u64(p + 24), mem.u64(p + 56));
    (0..size)
        .filter(|&i| bits != 0 && (mem.u32(bits + 4 * (i >> 5)) >> (i & 31)) & 1 == 1)
        .map(|i| mem.u64(base + elem * i))
        .filter(|&v| v != 0)
        .collect()
}

/// How many more entities of a kind the game takes.
pub fn free(mem: Mem, pool: Pool) -> u64 {
    match pool {
        Pool::Peds => base_free(mem, PED_POOL),
        Pool::Objects => base_free(mem, OBJECT_POOL),
        Pool::Vehicles => match vehicle_allocator(mem) {
            0 => 0,
            p => mem.u64(p + 16),
        },
    }
}

/// Script handles for the entities of a pool, at most `max`. Handles are
/// made for entities that have none, while the handles' pool keeps room
/// (the engine fails when it fills).
///
/// # Safety
/// `env` is the engine thread's, inside a script's update.
pub unsafe fn handles(env: *mut Env, pool: Pool, max: usize) -> Vec<i32> {
    let mem = unsafe { Mem::of(env) };
    let entities = match pool {
        Pool::Peds => base_pool(mem, PED_POOL),
        Pool::Objects => base_pool(mem, OBJECT_POOL),
        Pool::Vehicles => vehicles(mem),
    };
    let mut out = Vec::new();
    for e in entities {
        if out.len() >= max || base_free(mem, GUID_POOL) < 64 {
            break;
        }
        match unsafe { orig::script_guid(env, e) } {
            0 => {}
            h => out.push(h as i32),
        }
    }
    out
}

/// The installed game's names, lowercase models and uppercase weapons.
#[derive(Clone, Default, PartialEq)]
pub struct Catalog {
    pub vehicles: Vec<String>,
    pub peds: Vec<String>,
    pub weapons: Vec<String>,
}

static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);

/// The catalog, once read (this session, or the last one's, saved).
pub fn catalog() -> Option<Catalog> {
    crate::lock(&CATALOG).clone()
}

/// Reads the catalog from the engine's tables (after loading).
///
/// # Safety
/// `env` is the engine thread's.
pub unsafe fn read_catalog(env: *mut Env) -> Catalog {
    let mem = unsafe { Mem::of(env) };
    let name = |hash: u32| match unsafe { orig::hash_name(env, 0, hash) } {
        0 => None,
        p => Some(mem.str(p)),
    };
    let mut c = Catalog::default();
    let (storage, size, elem, bits) =
        (mem.u64(ARCHETYPES), mem.u64(ARCHETYPES + 8), mem.u64(ARCHETYPES + 24), mem.u64(ARCHETYPES + 56));
    if storage != 0 && bits != 0 {
        for i in 0..size.min(1 << 20) {
            if (mem.u32(bits + 4 * (i >> 5)) >> (i & 31)) & 1 == 0 {
                continue;
            }
            let arch = mem.u64(storage + elem * i);
            if arch == 0 {
                continue;
            }
            let list = match mem.u8(arch + 157) & 31 {
                5 => &mut c.vehicles,
                6 => &mut c.peds,
                _ => continue,
            };
            if let Some(n) = name(mem.u32(arch + 24)).filter(|n| valid(n)) {
                list.push(n.to_ascii_lowercase());
            }
        }
    }
    let (infos, count) = (mem.u64(ITEM_INFOS), mem.u16(ITEM_INFOS + 8));
    if infos != 0 {
        for i in 0..u64::from(count) {
            let info = mem.u64(infos + 8 * i);
            if info == 0 {
                continue;
            }
            if let Some(n) = name(mem.u32(info + 16)).filter(|n| n.starts_with("WEAPON_") && valid(n)) {
                c.weapons.push(n.to_ascii_uppercase());
            }
        }
    }
    for list in [&mut c.vehicles, &mut c.peds, &mut c.weapons] {
        list.sort();
        list.dedup();
    }
    c
}

/// A name a Luau string type can hold as it is.
fn valid(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Takes a catalog read now: kept, saved for the next sessions (the
/// checker has the types before the game loads), and the checker made
/// again when it changed.
pub fn set_catalog(c: Catalog) {
    let mut slot = crate::lock(&CATALOG);
    if slot.as_ref() == Some(&c) {
        return;
    }
    log::info!("mods: catalog of {} vehicles, {} peds, {} weapons", c.vehicles.len(), c.peds.len(), c.weapons.len());
    let text = definitions_of(Some(&c));
    *slot = Some(c);
    drop(slot);
    *crate::lock(&ALIASES) = None;
    // The rest off the caller's thread (the engine's): a file to write, and
    // the Studio's type checker to wait for (busy for a while at times).
    let dir = crate::dir().map(std::path::Path::to_path_buf);
    let _ = std::thread::Builder::new().name("mods catalog".into()).spawn(move || {
        if let Some(dir) = dir {
            let _ = std::fs::create_dir_all(dir.join(".re5osx"));
            let _ = std::fs::write(dir.join(".re5osx/world.d.luau"), text);
        }
        crate::checker::Checker::release();
    });
}

/// The catalog saved by an earlier session.
pub fn load_saved(dir: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(dir.join(".re5osx/world.d.luau")) else { return };
    let list = |name: &str| -> Vec<String> {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("type {name} = ")))
            .map(|rest| rest.split(" | ").map(|s| s.trim_matches('"').to_string()).filter(|s| valid(s)).collect())
            .unwrap_or_default()
    };
    let c = Catalog { vehicles: list("VehicleModel"), peds: list("PedModel"), weapons: list("WeaponName") };
    if !c.vehicles.is_empty() {
        *crate::lock(&CATALOG) = Some(c);
        *crate::lock(&ALIASES) = None;
    }
}

/// The catalog's types as sets of names, for [`shorten`] (the largest
/// first: a model is a vehicle's or a ped's).
static ALIASES: Mutex<Option<Vec<(&'static str, std::collections::HashSet<String>)>>> = Mutex::new(None);

fn aliases() -> Vec<(&'static str, std::collections::HashSet<String>)> {
    let mut slot = crate::lock(&ALIASES);
    if slot.is_none() {
        let c = catalog().unwrap_or_default();
        let set = |lists: &[&Vec<String>]| lists.iter().flat_map(|l| l.iter().cloned()).collect();
        *slot = Some(vec![
            ("Model", set(&[&c.vehicles, &c.peds])),
            ("VehicleModel", set(&[&c.vehicles])),
            ("PedModel", set(&[&c.peds])),
            ("WeaponName", set(&[&c.weapons])),
        ]);
    }
    slot.clone().unwrap()
}

/// `text` (a type or a message of the checker's) with the catalog's
/// unions of names written as their types (`VehicleModel`...) and other
/// long unions of strings cut short: the checker spells them out, a few
/// thousand characters for a vehicle's model.
pub fn shorten(text: &str) -> String {
    if !text.contains("\" | \"") {
        return text.to_owned();
    }
    let aliases = aliases();
    let mut out = String::with_capacity(text.len().min(4096));
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        out.push_str(&rest[..start]);
        // A run of string literals joined by " | ".
        let mut names = Vec::new();
        let mut at = start;
        loop {
            let Some(len) = rest[at + 1..].find('"') else { break };
            names.push(&rest[at + 1..at + 1 + len]);
            let after = at + len + 2;
            if rest[after..].starts_with(" | \"") {
                at = after + 3;
            } else {
                at = after;
                break;
            }
        }
        if names.is_empty() {
            out.push_str(&rest[start..]);
            return out;
        }
        if names.len() < 6 {
            out.push_str(&rest[start..at]);
        } else {
            let mut left: std::collections::HashSet<&str> = names.iter().copied().collect();
            let mut parts: Vec<String> = Vec::new();
            for (alias, set) in &aliases {
                if !set.is_empty() && set.iter().all(|n| left.contains(n.as_str())) {
                    left.retain(|n| !set.contains(*n));
                    parts.push((*alias).to_owned());
                }
            }
            let mut rest_names: Vec<&str> = names.iter().copied().filter(|n| left.contains(n)).collect();
            rest_names.dedup();
            let more = rest_names.len().saturating_sub(3);
            if more > 1 {
                rest_names.truncate(3);
            }
            parts.extend(rest_names.iter().map(|n| format!("\"{n}\"")));
            if more > 1 {
                parts.push(format!("… {more} more"));
            }
            out.push_str(&parts.join(" | "));
        }
        rest = &rest[at..];
    }
    out.push_str(rest);
    out
}

/// The world's types for the checker: the catalog's names as string
/// unions, or plain strings before there is a catalog.
pub fn definitions() -> String {
    definitions_of(crate::lock(&CATALOG).as_ref())
}

fn definitions_of(c: Option<&Catalog>) -> String {
    let union = |names: &[String]| {
        if names.is_empty() {
            "string".to_string()
        } else {
            names.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" | ")
        }
    };
    let empty = Vec::new();
    let (v, p, w) = match c {
        Some(c) => (&c.vehicles, &c.peds, &c.weapons),
        None => (&empty, &empty, &empty),
    };
    format!(
        "-- The installed game's names, read by re5OSX from what the game loaded.\n\
         type VehicleModel = {}\ntype PedModel = {}\ntype WeaponName = {}\ntype Model = VehicleModel | PedModel\n\n",
        union(v),
        union(p),
        union(w)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_unions_get_their_names() {
        let names = |p: &str, n: usize| (0..n).map(|i| format!("{p}{i}")).collect::<Vec<_>>();
        let c = Catalog { vehicles: names("car", 8), peds: names("ped", 7), weapons: names("WEAPON_", 6) };
        *crate::lock(&CATALOG) = Some(c.clone());
        *crate::lock(&ALIASES) = None;
        let union = |l: &[String]| l.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" | ");
        let both: Vec<String> = c.vehicles.iter().chain(&c.peds).cloned().collect();
        assert_eq!(
            shorten(&format!("(modelHash: {}, x: number) -> Vehicle?", union(&c.vehicles))),
            "(modelHash: VehicleModel, x: number) -> Vehicle?"
        );
        assert_eq!(shorten(&format!("(m: ({})?)", union(&both))), "(m: (Model)?)");
        assert_eq!(shorten(&union(&names("x", 9))), "\"x0\" | \"x1\" | \"x2\" | … 6 more");
        assert_eq!(shorten("(a: \"one\" | \"two\")"), "(a: \"one\" | \"two\")");
    }
}
