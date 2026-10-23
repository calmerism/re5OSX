//! MetalFX upscaling and frame interpolation (Apple GPUs), on the submit
//! thread, on the same queue, in command buffers of its own (wgpu does not
//! let raw commands share an encoder with its own) submitted between the
//! game's and the present pass's.
//!
//! - The spatial scaler takes the finished back buffer, the game's HUD with
//!   it.
//! - The temporal scaler takes the scene before the HUD, its depth and
//!   motion, drawn at a sub-pixel offset that changes every frame (see
//!   scene.rs), and builds the larger image from several frames: sharper
//!   than one frame scaled up, and antialiased. The HUD is put back over it
//!   by the present pass.
//! - The frame interpolator (macOS and iOS 26) makes a frame between the
//!   temporal scaler's last two from their depth and motion, shown before
//!   the second: twice the frames on screen for the game's work of one, for
//!   half a frame more of latency. The HUD goes over it as over the others
//!   (the newer frame's).
//!
//! An effect is made for one input and one output size, and making one costs
//! milliseconds: while the sizes keep changing (a window being resized, the
//! engine replacing its back buffer) frames take the bicubic filter, and an
//! effect is made once they have held for a moment.

use std::time::{Duration, Instant};

use objc2::Message;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLTexture, MTLTextureUsage};
use objc2_metal_fx::{
    MTLFXFrameInterpolator, MTLFXFrameInterpolatorBase, MTLFXFrameInterpolatorDescriptor, MTLFXSpatialScaler,
    MTLFXSpatialScalerBase, MTLFXSpatialScalerColorProcessingMode, MTLFXSpatialScalerDescriptor, MTLFXTemporalScaler,
    MTLFXTemporalScalerBase, MTLFXTemporalScalerDescriptor,
};
use wgpu::hal::api::Metal;

use crate::present::{Scaled, Upscaled};
use crate::submit::Present;

/// Sizes hold this long before an effect is made for them.
const SETTLE: Duration = Duration::from_millis(250);

/// Which effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spatial,
    Temporal,
    Interpolator,
}

/// What an effect is made for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    kind: Kind,
    input: (u32, u32),
    output: (u32, u32),
    format: MTLPixelFormat,
}

enum Effect {
    Spatial(Retained<ProtocolObject<dyn MTLFXSpatialScaler>>),
    Temporal(Retained<ProtocolObject<dyn MTLFXTemporalScaler>>),
    Interpolator(Retained<ProtocolObject<dyn MTLFXFrameInterpolator>>),
}

/// A texture an effect writes, for Metal and for the present pass.
struct Target {
    raw: Retained<ProtocolObject<dyn MTLTexture>>,
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
}

struct Made {
    key: Key,
    fx: Effect,
    /// The temporal scaler writes its frames in turn into two, so the one
    /// before stays for the interpolator.
    targets: Vec<Target>,
    /// The target written last.
    last: usize,
    /// Frames written since the history was reset.
    frames: u32,
}

/// An effect of each kind, made when first wanted.
#[derive(Default)]
struct Slot {
    made: Option<Made>,
    /// The sizes last asked for, and since when.
    wanted: Option<(Key, Instant)>,
    /// Sizes no effect could be made for.
    failed: Option<Key>,
}

pub struct MetalFx {
    /// None: this GPU has no MetalFX.
    device: Option<Retained<ProtocolObject<dyn MTLDevice>>>,
    temporal: bool,
    interpolator: bool,
    slots: [Slot; 3],
}

// Made on one thread (the application's, with the screen) and used on
// another (the submit thread), never at the same time; Metal's objects may
// be (wgpu's Metal backend relies on the same).
unsafe impl Send for MetalFx {}

impl MetalFx {
    pub fn new(device: &wgpu::Device) -> MetalFx {
        let raw = unsafe { device.as_hal::<Metal>() }.map(|d| d.raw_device().retain());
        // tvOS has MetalFX from 27.1 (the framework is linked weakly there).
        let raw = raw.filter(|_| objc2::available!(tvos = 27.1, ..));
        let device = raw.filter(|d| unsafe { MTLFXSpatialScalerDescriptor::supportsDevice(d) });
        let temporal = device.as_ref().is_some_and(|d| unsafe { MTLFXTemporalScalerDescriptor::supportsDevice(d) });
        // (The class is not there before macOS and iOS 26.)
        let interpolator = temporal
            && objc2::available!(macos = 26.0, ios = 26.0, tvos = 26.0, visionos = 26.0)
            && device.as_ref().is_some_and(|d| unsafe { MTLFXFrameInterpolatorDescriptor::supportsDevice(d) });
        log::info!(
            "gpu: MetalFX {}",
            match (&device, temporal, interpolator) {
                (None, ..) => "not available",
                (Some(_), false, _) => "spatial upscaling available",
                (Some(_), true, false) => "spatial and temporal upscaling available",
                (Some(_), true, true) => "spatial and temporal upscaling and frame interpolation available",
            }
        );
        MetalFx { device, temporal, interpolator, slots: Default::default() }
    }

