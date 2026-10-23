//! Generates the natives' table (`natives.rs`) and their Luau definitions
//! (`natives.d.luau`) from what game.wasm says about each native (re5osx-game's
//! build script, `DEP_RE5OSX_GAME_NATIVES`) and, when present, the natives
//! database (`third_party/nativedb/natives.json`, from `mise run setup`):
//! names of parameters, documentation, the handle types.
//!
//! The binary decides what can be called and how (handler, slots); the
//! database adds what the binary does not say. Without the database the
//! natives with a typed command function are still there, with parameters
//! named `p0`, `p1`...

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, Result};
use serde_json::Value;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// What the generated code calls a value (re5osx-mods' `Kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Int,
    Float,
    Bool,
    Hash,
    Str,
    Vec3,
    Any,
    Handle(Handle),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::enum_variant_names)] // ScrHandle: the database's name
enum Handle {
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

const HANDLES: &[(&str, Handle, Option<&str>)] = &[
    ("Entity", Handle::Entity, None),
    ("Ped", Handle::Ped, Some("Entity")),
    ("Vehicle", Handle::Vehicle, Some("Entity")),
    ("Object", Handle::Object, Some("Entity")),
    ("Player", Handle::Player, None),
    ("Cam", Handle::Cam, None),
    ("Blip", Handle::Blip, None),
    ("Pickup", Handle::Pickup, None),
    ("Interior", Handle::Interior, None),
    ("ScrHandle", Handle::ScrHandle, None),
    ("FireId", Handle::FireId, None),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    In,
    Out,
    InOut,
}

#[derive(Clone, Debug)]
struct Param {
    name: String,
    kind: Kind,
    dir: Dir,
}

impl Param {
    fn slots(&self) -> usize {
        if self.kind == Kind::Vec3 && self.dir == Dir::In { 3 } else { 1 }
    }
}

struct Native {
    name: String,
    hash: u64,
    handler: u64,
    namespace: String,
    lua: String,
    params: Vec<Param>,
    ret: Option<Kind>,
    doc: String,
}

/// One line of re5osx-game's table.
struct Binary {
    name: String,
    hash: u64,
    handler: u64,
    unused: bool,
    group: String,
    command: Option<Vec<String>>,
    /// The typed command's wasm result: "", "i32", "f32", "i64"; "sret"
    /// for a vector returned through a pointer.
    result: Option<String>,
    signature: u64,
}

/// Natives whose handle is never none: the player's ped.
const NEVER_NONE: &[&str] = &["PLAYER_PED_ID", "GET_PLAYER_PED", "GET_PLAYER_PED_SCRIPT_INDEX"];

/// Natives the mods may not call: the script thread's own control (a mod's
/// thread only waits through `wait`, and ends through `mod`).
const RESERVED: &[&str] = &["WAIT", "TERMINATE_THIS_THREAD"];

/// Natives that keep the pointer they are given past the call: a mod's
/// arguments are on the stack only while the native runs. Save blocks
/// (their ends too: nothing a mod can open), variables shared over the
/// network, variables the anti-cheat watches.
fn keeps_pointer(name: &str) -> bool {
    name.starts_with("START_SAVE_")
        || name.starts_with("STOP_SAVE_")
        || name.ends_with("_TO_SAVE")
        || (name.starts_with("NETWORK_REGISTER_") && name.ends_with("_BROADCAST_VARIABLES"))
        || name == "REGISTER_SCRIPT_VARIABLE"
        || name == "UNREGISTER_SCRIPT_VARIABLE"
}

/// Natives that read a structure through what the database calls an
/// `int*` (`CTaskScriptedAnimation::ScriptInitSlotData`): one value would
/// leave them reading past it.
const STRUCTURES: &[&str] = &["TASK_SCRIPTED_ANIMATION", "PLAY_ENTITY_SCRIPTED_ANIM"];

fn run() -> Result<()> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let root = manifest.parent().and_then(Path::parent).context("workspace root")?.to_path_buf();
    let out = PathBuf::from(env::var("OUT_DIR")?);
    let table = PathBuf::from(env::var("DEP_RE5OSX_GAME_NATIVES").context("re5osx-game's natives table")?);
    let db_path = root.join("third_party/nativedb/natives.json");
    println!("cargo:rerun-if-changed={}", table.display());
    println!("cargo:rerun-if-changed={}", db_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let binary = parse_table(&fs::read_to_string(&table)?)?;
    let db: Option<Value> = match fs::read(&db_path) {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).context("parsing natives.json")?),
        Err(_) => {
            println!(
                "cargo:warning=no {} (`mise run setup`): natives without documentation or names of parameters",
                db_path.display()
            );
            None
        }
    };
    // The database's entries by name, with their namespace.
    let mut by_name: HashMap<String, (String, &Value)> = HashMap::new();
    if let Some(Value::Object(spaces)) = &db {
        for (space, entries) in spaces {
            if let Value::Object(entries) = entries {
                for entry in entries.values() {
                    if let Some(name) = entry["name"].as_str() {
                        by_name.insert(name.to_string(), (space.clone(), entry));
                    }
                }
            }
        }
    }
    // Each of the binary's groups takes the namespace most of its natives
    // have in the database (camera -> CAM), so the names match the docs.
    let mut votes: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for b in &binary {
        if let Some((space, _)) = by_name.get(&b.name) {
            *votes.entry(&b.group).or_default().entry(space).or_default() += 1;
        }
    }
    let namespace_of = |group: &str| -> String {
        votes
            .get(group)
            .and_then(|v| v.iter().max_by_key(|&(name, n)| (*n, std::cmp::Reverse(*name))).map(|(s, _)| s.to_ascii_lowercase()))
            .unwrap_or_else(|| group.to_string())
    };

    let mut natives = Vec::new();
    let mut skipped = BTreeMap::<&str, usize>::new();
    let mut reserved = Vec::new();
    for b in &binary {
        if RESERVED.contains(&b.name.as_str()) {
            reserved.push((b.name.clone(), b.handler));
            *skipped.entry("reserved").or_default() += 1;
            continue;
        }
        if keeps_pointer(&b.name) || STRUCTURES.contains(&b.name.as_str()) {
            *skipped.entry("pointers kept or structures").or_default() += 1;
            continue;
        }
        let entry = by_name.get(&b.name).map(|(_, e)| *e);
        let from_db = entry.map(|e| db_params(e, &b.name)).transpose()?;
        let from_binary = b.command.as_ref().map(|c| binary_params(c, &b.name));
        // A pointer the database cannot type (`Any*`: a structure, an array,
        // a gamer handle) where the binary takes an `int&`, which is also
        // how the commands take the scripts' structures: its size is
        // unknown, so a buffer of one value could be written past
        // (COPY_SCRIPT_STRUCT, GET_EVENT_DATA, NETWORK_HANDLE_FROM_PLAYER).
        // A `float&` or a `Vector3&` is one value; where the binary takes a
        // value, the database is wrong and the binary is right (ADD_ROPE).
        let untyped = entry.is_some_and(untyped_pointer);
        let params = match (from_db, from_binary) {
            (Some(Some(db)), Some(Some(bin))) => merge(db, &bin),
            (Some(Some(db)), _) => db,
            (_, Some(Some(bin))) if untyped && bin.iter().any(|p| p.dir != Dir::In && p.kind == Kind::Int) => {
                *skipped.entry("untyped pointers").or_default() += 1;
                continue;
            }
            (_, Some(Some(bin))) => bin,
            _ => {
                *skipped.entry(if b.unused { "unused, untyped" } else { "untyped" }).or_default() += 1;
                continue;
            }
        };
        let ret = match entry.and_then(|e| e["return_type"].as_str()) {
            Some(t) => match value_kind(t) {
                Some(k) => Some(k),
                None if t == "void" => None,
                None => {
                    *skipped.entry("return type").or_default() += 1;
                    continue;
                }
            },
            None => match binary_return(b) {
                Some(r) => r,
                None => {
                    *skipped.entry("return type").or_default() += 1;
                    continue;
                }
            },
        };
        let doc = entry.and_then(|e| e["comment"].as_str()).unwrap_or("").to_string();
        natives.push(Native {
            name: b.name.clone(),
            hash: b.hash,
            handler: b.handler,
            namespace: namespace_of(&b.group),
            lua: camel(&b.name),
            params,
            ret,
            doc,
        });
    }
    natives.sort_by(|a, b| (&a.namespace, &a.lua).cmp(&(&b.namespace, &b.lua)));
    let skipped_text = skipped.iter().map(|(why, n)| format!("{n} {why}")).collect::<Vec<_>>().join(", ");
    println!("cargo:rustc-env=RE5OSX_MODS_NATIVES_SUMMARY={} natives; not exposed: {skipped_text}", natives.len());

    fs::write(out.join("natives.rs"), rust_table(&natives, &reserved))?;
    fs::write(out.join("natives.d.luau"), definitions(&natives))?;
    checker(&root)?;
    let web = studio_files(&root, &out)?;
    assets(&out, &web)?;
    Ok(())
}

