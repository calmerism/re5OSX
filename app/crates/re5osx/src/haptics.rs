//! Haptic feedback on iPhones (the Taptic Engine): a light tap where a thumb
//! presses a touch control, and the game's rumble. Elsewhere it does nothing.

/// Whether this device can give haptic feedback (and the setting is shown).
pub const SUPPORTED: bool = cfg!(target_os = "ios");

/// One light tap. Call it from the main thread (the event loop's).
pub fn tap() {
    imp::tap();
}

/// The game's rumble request (low and high frequency motors), called every
/// frame: a continuous vibration as strong as the stronger motor, sharper the
/// more the high-frequency one weighs. `(0, 0)` stops it.
pub fn rumble(motors: (u16, u16)) {
    imp::rumble(motors);
}

#[cfg(not(target_os = "ios"))]
mod imp {
    pub fn tap() {}
    pub fn rumble(_: (u16, u16)) {}
}

#[cfg(target_os = "ios")]
mod imp {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
    use objc2::{AnyThread, class, msg_send};
    use objc2_core_haptics::{
        CHHapticDeviceCapability, CHHapticDynamicParameter, CHHapticDynamicParameterIDHapticIntensityControl,
        CHHapticDynamicParameterIDHapticSharpnessControl, CHHapticEngine, CHHapticEvent, CHHapticEventParameter,
        CHHapticEventParameterIDHapticIntensity, CHHapticEventParameterIDHapticSharpness,
        CHHapticEventTypeHapticContinuous, CHHapticPattern, CHHapticPatternPlayer,
    };
    use objc2_foundation::NSArray;

    /// A continuous event lasts at most 30 seconds: a new one before that.
    const EVENT_LENGTH: f64 = 30.0;
    const RENEW_AFTER: Duration = Duration::from_secs(25);
    /// Dynamic parameters change by less than this are not worth sending.
    const STEP: f32 = 0.02;

    type Player = Retained<ProtocolObject<dyn CHHapticPatternPlayer>>;

    /// The haptic engine, and the continuous vibration playing on it.
    struct Rumble {
        engine: Retained<CHHapticEngine>,
        playing: Option<Playing>,
    }

    struct Playing {
        player: Player,
        since: Instant,
        /// What was last sent: intensity, and sharpness around the event's.
        intensity: f32,
        sharpness: f32,
    }