    pub fn supports(&self, kind: Kind) -> bool {
        self.device.is_some()
            && match kind {
                Kind::Spatial => true,
                Kind::Temporal => self.temporal,
                Kind::Interpolator => self.interpolator,
            }
    }

    /// The frame scaled to `size` by `kind`, when wanted and possible now
    /// (images at `size`, drawn 1:1). Temporal scaling needs the frame's
    /// scene; frames without one take the bicubic filter. With `interpolate`
    /// (temporal scaling only), also a frame between the previous one and
    /// this, `dt` seconds apart.
    pub fn upscale(
        &mut self,
        device: &wgpu::Device,
        src: &Present,
        size: (u32, u32),
        kind: Option<Kind>,
        interpolate: bool,
        dt: f32,
    ) -> Option<Upscaled> {
        let interpolate = interpolate && kind == Some(Kind::Temporal) && self.supports(Kind::Interpolator);
        // The other effects' memory back while they are not used.
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let used = kind.is_some_and(|k| k as usize == i) || (interpolate && i == Kind::Interpolator as usize);
            if !used {
                *slot = Slot::default();
            }
        }
        let kind = kind.filter(|&k| self.supports(k))?;
        let input = match (kind, &src.scene) {
            (Kind::Temporal, Some(scene)) => color_texture(&scene.color)?,
            (Kind::Temporal, None) => return None,
            _ => color_texture(&src.texture)?,
        };
        let key = Key { kind, input: (src.width, src.height), output: size, format: input.pixelFormat() };
        let mtl = self.device.clone()?;
        let mut commands = Vec::new();
        let made = self.slots[kind as usize].get(&mtl, device, key, interpolate, &mut commands)?;
        let previous = made.last;
        if let Effect::Temporal(_) = made.fx {
            made.last = (made.last + 1) % made.targets.len();
        }
        let s = &*made;
        let scene = src.scene.as_ref();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("metalfx") });
        let encoded = autoreleasepool(|_| unsafe {
            encoder.as_hal_mut::<Metal, _, _>(|e| {
                let Some(commands) = e.and_then(|e| e.raw_command_buffer()) else { return false };
                match &s.fx {
                    Effect::Spatial(fx) => {
                        fx.setColorTexture(Some(&input));
                        fx.setOutputTexture(Some(&s.targets[0].raw));
                        fx.setInputContentWidth(key.input.0 as usize);
                        fx.setInputContentHeight(key.input.1 as usize);
                        fx.encodeToCommandBuffer(commands);
                    }
                    Effect::Temporal(fx) => {
                        let scene = scene.unwrap();
                        let (Some(depth), Some(motion)) = (raw(&scene.depth), raw(&scene.motion)) else { return false };
                        fx.setColorTexture(Some(&input));
                        fx.setDepthTexture(Some(&depth));
                        fx.setMotionTexture(Some(&motion));
                        fx.setOutputTexture(Some(&s.targets[s.last].raw));
                        fx.setInputContentWidth(key.input.0 as usize);
                        fx.setInputContentHeight(key.input.1 as usize);
                        // How far the scene was moved (as it is, not the
                        // sample position it makes: measured).
                        fx.setJitterOffsetX(scene.jitter[0]);
                        fx.setJitterOffsetY(scene.jitter[1]);
                        fx.setMotionVectorScaleX(1.0);
                        fx.setMotionVectorScaleY(1.0);
                        fx.setDepthReversed(scene.camera.reversed);
                        // A scaler just made has no history yet either.
                        fx.setReset(scene.reset || s.frames == 0);
                        fx.encodeToCommandBuffer(commands);
                    }
                    Effect::Interpolator(_) => return false,
                }
                true
            })
        });
        if !encoded {
            return None;
        }
        commands.push(encoder.finish());
        let made = self.slots[kind as usize].made.as_mut()?;
        made.frames = if scene.is_some_and(|s| s.reset) { 1 } else { made.frames.saturating_add(1) };
        let (current, frames) = (made.targets[made.last].view.clone(), made.frames);
        let image = if kind == Kind::Temporal { Scaled::Scene(current) } else { Scaled::Frame(current) };
        let between = if interpolate && frames >= 2 {
            let prev = (made.targets[previous].raw.clone(), made.targets[made.last].raw.clone());
            self.interpolate(device, src, size, prev, dt, &mut commands)
        } else {
            None
        };
        Some(Upscaled { commands, image, between })
    }

    /// A frame between the temporal scaler's `frames` (previous, current).
    fn interpolate(
        &mut self,
        device: &wgpu::Device,
        src: &Present,
        size: (u32, u32),
        (prev, current): (Retained<ProtocolObject<dyn MTLTexture>>, Retained<ProtocolObject<dyn MTLTexture>>),
        dt: f32,
        commands: &mut Vec<wgpu::CommandBuffer>,
    ) -> Option<wgpu::TextureView> {
        let scene = src.scene.as_ref()?;
        let key = Key {
            kind: Kind::Interpolator,
            input: (src.width, src.height),
            output: size,
            format: current.pixelFormat(),
        };
        let mtl = self.device.clone()?;
        let made = self.slots[Kind::Interpolator as usize].get(&mtl, device, key, false, commands)?;
        // A new interpolator starts without history, as after a cut (until
        // a frame of it is encoded).
        let fresh = made.frames == 0;
        let Effect::Interpolator(fx) = &made.fx else { return None };
        let (depth, motion) = (raw(&scene.depth)?, raw(&scene.motion)?);
        let output = &made.targets[0];
        let camera = scene.camera;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("metalfx") });
        let encoded = autoreleasepool(|_| unsafe {
            encoder.as_hal_mut::<Metal, _, _>(|e| {
                let Some(raw_commands) = e.and_then(|e| e.raw_command_buffer()) else { return false };
                fx.setColorTexture(Some(&current));
                fx.setPrevColorTexture(Some(&prev));
                fx.setDepthTexture(Some(&depth));
                fx.setMotionTexture(Some(&motion));
                fx.setOutputTexture(Some(&output.raw));
                // The motion is in the scene's pixels, the colours at the output's.
                fx.setMotionVectorScaleX(size.0 as f32 / src.width as f32);
                fx.setMotionVectorScaleY(size.1 as f32 / src.height as f32);
                fx.setDeltaTime(dt);
                fx.setNearPlane(camera.near);
                fx.setFarPlane(camera.far);
                fx.setFieldOfView(camera.fov_y);
                fx.setAspectRatio(camera.aspect);
                fx.setJitterOffsetX(0.0);
                fx.setJitterOffsetY(0.0);
                fx.setDepthReversed(camera.reversed);
                fx.setShouldResetHistory(scene.reset || fresh);
                fx.encodeToCommandBuffer(raw_commands);
                true
            })
        });
        if !encoded {
            return None;
        }
        commands.push(encoder.finish());
        let view = output.view.clone();
        made.frames = 1;
        Some(view)
    }
}

