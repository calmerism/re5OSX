//! Holds the frame rate as the Mac warms up (Settings › Video › Hold 60 fps),
//! as re5OSX's browser launcher does (re5osx/hold60.js).
//!
//! A fanless laptop slows down as it heats up. Rather than a lower quality
//! for the whole session, the back buffer shrinks 5% at a time below the
//! chosen render scale while frames come slower than the limit, and grows
//! back while they keep it with room to spare. A step up that fails soon
//! after doubles the wait before the next one (still hot).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const STEP: f32 = 0.05;
/// At most this far below the chosen scale.
const LOWEST: f32 = 0.6;
/// Readings are taken every half second: 3 s under the limit steps down, 15 s
/// holding it steps up.
const EVERY: Duration = Duration::from_millis(500);
const DOWN_AFTER: usize = 6;
const UP_AFTER: usize = 30;
/// Readings skipped after a change (the resize and its first frames).
const SETTLE: u32 = 8;
/// Nothing this soon after the game starts (loading runs at its own rate; a
/// step down it causes is undone once the world holds the limit).
const WARM_UP: Duration = Duration::from_secs(90);

pub struct Hold60 {
    /// What the chosen render scale is multiplied by (at most 1).
    factor: f32,
    readings: VecDeque<f32>,
    last_reading: Option<Instant>,
    settle: u32,
    up_wait: usize,
    last_down: Option<Instant>,
    last_up: Option<Instant>,
    since: Option<Instant>,
}

impl Default for Hold60 {
    fn default() -> Self {
        Hold60 {
            factor: 1.0,
            readings: VecDeque::new(),
            last_reading: None,
            settle: 0,
            up_wait: UP_AFTER,
            last_down: None,
            last_up: None,
            since: None,
        }
    }
}

impl Hold60 {
    /// What the chosen render scale is multiplied by now.
    pub fn factor(&self) -> f32 {
        self.factor
    }

    /// Back to the chosen scale (the setting is off, or the game stopped).
    pub fn reset(&mut self) {
        *self = Hold60::default();
    }

    /// Called every turn while the game plays (`paused`: a page of ours is
    /// over it): takes a reading of the last frames (milliseconds between
    /// presents) every half second against `limit` fps. True when the factor
    /// changed.
    pub fn update(&mut self, frame_ms: &[f32], limit: f32, paused: bool) -> bool {
        let now = Instant::now();
        let since = *self.since.get_or_insert(now);
        if paused || now.duration_since(since) < WARM_UP || self.last_reading.is_some_and(|t| now - t < EVERY) {
            return false;
        }
        self.last_reading = Some(now);
        // The last half second's frames.
        let tail = &frame_ms[frame_ms.len().saturating_sub(30)..];
        if tail.is_empty() {
            return false;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return false;
        }
        let fps = 1000.0 * tail.len() as f32 / tail.iter().sum::<f32>().max(1.0);
        self.readings.push_back(fps);
        if self.readings.len() > UP_AFTER {
            self.readings.pop_front();
        }
        let lowest = LOWEST;
        let recent: Vec<f32> = self.readings.iter().rev().take(DOWN_AFTER).copied().collect();
        let average = recent.iter().sum::<f32>() / recent.len() as f32;
        let ago = |t: Option<Instant>| t.map_or(Duration::MAX, |t| now - t);
        let mut next = self.factor;
        if recent.len() == DOWN_AFTER && average < limit * 0.92 && self.factor > lowest + 0.001 {
            next = (self.factor - STEP).max(lowest);
            // Down again soon after a step up: still hot, wait twice as long.
            if ago(self.last_up) < Duration::from_secs(60) {
                self.up_wait = (self.up_wait * 2).min(480);
            }
            self.last_down = Some(now);
        } else {
            let holding = self.up_wait.min(UP_AFTER);
            let waited = ago(self.last_down) > EVERY * self.up_wait as u32;
            if self.readings.len() >= holding
                && self.readings.iter().rev().take(holding).all(|&v| v >= limit * 0.97)
                && waited
                && self.factor < 1.0 - 0.001
            {
                next = (self.factor + STEP).min(1.0);
                // The last step up held a minute: cooler, back to trying soon.
                if ago(self.last_up) > Duration::from_secs(60) + EVERY * self.up_wait as u32 {
                    self.up_wait = UP_AFTER;
                }
                self.last_up = Some(now);
            }
        }
        if (next - self.factor).abs() < 0.001 {
            return false;
        }
        log::info!("hold 60: render scale ×{:.2} -> ×{:.2} ({average:.0} fps)", self.factor, next);
        self.factor = (next * 100.0).round() / 100.0;
        self.readings.clear();
        self.settle = SETTLE;
        true
    }
}
