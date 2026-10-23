//! Packs: many whole files as one zstd stream.
//!
//! The game's small text files (shader effects, XML, metadata: 3,700 files,
//! 385 MB) compress to 8.6 MB together, against 30-50 % one by one, and
//! the translated shaders (155 MB) to 1.6 MB: a client takes each pack
//! once instead of thousands of small requests. The archives (`.rpf`), which
//! are most of the game, are already compressed or encrypted and travel as
//! they are.
//!
//! Stream: `G5PK`, version (u32), then per file its catalog id (u32), size
//! (u64) and bytes, then id `u32::MAX`. Files are ordered by extension, so
//! that alike files sit within the compressor's window.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::catalog::{Catalog, Entry, hex};

const MAGIC: &[u8; 4] = b"G5PK";
const VERSION: u32 = 1;
const LEVEL: i32 = 19;
/// 32 MB: what a client needs to decode (the gain of larger windows is
/// small: 8.6 MB for the text pack against 7.6 MB with 128 MB).
const WINDOW_LOG: u32 = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Shaders,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Text, Kind::Shaders];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Shaders => "shaders",
        }
    }
}

/// Engine files read whole, which compress well.
const TEXT: [&str; 21] = [
    "fxc", "xml", "lightxml", "meta", "ipl", "dat", "sps", "txt", "json", "csv", "gfx", "cfg", "ide", "list", "html",
    "css", "js", "nametable", "rel", "ugc", "orig",
];
const TEXT_MAX: u64 = 8 << 20;

/// The pack `entry` belongs to, if any.
pub fn kind_of(catalog: &Catalog, id: usize) -> Option<Kind> {
    let e = &catalog.entries[id];
    if id >= catalog.data_files {
        return Some(Kind::Shaders);
    }
    (e.size <= TEXT_MAX && TEXT.contains(&extension(&e.path).as_str())).then_some(Kind::Text)
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_ascii_lowercase())
}

/// The ids in `kind`'s pack, in pack order.
pub fn members(catalog: &Catalog, kind: Kind) -> Vec<u32> {
    let mut ids: Vec<u32> = (0..catalog.entries.len()).filter(|&i| kind_of(catalog, i) == Some(kind)).map(|i| i as u32).collect();
    ids.sort_by_cached_key(|&i| {
        let p = &catalog.entries[i as usize].path;
        (extension(p), p.clone())
    });
    ids
}

/// Identifies a pack's content: the server rebuilds it, and a client takes
/// it again, when this changes.
pub fn hash(catalog: &Catalog, ids: &[u32]) -> String {
    let mut sha = Sha256::new();
    sha.update(format!("{}:{VERSION}:{LEVEL}:{WINDOW_LOG}", catalog.build));
    for &i in ids {
        let Entry { path, size, mtime } = &catalog.entries[i as usize];
        sha.update(path.as_bytes());
        sha.update([0]);
        sha.update(size.to_le_bytes());
        sha.update(mtime.to_le_bytes());
        sha.update(i.to_le_bytes());
    }
    hex(&sha.finalize()[..8])
}

/// Builds the pack of `ids` from the game folder `game` into `out`, through
/// a temporary file of this build's own (another build of the same pack,
/// in this process or another, writes its own and renames it the same).
/// `progress` gets the bytes read so far and the total.
pub fn build(game: &Path, catalog: &Catalog, ids: &[u32], out: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    static BUILDS: AtomicU32 = AtomicU32::new(0);
    let n = BUILDS.fetch_add(1, Ordering::Relaxed);
    let tmp = out.with_extension(format!("{}-{n}.{TMP}", std::process::id()));
    let result = write(game, catalog, ids, &tmp, progress)
        .and_then(|()| fs::rename(&tmp, out).with_context(|| format!("renaming {} to {}", tmp.display(), out.display())));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// A build's temporary file ends with this.
const TMP: &str = "tmp";
/// A temporary file older than this is a build that died.
const TMP_STALE: std::time::Duration = std::time::Duration::from_secs(3600);

fn write(game: &Path, catalog: &Catalog, ids: &[u32], tmp: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    let total: u64 = ids.iter().map(|&i| catalog.entries[i as usize].size).sum();
    let file = File::create(tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut enc = zstd::stream::Encoder::new(BufWriter::new(file), LEVEL)?;
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()) as u32;
    enc.multithread(workers)?;
    enc.long_distance_matching(true)?;
    enc.window_log(WINDOW_LOG)?;
    enc.include_checksum(true)?;
    enc.write_all(MAGIC)?;
    enc.write_all(&VERSION.to_le_bytes())?;
    let mut done = 0;
    for &i in ids {
        let e = &catalog.entries[i as usize];
        let data = fs::read(game.join(&e.path)).with_context(|| format!("reading {}", e.path))?;
        if data.len() as u64 != e.size {
            bail!("{} changed while the pack was built", e.path);
        }
        enc.write_all(&i.to_le_bytes())?;
        enc.write_all(&e.size.to_le_bytes())?;
        enc.write_all(&data)?;
        done += e.size;
        progress(done, total);
    }
    enc.write_all(&u32::MAX.to_le_bytes())?;
    enc.finish()?.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(())
}

/// Reads a pack, handing each file to `file(id, bytes)`.
pub fn unpack(compressed: impl Read, mut file: impl FnMut(u32, &[u8]) -> Result<()>) -> Result<()> {
    let mut dec = zstd::stream::Decoder::new(compressed)?;
    dec.window_log_max(WINDOW_LOG)?;
    let mut r = BufReader::with_capacity(1 << 20, dec);
    let mut head = [0u8; 8];
    r.read_exact(&mut head)?;
    if &head[..4] != MAGIC || u32::from_le_bytes(head[4..].try_into().unwrap()) != VERSION {
        bail!("not a pack of this version");
    }
    let mut data = Vec::new();
    loop {
        let mut id = [0u8; 4];
        r.read_exact(&mut id)?;
        let id = u32::from_le_bytes(id);
        if id == u32::MAX {
            return Ok(());
        }
        let mut size = [0u8; 8];
        r.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);
        if size > TEXT_MAX.max(64 << 20) {
            bail!("pack entry of {size} bytes");
        }
        data.resize(size as usize, 0);
        r.read_exact(&mut data)?;
        file(id, &data)?;
    }
}

/// Where the server keeps a built pack.
pub fn cached_path(dir: &Path, kind: Kind, hash: &str) -> PathBuf {
    dir.join(format!("{}-{hash}.zst", kind.name()))
}

/// Removes this kind's packs other than `keep`, and temporary files of
/// builds that died; not those of builds going on (another server
/// preparing the same folder: its build would find its file gone).
pub fn remove_others(dir: &Path, kind: Kind, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&format!("{}-", kind.name())) || e.path() == keep {
            continue;
        }
        let stale = !name.ends_with(&format!(".{TMP}"))
            || e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age > TMP_STALE));
        if stale {
            let _ = fs::remove_file(e.path());
        }
    }
}
