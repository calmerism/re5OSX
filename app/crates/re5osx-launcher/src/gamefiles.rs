//! Readers for the game's own files, for the launcher's art and menu sounds.
//!
//! - RPF7 archives (`data/x64/audio/sfx/*.rpf`): their table of contents is
//!   AES-256 encrypted with a key that the game's own build carries;
//!   [`find_key`] looks for it in `game.wasm` (the 32 bytes that decrypt the
//!   archive's first entry into a directory entry).
//! - AWC wave containers: this build stores waves as Opus, each block a
//!   length byte and one packet (mono, 48 kHz).

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use aes::Aes256;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use anyhow::{Context, Result, bail};

/// AES-256-ECB; a tail shorter than a block stays as it is (the archives' format).
pub fn aes_decrypt(key: &[u8; 32], data: &mut [u8]) {
    let cipher = Aes256::new(GenericArray::from_slice(key));
    let n = data.len() - data.len() % 16;
    for block in data[..n].chunks_exact_mut(16) {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
}

/// The archives' AES key: the 32 bytes of the game's build that decrypt
/// `rpf`'s first entry into a directory.
pub fn find_key(wasm: &Path, rpf: &Path) -> Result<[u8; 32]> {
    let mut head = [0u8; 32];
    File::open(rpf)?.read_exact(&mut head)?;
    let head: [u8; 16] = head[16..32].try_into().unwrap();
    let data = std::fs::read(wasm).with_context(|| format!("reading {}", wasm.display()))?;
    let tries = |i: usize| {
        let key: &[u8; 32] = data[i..i + 32].try_into().unwrap();
        let mut block = head;
        Aes256::new(GenericArray::from_slice(key)).decrypt_block(GenericArray::from_mut_slice(&mut block));
        block[4..8] == [0x00, 0xff, 0xff, 0x7f]
    };
    // Keys sit 4-byte aligned in the build's data; every byte as a fallback.
    let end = data.len().saturating_sub(32);
    let found = (0..end).step_by(4).find(|&i| tries(i)).or_else(|| (0..end).filter(|i| i % 4 != 0).find(|&i| tries(i)));
    match found {
        Some(i) => Ok(data[i..i + 32].try_into().unwrap()),
        None => bail!("the archives' key was not found in {}", wasm.display()),
    }
}

enum Entry {
    Dir { name: String, first: u32, count: u32 },
    File { name: String, offset: u64, size: u32, unpacked: u32, encrypted: bool },
}

/// An RPF7 archive: [`Rpf::read`] returns one of its files (decrypted, inflated).
pub struct Rpf {
    file: File,
    key: [u8; 32],
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
}

impl Rpf {
    pub fn open(path: &Path, key: [u8; 32]) -> Result<Rpf> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;
        let word = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
        let (count, names_length, encryption) = (word(4) as usize, word(8), word(12));
        // The names' offsets are in units of 2 ** (the top nibble of their length).
        let (shift, names_length) = (names_length >> 28, (names_length & 0x0FFF_FFFF) as usize);
        let mut toc = vec![0u8; count * 16 + names_length];
        file.read_exact(&mut toc)?;
        if encryption == 0x0FFF_FFF9 {
            aes_decrypt(&key, &mut toc);
        }
        let names = &toc[count * 16..];
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let e = &toc[i * 16..i * 16 + 16];
            let word = u64::from_le_bytes(e[..8].try_into().unwrap());
            let u32_at = |at: usize| u32::from_le_bytes(e[at..at + 4].try_into().unwrap());
            let at = ((word & 0xFFFF) << shift) as usize;
            let end = names[at..].iter().position(|&b| b == 0).context("archive name")? + at;
            let name: String = names[at..end].iter().map(|&b| b as char).collect();
            if u32_at(4) == 0x7FFF_FF00 {
                entries.push(Entry::Dir { name, first: u32_at(8), count: u32_at(12) });
            } else {
                entries.push(Entry::File {
                    name,
                    offset: (word >> 40) & 0x7F_FFFF,
                    size: ((word >> 16) & 0xFF_FFFF) as u32,
                    unpacked: u32_at(8),
                    encrypted: u32_at(12) == 1,
                });
            }
        }
        let mut rpf = Rpf { file, key, entries, index: HashMap::new() };
        if !rpf.entries.is_empty() {
            rpf.walk(0, String::new());
        }
        Ok(rpf)
    }

    fn walk(&mut self, i: usize, prefix: String) {
        match &self.entries[i] {
            Entry::Dir { name, first, count } => {
                let inner = if i == 0 { String::new() } else { format!("{prefix}{name}/") };
                for j in *first..*first + *count {
                    self.walk(j as usize, inner.clone());
                }
            }
            Entry::File { name, .. } => {
                self.index.insert(format!("{prefix}{name}"), i);
            }
        }
    }

    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        let i = *self.index.get(path).with_context(|| format!("{path} is not in the archive"))?;
        let Entry::File { offset, size, unpacked, encrypted, .. } = self.entries[i] else { unreachable!() };
        self.file.seek(SeekFrom::Start(offset * 512))?;
        let mut data = vec![0u8; if size != 0 { size } else { unpacked } as usize];
        self.file.read_exact(&mut data)?;
        if encrypted {
            aes_decrypt(&self.key, &mut data);
        }
        if size == 0 {
            return Ok(data);
        }
        let mut out = Vec::with_capacity(unpacked as usize);
        flate2::read::DeflateDecoder::new(&data[..]).read_to_end(&mut out)?;
        Ok(out)
    }
}