/// The Studio's files, published path and contents: crates/re5osx-studio
/// built for wasm (a cargo of its own, with its own target directory, then
/// wasm-bindgen), its page and style, the editor's modules from
/// `third_party/codemirror` (an import map in the page names them), the
/// app's fonts.
fn studio_files(root: &Path, out: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let studio = root.join("crates/re5osx-studio");
    let codemirror = root.join("third_party/codemirror");
    for p in ["src", "js", "web", "Cargo.toml", "Cargo.lock", "packages.txt"] {
        println!("cargo:rerun-if-changed={}", studio.join(p).display());
    }
    println!("cargo:rerun-if-changed={}", codemirror.join(".re5osx-stamp").display());
    anyhow::ensure!(codemirror.join(".re5osx-stamp").exists(), "{} is missing: run `mise run setup`", codemirror.display());

    // The wasm: a cargo of its own (the outer one holds this target
    // directory), without this build's flags, which are for its target.
    // (A relative target directory is cargo's, from the workspace root.)
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(|t| root.join(t))
        .unwrap_or_else(|| root.join("target"))
        .join("re5osx-tools/studio");
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(studio.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir);
    for (key, _) in env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("CARGO_") && key != "CARGO_HOME" || key == "RUSTFLAGS" || key == "RUSTC_WORKSPACE_WRAPPER" {
            command.env_remove(key.as_ref());
        }
    }
    let status = command.status().context("building re5osx-studio")?;
    anyhow::ensure!(status.success(), "building re5osx-studio (wasm) failed");
    let wasm = target_dir.join("wasm32-unknown-unknown/release/re5osx_studio.wasm");
    let bindgen = out.join("studio");
    let _ = fs::remove_dir_all(&bindgen);
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(&wasm)
        .out_name("studio")
        .web(true)?
        .typescript(false)
        .debug(false)
        .keep_debug(false)
        .generate(&bindgen)?;

    let mut files = Vec::new();
    let add_tree = |dir: &Path, prefix: &str, files: &mut Vec<(String, Vec<u8>)>| -> Result<()> {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d)? {
                let p = e?.path();
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                    files.push((format!("{prefix}/{rel}"), fs::read(&p)?));
                }
            }
        }
        Ok(())
    };
    add_tree(&bindgen, "", &mut files)?;
    add_tree(&codemirror, "/cm", &mut files)?;
    for f in ["style.css", "logo.png"] {
        files.push((format!("/{f}"), fs::read(studio.join("web").join(f))?));
    }
    let fonts = root.join("crates/re5osx/assets/ui");
    for f in ["Barlow-SemiBold.ttf", "BarlowCondensed-Bold.ttf"] {
        println!("cargo:rerun-if-changed={}", fonts.join(f).display());
        files.push((format!("/fonts/{f}"), fs::read(fonts.join(f))?));
    }
    // The page, with the import map of the editor's modules.
    let mut imports = serde_json::Map::new();
    for line in fs::read_to_string(studio.join("packages.txt"))?.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 4 && !line.starts_with('#') {
            imports.insert(f[0].to_string(), format!("/cm/{}/{}", f[0], f[3]).into());
        }
    }
    let map = serde_json::json!({ "imports": imports }).to_string();
    let page = fs::read_to_string(studio.join("web/index.html"))?.replace("<!--IMPORTMAP-->", &map);
    files.push(("/index.html".into(), page.into_bytes()));
    files.retain(|(p, _)| !p.ends_with("LICENSE") && !p.ends_with(".d.ts"));
    Ok(files)
}

