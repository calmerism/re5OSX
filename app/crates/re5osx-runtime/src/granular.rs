//! Vehicle engines: their granular waves played from PCM, as on PC.
//!
//! An engine's sound is a granular synthesis of one recorded wave: short
//! grains of it (about 80 ms) are picked and crossfaded by the engine's
//! revs (`rage::audGranularSubmix`). On PC the wave is PCM and the grains
//! are read from it directly. This game data has its waves in Opus, which
//! the submix can only play through decoders (the consoles' path): every
//! grain is a packet of its own, decoded from up to four Opus packets
//! (240 ms) before it, one packet per mixer frame. The PC configuration
//! reserves no decoders for it, so the engines were silent; with decoders
//! they lag behind the grains (the next grain is not ready when the current
//! one ends: "moving to next grain before packet has been sent", "reading
//! from discarded data" in the engine's log, hundreds of thousands of
//! times), and stutter.
//!
//! Here the submix's Opus wave (a few seconds) is decoded to PCM once, with
//! the game's own decoder, and the submix plays it as on PC. Submixes of the
//! same wave share the PCM. When their grain players have shut down it is
//! kept, within a budget, for the next vehicle of the same model.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use re5osx_game::{Env, orig};

use crate::memory::Mem;

// `rage::audGranularSubmix` in this build.
/// The wave's data (u64) and its size in bytes (u32).
const SUBMIX_DATA: u64 = 192;
const SUBMIX_DATA_BYTES: u64 = 264;
/// The wave's format chunk (u64): its format byte at +19.
const SUBMIX_FORMAT: u64 = 216;
const FORMAT_CODEC: u64 = 19;
/// The wave's length in samples (u32).
const SUBMIX_SAMPLES: u64 = 260;
/// The wave's name hash (u32).
const SUBMIX_NAME: u64 = 168;
/// Played through decoders (u8): the wave is compressed.
const SUBMIX_COMPRESSED: u64 = 336;

// `rage::audGrainPlayer`: its granular mixes (u64 each, count i32), each
// with up to 3 submixes (u64 each from +0, count i32 at +24).
const PLAYER_MIXES: u64 = 40;
const PLAYER_MIX_COUNT: u64 = 88;
const MIX_SUBMIX_COUNT: u64 = 24;

/// `audWaveFormat::audStreamFormat` of Opus.
const OPUS: u8 = 12;
const SAMPLE_RATE: u32 = 48_000;
/// Samples an Opus packet of this data decodes to.
const PACKET_SAMPLES: u32 = 2880;

// `rage::audDecoderOpus`, as `audDecodeManager::AllocateDecoder` builds it.
const DECODER_BYTES: u64 = 29008;
const DECODER_STATE: u64 = 120;
/// Its output ring: buffer (u64), capacity in bytes (u32), bytes available
/// (atomic u32).
const RING: u64 = 128;
const RING_CAPACITY: u64 = 136;
const RING_AVAILABLE: u64 = 152;
const RING_DATA: u64 = 202;
const RING_BYTES: u32 = 23040;
const PLAYING: u32 = 1;

// `rage::audPacket`, as the decoder reads it.
const PACKET_BYTES: u64 = 64;
const PACKET_SIZE: u64 = 8;
const PACKET_START: u64 = 12;
const PACKET_END: u64 = 16;
const PACKET_LOOP_START: u64 = 20;
const PACKET_FINAL: u64 = 49;

/// PCM kept for waves no submix plays any more.
const SPARE_BUDGET: u64 = 24 << 20;

/// A wave: its data's address and size, and its name.
type Key = (u64, u32, u32);

struct Pcm {
    /// In the engine's heap: 16-bit samples.
    at: u64,
    bytes: u64,
    /// Submixes playing it.
    users: u32,
    /// When it was last used, for the spare budget.
    used: u64,
}

#[derive(Default)]
struct Waves {
    pcm: HashMap<Key, Pcm>,
    /// The submixes playing PCM from here, and their waves.
    submixes: HashMap<u64, Key>,
    clock: u64,
}

static WAVES: Mutex<Option<Waves>> = Mutex::new(None);

/// Hook for `rage::audGranularSubmix::audGranularSubmix(rage::audWaveRef
/// const*, rage::audGranularSubmix::audGrainData const*, unsigned int)`.
pub(crate) unsafe extern "C" fn submix_new(env: *mut Env, this: u64, wave: u64, grains: u64, count: u32) -> u64 {
    let ret = unsafe { orig::granular_submix_new(env, this, wave, grains, count) };
    let mem = unsafe { Mem::of(env) };
    let mut waves = WAVES.lock().unwrap();
    let waves = waves.get_or_insert_with(Waves::default);
    // A submix is new at this address: whatever played here is gone.
    if let Some(old) = waves.submixes.remove(&this) {
        waves.release(env, old);
    }
    let format = mem.u64(this + SUBMIX_FORMAT);
    if mem.u8(this + SUBMIX_COMPRESSED) == 0 || format == 0 || mem.u8(format + FORMAT_CODEC) != OPUS {
        return ret;
    }
    let key = (mem.u64(this + SUBMIX_DATA), mem.u32(this + SUBMIX_DATA_BYTES), mem.u32(this + SUBMIX_NAME));
    let samples = mem.u32(this + SUBMIX_SAMPLES);
    let Some(pcm) = waves.acquire(env, key, samples) else { return ret };
    mem.set_u64(this + SUBMIX_DATA, pcm);
    mem.set_u32(this + SUBMIX_DATA_BYTES, samples * 2);
    mem.set_u8(this + SUBMIX_COMPRESSED, 0);
    waves.submixes.insert(this, key);
    ret
}

