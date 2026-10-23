//! Compiles C files through a content-addressed object cache.
//!
//! The generated game is ~1 GB of C; cc would rebuild all of it whenever the
//! build script reruns. Objects are keyed by (compiler command, source, extra
//! key), so a hook change recompiles only the files it touches, and debug and
//! release builds share the game's objects.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};
use std::{fs, thread};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

pub fn compile(
    build: &cc::Build,
    sources: &[PathBuf],
    extra_key: &str,
    cache: &Path,
    jobs: usize,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(cache)?;
    let tool = build.get_compiler();
    let base_key = format!("{:?} {:?} {extra_key}", tool.path(), tool.args());

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let objects = Mutex::new(vec![PathBuf::new(); sources.len()]);
    let error = Mutex::new(None);
    thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= sources.len() || error.lock().unwrap().is_some() {
                        break;
                    }
                    match compile_one(&tool, &base_key, &sources[i], cache) {
                        Ok(obj) => objects.lock().unwrap()[i] = obj,
                        Err(e) => *error.lock().unwrap() = Some(e),
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    eprintln!("[{n}/{}] {}", sources.len(), sources[i].display());
                }
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap() {
        return Err(e);
    }
    prune(cache);
    Ok(objects.into_inner().unwrap())
}

/// Objects no build has used for a while go (each hit marks its object as
/// used; archives hold copies, so the worst a prune does is a recompile),
/// with what interrupted compiles left.
fn prune(cache: &Path) {
    const UNUSED: Duration = Duration::from_secs(14 * 24 * 3600);
    const LEFT: Duration = Duration::from_secs(3600);
    let Ok(entries) = fs::read_dir(cache) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let age = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        let stale = if name.ends_with(".o") {
            age.is_some_and(|a| a > UNUSED)
        } else {
            (name.contains(".o.tmp") || name.ends_with(".c")) && age.is_some_and(|a| a > LEFT)
        };
        if stale {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn compile_one(tool: &cc::Tool, base_key: &str, src: &Path, cache: &Path) -> Result<PathBuf> {
    let text = fs::read(src).with_context(|| format!("reading {}", src.display()))?;
    let mut hash = Sha256::new();
    hash.update(base_key.as_bytes());
    hash.update(src.to_string_lossy().as_bytes());
    hash.update(&text);
    let digest = hash.finalize();
    let name = digest.iter().take(16).map(|b| format!("{b:02x}")).collect::<String>();
    let obj = cache.join(format!("{name}.o"));
    if obj.exists() {
        // Used: kept by the prune.
        let _ = fs::File::options().write(true).open(&obj).and_then(|f| f.set_modified(SystemTime::now()));
        return Ok(obj);
    }
    let tmp = cache.join(format!("{name}.o.tmp{}", std::process::id()));
    // Compiled from a copy of the very text hashed: the source may be
    // rewritten meanwhile by another build sharing it (the generated files
    // are shared), whose object would then be kept under this text's key
    // for good. Its includes are all found through the include paths.
    let copy = cache.join(format!("{name}.{}.c", std::process::id()));
    fs::write(&copy, &text)?;
    let mut cmd: Command = tool.to_command();
    cmd.arg("-c").arg(&copy);
    if tool.is_like_msvc() {
        cmd.arg(format!("-Fo{}", tmp.display()));
    } else {
        cmd.arg("-o").arg(&tmp);
    }
    let output = cmd.output().with_context(|| format!("running {cmd:?}"));
    let _ = fs::remove_file(&copy);
    let output = output?;
    if !output.status.success() {
        let _ = fs::remove_file(&tmp);
        bail!("compiling {} failed:\n{}", src.display(), String::from_utf8_lossy(&output.stderr));
    }
    fs::rename(&tmp, &obj)?;
    Ok(obj)
}

pub fn archive(build: &cc::Build, objects: &[PathBuf], lib: &Path) -> Result<()> {
    let _ = fs::remove_file(lib);
    let mut ar = build.get_archiver();
    if build.get_compiler().is_like_msvc() {
        // lib.exe or llvm-lib
        ar.arg("-nologo").arg(format!("-out:{}", lib.display())).args(objects);
    } else {
        ar.arg("crs").arg(lib).args(objects);
    }
    let status = ar.status().with_context(|| format!("running {ar:?}"))?;
    if !status.success() {
        bail!("{ar:?} failed");
    }
    Ok(())
}