/// Embeds `files` (assets.rs): each with a gzip copy when that is smaller.
fn assets(out: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    use std::io::Write as _;
    let dir = out.join("web");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut s = String::from("pub static ASSETS: &[(&str, Asset)] = &[\n");
    for (i, (path, body)) in sorted.iter().enumerate() {
        let raw = dir.join(format!("{i}"));
        fs::write(&raw, body)?;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(body)?;
        let gz = gz.finish()?;
        let gzip = if gz.len() + 64 < body.len() {
            let at = dir.join(format!("{i}.gz"));
            fs::write(&at, &gz)?;
            format!("Some(include_bytes!({:?}))", at.display().to_string())
        } else {
            "None".into()
        };
        let mime = match path.rsplit('.').next().unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "wasm" => "application/wasm",
            "json" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" => "image/jpeg",
            "ttf" => "font/ttf",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        // Revalidated every time: an ETag (the content's hash) makes that a
        // 304 when nothing changed, and a new app never serves stale files.
        let etag = {
            use sha2::Digest as _;
            let h = sha2::Sha256::digest(body);
            format!("\"{}\"", h.iter().take(8).map(|b| format!("{b:02x}")).collect::<String>())
        };
        let _ = writeln!(
            s,
            "    ({path:?}, Asset {{ body: include_bytes!({:?}), gzip: {gzip}, mime: {mime:?}, etag: {etag:?} }}),",
            raw.display().to_string()
        );
    }
    s.push_str("];\n");
    fs::write(out.join("assets.rs"), s)?;
    Ok(())
}