/// Hook for `rage::audGrainPlayer::Shutdown()`, which deletes the player's
/// submixes.
pub(crate) unsafe extern "C" fn grain_player_shutdown(env: *mut Env, this: u64) {
    let mem = unsafe { Mem::of(env) };
    if let Some(waves) = WAVES.lock().unwrap().as_mut() {
        for i in 0..mem.i32(this + PLAYER_MIX_COUNT).max(0) as u64 {
            let mix = mem.u64(this + PLAYER_MIXES + i * 8);
            if mix == 0 {
                continue;
            }
            for j in 0..mem.i32(mix + MIX_SUBMIX_COUNT).max(0) as u64 {
                if let Some(key) = waves.submixes.remove(&mem.u64(mix + j * 8)) {
                    waves.release(env, key);
                }
            }
        }
    }
    unsafe { orig::grain_player_shutdown(env, this) }
}

impl Waves {
    /// The PCM of a wave, decoded if no other submix has it.
    fn acquire(&mut self, env: *mut Env, key: Key, samples: u32) -> Option<u64> {
        self.clock += 1;
        if let Some(pcm) = self.pcm.get_mut(&key) {
            pcm.users += 1;
            pcm.used = self.clock;
            return Some(pcm.at);
        }
        // About 1 ms for a wave of a few seconds, on the mixer thread.
        let at = decode(env, key.0, key.1, samples)?;
        let bytes = samples as u64 * 2;
        self.pcm.insert(key, Pcm { at, bytes, users: 1, used: self.clock });
        self.trim(env);
        Some(at)
    }

    fn release(&mut self, env: *mut Env, key: Key) {
        if let Some(pcm) = self.pcm.get_mut(&key) {
            pcm.users -= 1;
        }
        self.trim(env);
    }

    /// Frees the least recently used waves no submix plays beyond the budget.
    fn trim(&mut self, env: *mut Env) {
        loop {
            let spare = self.pcm.values().filter(|p| p.users == 0);
            if spare.clone().map(|p| p.bytes).sum::<u64>() <= SPARE_BUDGET {
                return;
            }
            let Some((&key, _)) = self.pcm.iter().filter(|(_, p)| p.users == 0).min_by_key(|(_, p)| p.used) else {
                return;
            };
            let pcm = self.pcm.remove(&key).unwrap();
            unsafe { orig::builtin_free(env, pcm.at) };
        }
    }
}

/// Decodes an Opus wave to 16-bit PCM in the engine's heap.
fn decode(env: *mut Env, data: u64, bytes: u32, samples: u32) -> Option<u64> {
    let mem = unsafe { Mem::of(env) };
    if data == 0 || samples == 0 {
        return None;
    }
    let out = unsafe { orig::builtin_malloc(env, samples as u64 * 2) };
    let decoder = unsafe { orig::builtin_malloc(env, DECODER_BYTES + PACKET_BYTES) };
    if out == 0 || decoder == 0 {
        unsafe {
            orig::builtin_free(env, out);
            orig::builtin_free(env, decoder);
        }
        return None;
    }
    // The decoder as the engine allocates it, without its vtable: it is
    // only called here, directly. Its lock is off (+201 = 0): one thread.
    mem.bytes_mut(decoder, (DECODER_BYTES + PACKET_BYTES) as usize).fill(0);
    mem.set_u64(decoder + RING, decoder + RING_DATA);
    mem.set_u32(decoder + RING_CAPACITY, RING_BYTES);
    let mut done = 0;
    if unsafe { orig::opus_init(env, decoder, OPUS as u32, 1, SAMPLE_RATE) } != 0 {
        // The whole wave as one packet, ending the stream.
        let packet = decoder + DECODER_BYTES;
        mem.set_u64(packet, data);
        mem.set_u32(packet + PACKET_SIZE, bytes);
        mem.set_u32(packet + PACKET_START, 0);
        mem.set_u32(packet + PACKET_END, samples);
        mem.set_u32(packet + PACKET_LOOP_START, u32::MAX);
        mem.set_u8(packet + PACKET_FINAL, 1);
        unsafe { orig::opus_submit(env, decoder, packet) };
        // Each update decodes one packet of the data, which the ring holds.
        for _ in 0..samples / PACKET_SAMPLES + 2 {
            if mem.u32(decoder + DECODER_STATE) != PLAYING || done == samples {
                break;
            }
            unsafe { orig::opus_update(env, decoder) };
            let available = mem.atomic_u32(decoder + RING_AVAILABLE).load(Ordering::Acquire) / 2;
            let n = available.min(samples - done);
            unsafe {
                orig::opus_read(env, decoder, out + done as u64 * 2, n);
                orig::opus_advance(env, decoder, n);
            }
            done += n;
        }
        unsafe { orig::opus_shutdown(env, decoder) };
    }
    unsafe { orig::builtin_free(env, decoder) };
    if done < samples {
        log::warn!("granular: Opus wave of {samples} samples decoded to {done}, left compressed");
        unsafe { orig::builtin_free(env, out) };
        return None;
    }
    Some(out)
}

/// This module's hooks.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.granular_submix_new = Some(submix_new);
    hooks.grain_player_shutdown = Some(grain_player_shutdown);
}
