//! The game's natives, as the build script found them, and calling one from
//! Lua: the arguments go in a `rage::scrThread::Info` on the wasm stack, the
//! handler runs through `re5osx_game::call_native` (a trap inside comes back
//! as an error), the results come back as Lua values.

use std::cell::Cell;

use re5osx_game::{Env, exports};
use re5osx_runtime::memory::Mem;
use mlua::{Lua, MultiValue, Value};

/// A value's type, as the mods see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Int,
    Float,
    Bool,
    /// A number, or a string the native gets the hash of.
    Hash,
    Str,
    Vec3,
    /// A number or a boolean, as the database knows no better.
    Any,
    Handle(Handle),
}

/// The kinds of handles: script handles of entities, and other indices.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::enum_variant_names)] // ScrHandle: the database's name
pub enum Handle {
    Entity,
    Ped,
    Vehicle,
    Object,
    Player,
    Cam,
    Blip,
    Pickup,
    Interior,
    ScrHandle,
    FireId,
}

impl Handle {
    fn is_entity(self) -> bool {
        matches!(self, Handle::Entity | Handle::Ped | Handle::Vehicle | Handle::Object)
    }
}

/// Whether a parameter goes in, comes out (a pointer the native fills), or both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    In,
    Out,
    InOut,
}

#[derive(Debug)]
pub struct Param {
    pub kind: Kind,
    pub dir: Dir,
}

#[derive(Debug)]
pub struct Native {
    /// Index in [`NAMESPACES`].
    pub ns: u16,
    /// The name in Lua (`createVehicle`).
    pub lua: &'static str,
    /// The game's name (`CREATE_VEHICLE`).
    pub name: &'static str,
    /// The hash this game build registers it with.
    pub hash: u64,
    /// The handler's function table index.
    pub handler: u64,
    pub params: &'static [Param],
    pub ret: Option<Kind>,
    /// The natives database's notes.
    pub doc: &'static str,
}

use {Dir as D, Handle as H, Kind as K, Param as P};
include!(concat!(env!("OUT_DIR"), "/natives.rs"));

/// The natives' Luau definitions, for the type checker.
pub const DEFINITIONS: &str = include_str!(concat!(env!("OUT_DIR"), "/natives.d.luau"));

/// What the build found: how many natives there are, which were left out.
pub const SUMMARY: &str = env!("RE5OSX_MODS_NATIVES_SUMMARY");

/// The handler of a native the mods may not call (`WAIT`).
pub fn reserved_handler(name: &str) -> Option<u64> {
    RESERVED.iter().find(|(n, _)| *n == name).map(|&(_, h)| h)
}

/// The native `lua` of namespace `ns` (NATIVES is sorted by both).
pub fn find(ns: usize, lua: &str) -> Option<usize> {
    let ns = ns as u16;
    NATIVES.binary_search_by(|n| (n.ns, n.lua).cmp(&(ns, lua))).ok()
}

thread_local! {
    /// The engine thread's environment while a mod runs: natives can only
    /// be called then.
    static ENV: Cell<*mut Env> = const { Cell::new(std::ptr::null_mut()) };
}

/// Runs `f` with natives callable through `env`.
pub fn with_env<R>(env: *mut Env, f: impl FnOnce() -> R) -> R {
    let old = ENV.replace(env);
    let r = f();
    ENV.set(old);
    r
}