/// Luau's type checker (Luau.Analysis, from `third_party/luau`, the release
/// mlua builds the rest of Luau from) and our C API over it.
fn checker(root: &Path) -> Result<()> {
    let luau = root.join("third_party/luau");
    anyhow::ensure!(luau.join("Analysis").is_dir(), "{} is missing: run `mise run setup`", luau.display());
    println!("cargo:rerun-if-changed=csrc");
    println!("cargo:rerun-if-changed={}", luau.join(".re5osx-stamp").display());
    let mut sources: Vec<PathBuf> = fs::read_dir(luau.join("Analysis/src"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    sources.retain(|p| p.extension().is_some_and(|e| e == "cpp"));
    sources.sort();
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .opt_level(2)
        .debug(false)
        .define("LUA_API", "extern \"C\"")
        .define("LUACODE_API", "extern \"C\"");
    for dir in ["Analysis", "Ast", "Bytecode", "Common", "Config", "Compiler", "VM"] {
        build.include(luau.join(dir).join("include"));
    }
    if env::var("CARGO_CFG_TARGET_ENV")? == "msvc" {
        build.flag("/EHsc");
    }
    build.files(&sources).file("csrc/checker.cpp").compile("re5osx_luau_analysis");
    Ok(())
}

fn parse_table(text: &str) -> Result<Vec<Binary>> {
    text.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            anyhow::ensure!(f.len() == 8, "bad natives line {line:?}");
            let hex = |s: &str| u64::from_str_radix(s.trim_start_matches("0x"), 16);
            let command = (f[5] != "-").then(|| {
                if f[5] == "void" { Vec::new() } else { f[5].split(", ").map(str::to_string).collect() }
            });
            let result = (f[6] != "-").then(|| {
                let (params, result) = f[6].split_once("->").unwrap_or(("", ""));
                let wasm_params = params.split(',').filter(|p| !p.is_empty()).count();
                let typed = command.as_ref().map_or(0, Vec::len);
                if result.is_empty() && wasm_params == typed + 1 { "sret".to_string() } else { result.to_string() }
            });
            Ok(Binary {
                name: f[0].to_string(),
                hash: hex(f[1])?,
                handler: f[2].parse()?,
                unused: f[3] == "1",
                group: f[4].to_string(),
                command,
                result,
                signature: hex(f[7])?,
            })
        })
        .collect()
}