/// The engine's name hash (Jenkins one-at-a-time, lower case).
pub fn joaat(text: &str) -> u32 {
    let mut h: u32 = 0;
    for c in text.to_lowercase().bytes() {
        h = h.wrapping_add(c as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h.wrapping_add(h << 15)
}

/// A single-channel AWC's waves: name hash (29 bits) → (samples, packed data).
pub fn awc_waves(data: &[u8]) -> Result<HashMap<u32, (u32, &[u8])>> {
    let u32_at = |at: usize| -> Result<u32> { Ok(u32::from_le_bytes(data.get(at..at + 4).context("AWC")?.try_into()?)) };
    let flags = u16::from_le_bytes(data.get(6..8).context("AWC")?.try_into()?);
    let count = u32_at(8)? as usize;
    let mut at = 16 + if flags & 1 != 0 { count * 2 } else { 0 };
    let infos: Vec<u32> = (0..count).map(|i| u32_at(at + 4 * i)).collect::<Result<_>>()?;
    at += 4 * count;
    let mut waves = HashMap::new();
    for info in infos {
        let mut chunks = HashMap::new();
        for _ in 0..info >> 29 {
            let chunk = u64::from_le_bytes(data.get(at..at + 8).context("AWC")?.try_into()?);
            at += 8;
            chunks.insert(chunk >> 56, ((chunk & 0x0FFF_FFFF) as usize, ((chunk >> 28) & 0x0FFF_FFFF) as usize));
        }
        if let (Some(&(format, _)), Some(&(offset, size))) = (chunks.get(&0xFA), chunks.get(&0x55)) {
            let samples = u32_at(format)?;
            waves.insert(info & 0x1FFF_FFFF, (samples, data.get(offset..offset + size).context("AWC")?));
        }
    }
    Ok(waves)
}

/// The game's packed Opus ([length byte][packet] blocks, mono, 48 kHz) as
/// samples in -1..1, `samples` long.
pub fn decode_opus(packed: &[u8], samples: usize) -> Result<Vec<f32>> {
    use audiopus_sys as opus;
    let mut error = 0;
    let decoder = unsafe { opus::opus_decoder_create(48000, 1, &mut error) };
    if decoder.is_null() {
        bail!("Opus decoder: error {error}");
    }
    let mut pcm = [0i16; 5760];
    let mut out = Vec::with_capacity(samples);
    let mut at = 0;
    let result = (|| {
        while at < packed.len() && packed[at] != 0 {
            let length = packed[at] as usize;
            let packet = packed.get(at + 1..at + 1 + length).context("Opus packet")?;
            let n = unsafe { opus::opus_decode(decoder, packet.as_ptr(), length as i32, pcm.as_mut_ptr(), 5760, 0) };
            if n < 0 {
                bail!("Opus decode error {n}");
            }
            out.extend(pcm[..n as usize].iter().map(|&v| v as f32 / 32768.0));
            at += 1 + length;
        }
        Ok(())
    })();
    unsafe { opus::opus_decoder_destroy(decoder) };
    result?;
    out.truncate(samples);
    Ok(out)
}