    thread_local! {
        /// `None` until the first rumble, and after an error (it starts over).
        static RUMBLE: RefCell<Option<Rumble>> = const { RefCell::new(None) };
        /// No engine could be started (an iPad has none): when to try again.
        static RETRY: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
        static TAP: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    pub fn tap() {
        TAP.with_borrow_mut(|generator| {
            let g = generator.get_or_insert_with(|| {
                let class: &AnyClass = class!(UIImpactFeedbackGenerator);
                // UIImpactFeedbackStyleMedium = 1.
                // SAFETY: UIKit, from the main thread's event loop.
                unsafe { msg_send![msg_send![class, alloc], initWithStyle: 1isize] }
            });
            // SAFETY: as above.
            unsafe {
                let _: () = msg_send![&**g, impactOccurredWithIntensity: 0.6f64];
                // Keeps the engine awake for the next tap (less latency).
                let _: () = msg_send![&**g, prepare];
            }
        });
    }

    pub fn rumble((low, high): (u16, u16)) {
        let (low, high) = (f32::from(low) / 65535.0, f32::from(high) / 65535.0);
        let intensity = low.max(high);
        // Sharpness is relative to the event's 0.5, in -0.5..=0.5.
        let sharpness = if intensity > 0.0 { high / (low + high) - 0.5 } else { 0.0 };
        RUMBLE.with_borrow_mut(|state| {
            if intensity == 0.0 {
                if let Some(r) = state.as_mut() {
                    if let Some(p) = r.playing.take() {
                        // SAFETY: CoreHaptics calls, from the main thread.
                        let _ = unsafe { p.player.stopAtTime_error(0.0) };
                    }
                }
                return;
            }
            let r = match state {
                Some(r) => r,
                None if RETRY.get().is_some_and(|t| Instant::now() < t) => return,
                None => match start() {
                    Some(r) => state.insert(r),
                    // Not at every frame of a rumble: once a second.
                    None => {
                        RETRY.set(Some(Instant::now() + Duration::from_secs(1)));
                        return;
                    }
                },
            };
            if update(r, intensity, sharpness).is_none() {
                // The engine was stopped (the app in the background, a call):
                // starts over with the next frame.
                *state = None;
            }
        });
    }

    fn start() -> Option<Rumble> {
        // SAFETY: CoreHaptics calls, from the main thread.
        unsafe {
            if !CHHapticEngine::capabilitiesForHardware().supportsHaptics() {
                return None;
            }
            let engine = CHHapticEngine::initAndReturnError(CHHapticEngine::alloc()).ok()?;
            engine.setPlaysHapticsOnly(true);
            engine.startAndReturnError().ok()?;
            Some(Rumble { engine, playing: None })
        }
    }

    /// Plays or renews the vibration and sets its strength and sharpness.
    fn update(r: &mut Rumble, intensity: f32, sharpness: f32) -> Option<()> {
        if r.playing.as_ref().is_some_and(|p| p.since.elapsed() > RENEW_AFTER) {
            let old = r.playing.take()?;
            // SAFETY: CoreHaptics calls, from the main thread.
            let _ = unsafe { old.player.stopAtTime_error(0.0) };
        }
        // SAFETY: CoreHaptics calls, from the main thread.
        unsafe {
            if r.playing.is_none() {
                let player = vibration(&r.engine)?;
                player.startAtTime_error(0.0).ok()?;
                // Starts at zero: the first update below sets its strength.
                r.playing = Some(Playing { player, since: Instant::now(), intensity: -1.0, sharpness: 0.0 });
            }
            let p = r.playing.as_mut()?;
            if (p.intensity - intensity).abs() >= STEP || (p.sharpness - sharpness).abs() >= STEP {
                let dynamic = |id, value| {
                    CHHapticDynamicParameter::initWithParameterID_value_relativeTime(
                        CHHapticDynamicParameter::alloc(),
                        id,
                        value,
                        0.0,
                    )
                };
                let parameters = NSArray::from_retained_slice(&[
                    dynamic(CHHapticDynamicParameterIDHapticIntensityControl, intensity),
                    dynamic(CHHapticDynamicParameterIDHapticSharpnessControl, sharpness),
                ]);
                p.player.sendParameters_atTime_error(&parameters, 0.0).ok()?;
                p.intensity = intensity;
                p.sharpness = sharpness;
            }
        }
        Some(())
    }

    /// A player for one long continuous event, at full intensity and middle
    /// sharpness: the dynamic parameters scale and shift them.
    unsafe fn vibration(engine: &CHHapticEngine) -> Option<Player> {
        unsafe {
            let event_parameter = |id, value| {
                CHHapticEventParameter::initWithParameterID_value(CHHapticEventParameter::alloc(), id, value)
            };
            let parameters = NSArray::from_retained_slice(&[
                event_parameter(CHHapticEventParameterIDHapticIntensity, 1.0),
                event_parameter(CHHapticEventParameterIDHapticSharpness, 0.5),
            ]);
            let event = CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
                CHHapticEvent::alloc(),
                CHHapticEventTypeHapticContinuous,
                &parameters,
                0.0,
                EVENT_LENGTH,
            );
            let pattern = CHHapticPattern::initWithEvents_parameters_error(
                CHHapticPattern::alloc(),
                &NSArray::from_retained_slice(&[event]),
                &NSArray::new(),
            )
            .ok()?;
            engine.createPlayerWithPattern_error(&pattern).ok()
        }
    }
}