fn handle(name: &str) -> Option<Handle> {
    HANDLES.iter().find(|(n, ..)| *n == name).map(|&(_, h, _)| h)
}

/// A value type of the database.
fn value_kind(t: &str) -> Option<Kind> {
    Some(match t {
        "int" => Kind::Int,
        "float" => Kind::Float,
        "BOOL" => Kind::Bool,
        "Hash" => Kind::Hash,
        "const char*" => Kind::Str,
        "Vector3" => Kind::Vec3,
        "Any" => Kind::Any,
        other => Kind::Handle(handle(other)?),
    })
}

/// Pointers to a value: outputs of getters and predicates, else the value
/// goes in and comes back (`DELETE_ENTITY(Entity*)`).
fn pointer_dir(native: &str) -> Dir {
    let getter = ["GET_", "IS_", "HAS_", "_GET_", "DOES_", "FIND_", "CAN_"].iter().any(|p| native.starts_with(p));
    if getter { Dir::Out } else { Dir::InOut }
}

/// The database's parameters, `None` when one has no type the mods can
/// pass (pointers to structures, buffers).
fn db_params(entry: &Value, native: &str) -> Result<Option<Vec<Param>>> {
    let mut params = Vec::new();
    for p in entry["params"].as_array().context("params")? {
        let t = p["type"].as_str().context("type")?;
        let name = p["name"].as_str().unwrap_or("");
        let (kind, dir) = if t == "const char*" {
            (Kind::Str, Dir::In)
        } else if let Some(inner) = t.strip_suffix('*') {
            match value_kind(inner) {
                Some(k) if !matches!(k, Kind::Any | Kind::Str) => (k, pointer_dir(native)),
                _ => return Ok(None),
            }
        } else {
            match value_kind(t) {
                Some(k) => (k, Dir::In),
                None => return Ok(None),
            }
        };
        params.push(Param { name: param_name(name), kind, dir });
    }
    Ok(Some(params))
}

/// Whether one of the database's parameters is a pointer to something
/// the mods cannot pass (`Any*`, `char*`, a structure).
fn untyped_pointer(entry: &Value) -> bool {
    entry["params"].as_array().is_some_and(|params| {
        params.iter().filter_map(|p| p["type"].as_str()).any(|t| {
            t != "const char*"
                && t.strip_suffix('*').is_some_and(|inner| !value_kind(inner).is_some_and(|k| !matches!(k, Kind::Any | Kind::Str)))
        })
    })
}

/// The typed command's C++ parameters.
fn binary_params(command: &[String], native: &str) -> Option<Vec<Param>> {
    command
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (kind, dir) = match t.as_str() {
                "int" | "unsigned int" => (Kind::Int, Dir::In),
                "bool" => (Kind::Bool, Dir::In),
                "float" => (Kind::Float, Dir::In),
                "char const*" => (Kind::Str, Dir::In),
                "rage::scrVector const&" | "rage::Vector3 const&" => (Kind::Vec3, Dir::In),
                "int&" => (Kind::Int, pointer_dir(native)),
                "float&" => (Kind::Float, pointer_dir(native)),
                "bool&" => (Kind::Bool, pointer_dir(native)),
                "rage::Vector3&" | "rage::scrVector&" => (Kind::Vec3, pointer_dir(native)),
                _ => return None,
            };
            Some(Param { name: format!("p{i}"), kind, dir })
        })
        .collect()
}