impl Slot {
    /// The effect for `key`, made once its sizes have held (its first
    /// commands into `commands`).
    fn get(
        &mut self,
        mtl: &ProtocolObject<dyn MTLDevice>,
        device: &wgpu::Device,
        key: Key,
        two_targets: bool,
        commands: &mut Vec<wgpu::CommandBuffer>,
    ) -> Option<&mut Made> {
        let targets = if two_targets { 2 } else { 1 };
        if self.made.as_ref().is_none_or(|m| m.key != key || m.targets.len() != targets) {
            match self.wanted {
                Some((k, since)) if k == key => {
                    if since.elapsed() < SETTLE || self.failed == Some(key) {
                        return None;
                    }
                }
                _ => {
                    self.wanted = Some((key, Instant::now()));
                    return None;
                }
            }
            self.made = None;
            let made = autoreleasepool(|_| make(mtl, device, key, targets));
            let Some((made, clear)) = made else {
                log::warn!("gpu: no MetalFX {:?} for {key:?}, bicubic instead", key.kind);
                self.failed = Some(key);
                return None;
            };
            log::info!(
                "gpu: MetalFX {:?} {}x{} -> {}x{}",
                key.kind,
                key.input.0,
                key.input.1,
                key.output.0,
                key.output.1
            );
            self.made = Some(made);
            commands.push(clear);
        }
        self.made.as_mut()
    }
}

