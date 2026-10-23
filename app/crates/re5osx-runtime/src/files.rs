//! The engine's files in memory, whose names carry their buffer's address:
//! "growbuffer:<address>" (a `datGrowBuffer`, the cloud's files are read
//! into) and "memory:$<address>,<size>,<flag>:<name>" (the Creator writes a
//! job's JSON into one, other parts of the engine their data). The engine
//! formats the address with `%p`, which in this build's C library begins
//! with "0x": its devices read the address back as 0 (Windows' `%p` has no
//! prefix), and the files in memory had no buffer.

use re5osx_game::Env;

use crate::memory::Mem;

/// Hook for `rage::fiDevice::MakeGrowBufferFileName(char*, int,
/// datGrowBuffer*)`.
pub(crate) unsafe extern "C" fn grow_buffer_file_name(env: *mut Env, name: u64, size: u32, buffer: u64) {
    let m = unsafe { Mem::of(env) };
    m.write_str(name, &format!("growbuffer:{buffer:x}"), size as usize);
}

/// Hook for `rage::fiDevice::MakeMemoryFileName(char*, int, void const*,
/// unsigned long, bool, char const*)`.
pub(crate) unsafe extern "C" fn memory_file_name(
    env: *mut Env,
    name: u64,
    size: u32,
    buffer: u64,
    length: u64,
    flag: u32,
    file: u64,
) {
    let m = unsafe { Mem::of(env) };
    let file = m.str(file);
    let length = length as u32 as i32;
    m.write_str(name, &format!("memory:${buffer:x},{length},{}:{file}", flag as u8), size as usize);
}

pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.grow_buffer_file_name = Some(grow_buffer_file_name);
    hooks.memory_file_name = Some(memory_file_name);
}