/// The database's parameters checked against the binary's, slot by slot:
/// the binary's count and kinds of values, the database's names and
/// finer types where they agree (a newer database may have parameters
/// this build lacks); the binary's vectors where the database has three
/// floats.
fn merge(db: Vec<Param>, bin: &[Param]) -> Vec<Param> {
    // The database's parameters by the slot they start at.
    let mut at = HashMap::new();
    let mut slot = 0;
    for p in &db {
        at.insert(slot, p);
        slot += p.slots();
    }
    let compatible = |b: &Param, d: &Param| {
        (b.dir == Dir::In) == (d.dir == Dir::In)
            && match (b.kind, d.kind) {
                (Kind::Int, Kind::Int | Kind::Any | Kind::Hash | Kind::Handle(_)) => true,
                (Kind::Bool, Kind::Bool | Kind::Any) => true,
                (a, b) => a == b,
            }
    };
    let mut out = Vec::new();
    let mut slot = 0;
    for b in bin {
        let p = match at.get(&slot) {
            Some(d) if b.kind == Kind::Vec3 && b.dir == Dir::In && d.kind == Kind::Float => {
                let floats = (0..3).all(|i| at.get(&(slot + i)).is_some_and(|x| x.kind == Kind::Float));
                Param { name: if floats { vector_name(&d.name) } else { format!("p{}", out.len()) }, ..b.clone() }
            }
            Some(d) if compatible(b, d) => (*d).clone(),
            Some(d) => Param { name: d.name.clone(), ..b.clone() },
            None => b.clone(),
        };
        slot += b.slots();
        out.push(p);
    }
    out
}

/// What the binary says about the result, when the database has nothing.
fn binary_return(b: &Binary) -> Option<Option<Kind>> {
    match b.result.as_deref() {
        Some("") => Some(None),
        Some("sret") => Some(Some(Kind::Vec3)),
        Some("f32") => Some(Some(Kind::Float)),
        Some("i64") => Some(Some(Kind::Str)),
        Some("i32") => Some(Some(if (b.signature >> 1) & 7 == 2 { Kind::Bool } else { Kind::Int })),
        _ => None,
    }
}

/// `CREATE_VEHICLE` -> `createVehicle`.
fn camel(name: &str) -> String {
    let mut out = String::new();
    for (i, word) in name.split('_').filter(|w| !w.is_empty()).enumerate() {
        let lower = word.to_ascii_lowercase();
        if i == 0 {
            out.push_str(&lower);
        } else {
            let mut chars = lower.chars();
            if let Some(c) = chars.next() {
                out.push(c.to_ascii_uppercase());
                out.extend(chars);
            }
        }
    }
    out
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not",
    "or", "repeat", "return", "then", "true", "until", "while", "continue", "type", "export",
];

/// A database parameter's name in Luau's style: `bScriptHostVeh` ->
/// `scriptHostVeh`, snake case to camel case, no keywords.
fn param_name(name: &str) -> String {
    let mut n = name.to_string();
    let bytes = n.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'b' && bytes[1].is_ascii_uppercase() {
        n = format!("{}{}", n[1..2].to_ascii_lowercase(), &n[2..]);
    }
    if n.contains('_') {
        let mut c = camel(&n);
        if c.is_empty() {
            c = "value".into();
        }
        n = c;
    }
    if n.is_empty() || !n.chars().next().unwrap().is_ascii_alphabetic() {
        n = format!("p{n}");
    }
    if KEYWORDS.contains(&n.as_str()) {
        n.push('_');
    }
    n
}

/// `x` -> `coords`, `posX` -> `pos`, `x1` -> `coords1`.
fn vector_name(x: &str) -> String {
    let lower = x.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix('x')
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        return format!("coords{rest}");
    }
    let stem = x.strip_suffix('X').or_else(|| x.strip_suffix('x')).unwrap_or(x);
    match stem {
        "" => "coords".into(),
        k if KEYWORDS.contains(&k) => format!("{k}_"),
        _ => stem.to_string(),
    }
}

fn rust_kind(k: Kind) -> String {
    match k {
        Kind::Handle(h) => format!("K::Handle(H::{h:?})"),
        other => format!("K::{other:?}"),
    }
}