/// An effect for `key` writing into `targets` textures, and the commands
/// that clear them once (wgpu would otherwise clear them, as never written,
/// after MetalFX has).
fn make(
    mtl: &ProtocolObject<dyn MTLDevice>,
    device: &wgpu::Device,
    key: Key,
    targets: usize,
) -> Option<(Made, wgpu::CommandBuffer)> {
    let format = match key.format {
        MTLPixelFormat::RGBA8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        MTLPixelFormat::BGRA8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        MTLPixelFormat::RGB10A2Unorm => wgpu::TextureFormat::Rgb10a2Unorm,
        MTLPixelFormat::RGBA16Float => wgpu::TextureFormat::Rgba16Float,
        _ => return None,
    };
    let (fx, needs) = unsafe {
        match key.kind {
            Kind::Spatial => {
                let desc = MTLFXSpatialScalerDescriptor::new();
                desc.setColorTextureFormat(key.format);
                desc.setOutputTextureFormat(key.format);
                desc.setInputWidth(key.input.0 as usize);
                desc.setInputHeight(key.input.1 as usize);
                desc.setOutputWidth(key.output.0 as usize);
                desc.setOutputHeight(key.output.1 as usize);
                // The back buffer holds display-ready (gamma-encoded) colour.
                desc.setColorProcessingMode(MTLFXSpatialScalerColorProcessingMode::Perceptual);
                let fx = desc.newSpatialScalerWithDevice(mtl)?;
                let needs = fx.outputTextureUsage();
                (Effect::Spatial(fx), needs)
            }
            Kind::Temporal => {
                let desc = MTLFXTemporalScalerDescriptor::new();
                desc.setColorTextureFormat(key.format);
                desc.setDepthTextureFormat(MTLPixelFormat::Depth32Float);
                desc.setMotionTextureFormat(MTLPixelFormat::RG16Float);
                desc.setOutputTextureFormat(key.format);
                desc.setInputWidth(key.input.0 as usize);
                desc.setInputHeight(key.input.1 as usize);
                desc.setOutputWidth(key.output.0 as usize);
                desc.setOutputHeight(key.output.1 as usize);
                let fx = desc.newTemporalScalerWithDevice(mtl)?;
                let needs = fx.outputTextureUsage();
                (Effect::Temporal(fx), needs)
            }
            Kind::Interpolator => {
                let desc = MTLFXFrameInterpolatorDescriptor::new();
                desc.setColorTextureFormat(key.format);
                desc.setOutputTextureFormat(key.format);
                desc.setDepthTextureFormat(MTLPixelFormat::Depth32Float);
                desc.setMotionTextureFormat(MTLPixelFormat::RG16Float);
                desc.setInputWidth(key.input.0 as usize);
                desc.setInputHeight(key.input.1 as usize);
                desc.setOutputWidth(key.output.0 as usize);
                desc.setOutputHeight(key.output.1 as usize);
                let fx = desc.newFrameInterpolatorWithDevice(mtl)?;
                let needs = fx.outputTextureUsage();
                (Effect::Interpolator(fx), needs)
            }
        }
    };
    // What MetalFX needs of its outputs (render target, shader writes); the
    // present pass samples them, the interpolator reads them.
    // (Copied out by the tests.)
    let mut usage =
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    if needs.contains(MTLTextureUsage::ShaderWrite) {
        usage |= wgpu::TextureUsages::STORAGE_BINDING;
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("metalfx clear") });
    let mut made = Vec::new();
    for _ in 0..targets {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("metalfx output"),
            size: wgpu::Extent3d { width: key.output.0, height: key.output.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            log::warn!("gpu: MetalFX output texture: {e}");
            return None;
        }
        let view = texture.create_view(&Default::default());
        drop(crate::present::begin_pass(&mut encoder, &view, wgpu::Color::BLACK));
        made.push(Target { raw: raw(&texture)?, view, _texture: texture });
    }
    Some((Made { key, fx, targets: made, last: 0, frames: 0 }, encoder.finish()))
}

/// A texture's Metal texture.
fn raw(texture: &wgpu::Texture) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    unsafe { texture.as_hal::<Metal>() }.map(|t| t.raw_handle().retain())
}

/// A back buffer as MetalFX reads it: its stored values (an sRGB format
/// holds encoded colour, which is what the perceptual mode expects).
fn color_texture(texture: &wgpu::Texture) -> Option<Retained<ProtocolObject<dyn MTLTexture>>> {
    let raw = raw(texture)?;
    let plain = match raw.pixelFormat() {
        MTLPixelFormat::RGBA8Unorm_sRGB => MTLPixelFormat::RGBA8Unorm,
        MTLPixelFormat::BGRA8Unorm_sRGB => MTLPixelFormat::BGRA8Unorm,
        _ => return Some(raw),
    };
    raw.newTextureViewWithPixelFormat(plain)
}