/// Jenkins' one-at-a-time hash of the lowercase string (`GET_HASH_KEY`).
pub fn joaat(s: &str) -> u32 {
    let mut h: u32 = 0;
    for b in s.bytes() {
        h = h.wrapping_add(b.to_ascii_lowercase() as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

/// The script handles' pool (`rage::fwScriptGuid`), u64 at this address.
const GUID_POOL: u64 = 12318120;

/// The type of the live entity `handle` names (3 vehicle, 4 ped, 5
/// object, `CEntity` +40), none if it names none: what
/// `fwScriptGuid::GetBaseFromGuid` checks, without its `unreachable` for an
/// index past the pool.
fn entity_type(mem: Mem, handle: i32) -> Option<u8> {
    if handle == -1 || handle == 0 {
        return None;
    }
    let pool = mem.u64(GUID_POOL);
    if pool == 0 {
        return None;
    }
    let index = (handle >> 8) as u32;
    if index >= mem.u32(pool + 32) || mem.u8(mem.u64(pool + 24) + u64::from(index)) != handle as u8 {
        return None;
    }
    let guid = mem.u64(pool + 16) + u64::from(mem.u32(pool + 36)) * u64::from(index);
    if guid == 0 {
        return None;
    }
    match mem.u64(guid + 8) {
        0 => None,
        entity => Some(mem.u8(entity + 40)),
    }
}

fn type_name(t: u8) -> &'static str {
    match t {
        3 => "Vehicle",
        4 => "Ped",
        5 => "Object",
        _ => "Entity",
    }
}

/// The current environment, for what mods call from Rust (world.rs).
pub fn env() -> Option<*mut Env> {
    let e = ENV.get();
    (!e.is_null()).then_some(e)
}

/// The native running now (index + 1, 0: none), for the failure note.
pub static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A trap happened in a native: the engine may be in a bad state.
pub static TAINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Calls a native that takes nothing and gives an int (`GET_GAME_TIMER`),
/// from Rust, in the current script context; the reserved ones too.
///
/// # Safety
/// `env` is the engine thread's, inside a script's update.
pub unsafe fn call_int(env: *mut Env, name: &str) -> Option<i32> {
    // By name, once: every script's WAIT asks for some (thousands a second).
    static NO_PARAMS: std::sync::LazyLock<std::collections::HashMap<&str, u64>> = std::sync::LazyLock::new(|| {
        let reserved = RESERVED.iter().map(|&(n, h)| (n, h));
        reserved.chain(NATIVES.iter().filter(|n| n.params.is_empty()).map(|n| (n.name, n.handler))).collect()
    });
    let handler = *NO_PARAMS.get(name)?;
    let mem = unsafe { Mem::of(env) };
    let top = unsafe { exports::emscripten_stack_get_current(env) }.ok()?;
    let r = (|| {
        let info = unsafe { exports::_emscripten_stack_alloc(env, 160) }.ok()?;
        mem.bytes_mut(info, 160).fill(0);
        let result = info + 128;
        mem.set_u64(info, result);
        mem.set_u64(info + 16, info + 136);
        unsafe { re5osx_game::call_native(env, handler, info) }.ok()?;
        Some(mem.i32(result))
    })();
    let _ = unsafe { exports::_emscripten_stack_restore(env, top) };
    r
}

/// The full name of native `n`, for messages (`vehicle.createVehicle`).
pub fn qualified(n: &Native) -> String {
    format!("{}.{}", NAMESPACES[n.ns as usize], n.lua)
}

/// The longest string a native takes from Lua.
const MAX_STRING: usize = 16 << 10;

/// Calls native `index` with Lua's arguments.
pub fn call(lua: &Lua, index: usize, args: MultiValue) -> mlua::Result<MultiValue> {
    let n = &NATIVES[index];
    let env = ENV.get();
    if env.is_null() {
        return Err(mlua::Error::runtime(format!("{}: natives can only run in a mod's script", qualified(n))));
    }
    let mem = unsafe { Mem::of(env) };
    let trap = |e| mlua::Error::runtime(format!("{}: {e}", qualified(n)));
    let top = unsafe { exports::emscripten_stack_get_current(env) }.map_err(trap)?;
    let result = (|| {
        let alloc = |size: u64| -> mlua::Result<u64> {
            let at = unsafe { exports::_emscripten_stack_alloc(env, size.next_multiple_of(16)) }.map_err(trap)?;
            mem.bytes_mut(at, size as usize).fill(0);
            Ok(at)
        };
        let slots: u64 = n.params.iter().map(|p| if p.kind == K::Vec3 && p.dir == D::In { 3 } else { 1 }).sum();
        let info = alloc(128)?;
        let params = alloc(slots.max(1) * 8)?;
        let result = alloc(32)?;
        let mut outputs = Vec::new();
        let mut args = args.into_iter();
        let mut slot = params;
        for (i, p) in n.params.iter().enumerate() {
            let bad = |what: &str| {
                mlua::Error::runtime(format!("{}: argument {} must be {what}", qualified(n), i + 1))
            };
            let value = if p.dir == D::Out { Value::Nil } else { args.next().unwrap_or(Value::Nil) };
            let target = if p.dir == D::In {
                slot
            } else {
                // A pointer to a buffer the native fills: scrValues, 8 bytes each.
                let buf = alloc(24)?;
                mem.set_u64(slot, buf);
                outputs.push((buf, p.kind));
                buf
            };
            if p.dir == D::Out {
                slot += 8;
                continue;
            }
            match p.kind {
                K::Int => mem.set_i32(target, int(&value).ok_or_else(|| bad("a number"))?),
                K::Any => match value {
                    Value::Boolean(b) => mem.set_i32(target, b as i32),
                    Value::Nil => {}
                    ref v => mem.set_i32(target, int(v).ok_or_else(|| bad("a number or a boolean"))?),
                },
                K::Float => mem.set_u32(target, (number(&value).ok_or_else(|| bad("a number"))? as f32).to_bits()),
                K::Bool => match value {
                    Value::Boolean(b) => mem.set_i32(target, b as i32),
                    _ => return Err(bad("a boolean")),
                },
                K::Hash => match &value {
                    Value::String(s) => mem.set_u32(target, joaat(&s.to_str()?)),
                    v => mem.set_u32(target, number(v).ok_or_else(|| bad("a hash or a name"))? as i64 as u32),
                },
                K::Str => match &value {
                    Value::String(s) => {
                        let bytes = s.as_bytes();
                        if bytes.contains(&0) {
                            return Err(bad("a string without NUL"));
                        }
                        // It goes on the engine thread's stack, which has
                        // no room for megabytes (no native takes them).
                        if bytes.len() > MAX_STRING {
                            return Err(bad(&format!("a string of at most {} KB", MAX_STRING >> 10)));
                        }
                        let at = alloc(bytes.len() as u64 + 1)?;
                        mem.bytes_mut(at, bytes.len()).copy_from_slice(&bytes);
                        mem.set_u64(target, at);
                    }
                    Value::Nil => {}
                    _ => return Err(bad("a string")),
                },
                K::Vec3 => match value {
                    Value::Vector(v) => {
                        for (k, c) in [v.x(), v.y(), v.z()].into_iter().enumerate() {
                            mem.set_u32(target + 8 * k as u64, c.to_bits());
                        }
                    }
                    _ => return Err(bad("a vector")),
                },
                K::Handle(h) => {
                    let id = match value {
                        Value::Nil => 0,
                        ref v => int(v).ok_or_else(|| bad(&format!("a {h:?}")))?,
                    };
                    // DOES_ENTITY_EXIST and the like are how a mod checks one.
                    let checks = n.name.starts_with("DOES_") && n.name.ends_with("_EXIST");
                    // One passed by reference (DELETE_ENTITY, ..._AS_NO_LONGER_NEEDED)
                    // that is gone already: nothing to do (the engine would
                    // trap on a handle past its pool).
                    if h.is_entity() && p.dir == D::InOut && entity_type(mem, id).is_none() {
                        return Ok(MultiValue::new());
                    }
                    if h.is_entity() && p.dir != D::Out && !checks {
                        let want = match h {
                            H::Vehicle => Some(3),
                            H::Ped => Some(4),
                            H::Object => Some(5),
                            _ => None,
                        };
                        match entity_type(mem, id) {
                            None => {
                                let got = if value.is_nil() { "nil".to_string() } else { id.to_string() };
                                return Err(bad(&format!("an existing {h:?} (got {got})")));
                            }
                            Some(t) if want.is_some_and(|w| w != t) => {
                                return Err(bad(&format!("a {h:?} (got a {})", type_name(t))));
                            }
                            _ => {}
                        }
                    }
                    mem.set_i32(target, id);
                }
            }
            slot += if p.kind == K::Vec3 && p.dir == D::In { 24 } else { 8 };
        }
        // A mod's script thread is ended by its mod, not by a native
        // (engine.rs); the game's own threads may be.
        let mods_thread = match n.name {
            "TERMINATE_THREAD" => crate::engine::is_mod_thread(mem.u32(params)),
            // By name, as the engine compares them (case aside).
            "TERMINATE_ALL_SCRIPTS_WITH_THIS_NAME" => {
                mem.str(mem.u64(params)).to_ascii_lowercase().starts_with(crate::engine::PREFIX)
            }
            _ => false,
        };
        if mods_thread {
            return Err(mlua::Error::runtime(format!(
                "{}: that is a mod's script thread; a mod ends when its code returns, or from the Mods page",
                qualified(n)
            )));
        }
        // The engine fails when a pool is full: refused before that
        // (CREATE_PED, CLONE_PED, CREATE_VEHICLE...).
        if n.name.starts_with("CREATE_") || n.name.starts_with("CLONE_") {
            let pool = match n.ret {
                Some(K::Handle(H::Vehicle)) => Some(crate::world::Pool::Vehicles),
                Some(K::Handle(H::Ped)) => Some(crate::world::Pool::Peds),
                Some(K::Handle(H::Object)) => Some(crate::world::Pool::Objects),
                _ => None,
            };
            if let Some(pool) = pool
                && crate::world::free(mem, pool) < 8
            {
                return Err(mlua::Error::runtime(format!(
                    "{}: the game has no room for more ({pool:?} are full); delete some first",
                    qualified(n)
                )));
            }
        }
        mem.set_u64(info, result);
        mem.set_u32(info + 8, slots as u32);
        mem.set_u64(info + 16, params);
        mem.set_i32(info + 24, 0);
        RUNNING.store(index + 1, std::sync::atomic::Ordering::Relaxed);
        let called = unsafe { re5osx_game::call_native(env, n.handler, info) };
        RUNNING.store(0, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = called {
            TAINTED.store(true, std::sync::atomic::Ordering::Relaxed);
            return Err(mlua::Error::runtime(format!("{} stopped with a {e}", qualified(n))));
        }
        // Vectors the native took by reference go back where they came from.
        for k in 0..mem.i32(info + 24).clamp(0, 4) as u64 {
            let orig = mem.u64(info + 32 + 8 * k);
            let buf = info + 64 + 16 * k;
            for c in 0..3 {
                mem.set_u32(orig + 8 * c, mem.u32(buf + 4 * c));
            }
        }
        let mut out = Vec::new();
        if let Some(k) = n.ret {
            out.push(read(lua, mem, result, k)?);
        }
        for (buf, k) in outputs {
            out.push(read(lua, mem, buf, k)?);
        }
        Ok(MultiValue::from_vec(out))
    })();
    unsafe { exports::_emscripten_stack_restore(env, top) }.map_err(trap)?;
    result
}

fn number(v: &Value) -> Option<f64> {
    match *v {
        Value::Integer(i) => Some(i as f64),
        Value::Number(n) => Some(n),
        _ => None,
    }
}

fn int(v: &Value) -> Option<i32> {
    let n = number(v)?;
    (n.fract() == 0.0 && n >= i32::MIN as f64 && n <= u32::MAX as f64).then_some(n as i64 as i32)
}

/// A value the native left at `at`.
fn read(lua: &Lua, mem: Mem, at: u64, k: Kind) -> mlua::Result<Value> {
    Ok(match k {
        K::Int | K::Any => Value::Integer(mem.i32(at).into()),
        K::Hash => Value::Integer(mem.u32(at).into()),
        K::Float => Value::Number(f32::from_bits(mem.u32(at)).into()),
        K::Bool => Value::Boolean(mem.i32(at) != 0),
        K::Str => match mem.u64(at) {
            0 => Value::Nil,
            p => Value::String(lua.create_string(mem.str(p))?),
        },
        K::Vec3 => {
            let c = |i: u64| f32::from_bits(mem.u32(at + 8 * i));
            Value::Vector(mlua::Vector::new(c(0), c(1), c(2)))
        }
        // Player 0 is the local player; for the others 0 is none.
        K::Handle(H::Player) => Value::Integer(mem.i32(at).into()),
        K::Handle(_) => match mem.i32(at) {
            0 => Value::Nil,
            id => Value::Integer(id.into()),
        },
    })
}