fn rust_table(natives: &[Native], reserved: &[(String, u64)]) -> String {
    let mut spaces: Vec<&str> = natives.iter().map(|n| n.namespace.as_str()).collect();
    spaces.dedup();
    let mut s = String::new();
    s.push_str("// Generated by re5osx-mods' build script from game.wasm and nativedb.\n");
    let _ = writeln!(s, "pub const NAMESPACES: &[&str] = &{spaces:?};");
    s.push_str("pub static NATIVES: &[Native] = &[\n");
    for n in natives {
        let ns = spaces.iter().position(|&x| x == n.namespace).unwrap();
        let params: Vec<String> = n
            .params
            .iter()
            .map(|p| format!("P {{ kind: {}, dir: D::{:?} }}", rust_kind(p.kind), p.dir))
            .collect();
        let ret = n.ret.map_or("None".to_string(), |k| format!("Some({})", rust_kind(k)));
        let _ = writeln!(
            s,
            "    Native {{ ns: {ns}, lua: {:?}, name: {:?}, hash: {:#x}, handler: {}, params: &[{}], ret: {ret}, doc: {:?} }},",
            n.lua,
            n.name,
            n.hash,
            n.handler,
            params.join(", "),
            n.doc.trim()
        );
    }
    s.push_str("];\n");
    let _ = writeln!(s, "pub const RESERVED: &[(&str, u64)] = &{reserved:?};");
    s
}

fn luau_type(k: Kind, input: bool) -> String {
    match k {
        Kind::Int | Kind::Float => "number".into(),
        Kind::Bool => "boolean".into(),
        Kind::Hash if input => "Hash | string".into(),
        Kind::Hash => "Hash".into(),
        Kind::Str => "string".into(),
        Kind::Vec3 => "vector".into(),
        Kind::Any => "any".into(),
        Kind::Handle(h) => format!("{h:?}"),
    }
}

/// What a `Hash` parameter takes: the names of the catalog (world.rs) when
/// the parameter is a weapon or a vehicle's or ped's model, any name for
/// other models (objects' names are not known), else any string.
fn hash_type(native: &str, ns: &str, param: &str) -> &'static str {
    let p = param.to_ascii_lowercase();
    if p.contains("weapon") {
        return "WeaponName | Hash";
    }
    let model = p == "model" || p.ends_with("model") || p.contains("modelhash");
    if !model {
        return "Hash | string";
    }
    if native.starts_with("IS_") {
        return "Model | Hash | string";
    }
    if ns == "ped" || native.starts_with("CREATE_PED") || native.contains("PED_MODEL") || native.contains("PLAYER_MODEL") {
        return "PedModel | Hash";
    }
    if ns == "vehicle" || native.contains("VEHICLE") {
        return "VehicleModel | Hash";
    }
    "Model | Hash | string"
}

/// Handles the natives return: 0 (none) becomes nil, so they are optional
/// (but for players: 0 is the local one).
fn luau_result(k: Kind) -> String {
    match k {
        Kind::Handle(Handle::Player) => "Player".into(),
        Kind::Handle(h) => format!("{h:?}?"),
        other => luau_type(other, false),
    }
}

/// `task.spawn`, `task.delay`, `task.wait` (vm.rs), beside the TASK_ natives.
const TASK_FIELDS: &str = "\
\t--- Runs `f` now, beside the mod's main code (it may wait).
\tspawn: <A...>(f: (A...) -> ...any, A...) -> (),
\t--- Runs `f` in `ms` milliseconds of game time.
\tdelay: <A...>(ms: number, f: (A...) -> ...any, A...) -> (),
\t--- Back to the game for `ms` milliseconds, as `wait`.
\twait: (ms: number?) -> (),
";

/// What the mods' VM adds besides the natives (vm.rs).
const GLOBALS: &str = "\
type Hash = number

-- Back to the game for `ms` milliseconds (0 or none: until the next frame).
declare function wait(ms: number?): ()
-- The hash the game uses for a name (GET_HASH_KEY).
declare function joaat(name: string): Hash
declare mod: { name: string }

export type Toggle = { id: number, value: boolean, set: (self: Toggle, value: boolean | string) -> () }
export type Slider = { id: number, value: number, set: (self: Slider, value: number | string) -> () }
export type Choice = { id: number, value: number, set: (self: Choice, value: number | string) -> () }
export type Button = { id: number, set: (self: Button, label: string) -> () }
export type Menu = {
	id: number,
	-- A switch; `value` follows the player, `onChange` runs as a task.
	toggle: (self: Menu, label: string, on: boolean?, note: (string | (on: boolean) -> ())?, onChange: ((on: boolean) -> ())?) -> Toggle,
	slider: (self: Menu, label: string, value: number, min: number, max: number, step: number?, onChange: ((value: number) -> ())?) -> Slider,
	-- `value` is the option's index, from 1.
	choice: (self: Menu, label: string, options: { string }, index: number?, onChange: ((index: number, option: string) -> ())?) -> Choice,
	button: (self: Menu, label: string, note: (string | () -> ())?, onPress: (() -> ())?) -> Button,
	label: (self: Menu, text: string) -> Button,
	remove: (self: Menu) -> (),
}

