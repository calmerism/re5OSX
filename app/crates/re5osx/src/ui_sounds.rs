//! The menus' sounds, as the game's pause menu plays them: a move between
//! controls (NAV_UP_DOWN), a choice (SELECT), going back (BACK). They are the
//! game's own waves (RESIDENT.rpf, frontend.awc), mixed as its
//! HUD_FRONTEND_DEFAULT_SOUNDSET does, on a playback stream of their own
//! (SDL mixes it with the game's). re5OSX.app's setup makes them from the
//! player's own copy of the game (`crates/re5osx-launcher/src/setup.rs`):
//! `nav.wav`, `select.wav` and `back.wav` in the assets' `sounds` folder.
//! Without them, none play.

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Nav,
    Select,
    Back,
}

const RATE: u32 = 48_000;
/// Under the game's mix, as its frontend category sits.
const VOLUME: f32 = 0.6;

/// Playing while alive.
pub struct UiSounds {
    playing: Arc<Mutex<Vec<(Sound, usize)>>>,
    _stream: AudioStreamWithCallback<Mix>,
}

impl UiSounds {
    pub fn start(subsystem: &sdl3::AudioSubsystem) -> Result<UiSounds> {
        let dir = crate::paths::assets_dir().join("sounds");
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| anyhow!("{name}: {e}"));
        let waves = [wave(&read("nav.wav")?)?, wave(&read("select.wav")?)?, wave(&read("back.wav")?)?];
        let spec = AudioSpec { freq: Some(RATE as i32), channels: Some(2), format: Some(AudioFormat::f32_sys()) };
        let playing = Arc::new(Mutex::new(Vec::new()));
        let mix = Mix { waves, playing: playing.clone(), buf: Vec::new() };
        let stream = subsystem.open_playback_stream(&spec, mix).map_err(|e| anyhow!("SDL audio: {e}"))?;
        stream.resume().map_err(|e| anyhow!("SDL audio: {e}"))?;
        log::info!("menu sounds: on ({})", subsystem.current_audio_driver());
        Ok(UiSounds { playing, _stream: stream })
    }

    pub fn play(&self, sound: Sound) {
        let mut playing = self.playing.lock().unwrap();
        // A sound again cuts its last one short, as the game's does.
        playing.retain(|(s, _)| *s != sound);
        playing.push((sound, 0));
    }
}

/// Stereo 16-bit PCM at 48 kHz from a WAV file, as f32 frames.
fn wave(bytes: &[u8]) -> Result<Vec<f32>> {
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("not a WAV file");
    }
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let (id, size) = (&bytes[at..at + 4], u32_at(at + 4) as usize);
        if id == b"fmt " && (u16_at(at + 10) != 2 || u32_at(at + 12) != RATE || u16_at(at + 22) != 16) {
            bail!("not 16-bit stereo at {RATE} Hz");
        }
        if id == b"data" {
            let data = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            return Ok(data.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).collect());
        }
        at += 8 + size + (size & 1);
    }
    bail!("no data in the WAV file")
}

/// SDL's audio thread asks for samples: the sounds playing, summed.
struct Mix {
    waves: [Vec<f32>; 3],
    playing: Arc<Mutex<Vec<(Sound, usize)>>>,
    buf: Vec<f32>,
}

impl AudioCallback<f32> for Mix {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        let samples = (requested.max(0) as usize).next_multiple_of(2);
        self.buf.clear();
        self.buf.resize(samples, 0.0);
        let mut playing = self.playing.lock().unwrap();
        for (sound, at) in playing.iter_mut() {
            let wave = &self.waves[*sound as usize];
            let n = (wave.len() - *at).min(samples);
            for (out, s) in self.buf.iter_mut().zip(&wave[*at..*at + n]) {
                *out += s * VOLUME;
            }
            *at += n;
        }
        playing.retain(|(sound, at)| *at < self.waves[*sound as usize].len());
        drop(playing);
        let _ = stream.put_data_f32(&self.buf);
    }
}