declare world: {
\t-- The entities around, as handles (at most `max`, 1024 by default).
\tvehicles: (max: number?) -> { Vehicle },
\tpeds: (max: number?) -> { Ped },
\tobjects: (max: number?) -> { Object },
\t-- The installed game's names (empty until the game has loaded once).
\tvehicleModels: () -> { VehicleModel },
\tpedModels: () -> { PedModel },
\tweapons: () -> { WeaponName },
}

declare json: {
	-- The value as JSON text: a table with keys 1..n (or none) is an array, another an object
	-- (string keys; number keys written as strings); a vector is [x, y, z]. `pretty` indents it.
	-- Text over 1 MB (what storage keeps), or tables nested over 64 deep (a table that holds itself), is an error.
	encode: (value: any, pretty: boolean?) -> string,
	-- JSON text as Luau values; null is nil.
	decode: (text: string) -> any,
}

-- What the mod keeps between sessions, by key (JSON values, 1 MB at most); written within a
-- couple of seconds of a change and when the mod stops. Kept for this player, not in the mod's folder.
declare storage: {
	-- The value kept under `key` (a copy), or nil.
	get: (key: string) -> any,
	-- Keeps a copy of `value` under `key`; nil removes it.
	set: (key: string, value: any) -> (),
	keys: () -> { string },
	clear: () -> (),
}

declare ui: {
	-- A menu the app shows (the Mods page's MENUS: F7, or the button beside the settings
	-- button on touch screens); a mod with one stays running. 16 menus of 64 items at most.
	menu: (title: string) -> Menu,
	-- A notification over the game.
	notify: (text: string) -> (),
}

";

fn definitions(natives: &[Native]) -> String {
    let mut s = String::new();
    s.push_str("--!strict\n-- The game's natives, generated from game.wasm and nativedb by re5osx-mods. Do not edit.\n\n");
    for (name, _, base) in HANDLES {
        let base = base.map(|b| format!(" extends {b}")).unwrap_or_default();
        let _ = writeln!(s, "declare extern type {name}{base} with end");
    }
    s.push_str(GLOBALS);
    let mut i = 0;
    while i < natives.len() {
        let ns = &natives[i].namespace;
        let _ = writeln!(s, "declare {ns}: {{");
        if ns == "task" {
            // The mods' scheduler shares the table with the TASK_ natives.
            s.push_str(TASK_FIELDS);
        }
        while i < natives.len() && &natives[i].namespace == ns {
            let n = &natives[i];
            let inputs: Vec<String> = n
                .params
                .iter()
                .filter(|p| p.dir != Dir::Out)
                .map(|p| match p.kind {
                    Kind::Hash => format!("{}: {}", p.name, hash_type(&n.name, &n.namespace, &p.name)),
                    k => format!("{}: {}", p.name, luau_type(k, true)),
                })
                .collect();
            let mut outputs: Vec<String> = n
                .ret
                .into_iter()
                .map(|k| if NEVER_NONE.contains(&n.name.as_str()) { luau_type(k, false) } else { luau_result(k) })
                .collect();
            outputs.extend(n.params.iter().filter(|p| p.dir != Dir::In).map(|p| luau_result(p.kind)));
            let result = match outputs.len() {
                0 => "()".to_string(),
                1 if !outputs[0].contains('|') => outputs[0].clone(),
                _ => format!("({})", outputs.join(", ")),
            };
            let _ = writeln!(s, "\t{}: ({}) -> {},", n.lua, inputs.join(", "), result);
            i += 1;
        }
        s.push_str("}\n\n");
    }
    s
}
