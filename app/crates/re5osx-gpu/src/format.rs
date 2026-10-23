//! D3D11 (DXGI) formats and enums to wgpu.

use wgpu::{
    AddressMode, BlendFactor, BlendOperation, CompareFunction, Features, PrimitiveTopology, StencilOperation,
    TextureFormat as F, VertexFormat as V,
};

pub const BIND_VERTEX: u32 = 0x1;
pub const BIND_INDEX: u32 = 0x2;
pub const BIND_CONSTANT: u32 = 0x4;
pub const BIND_SRV: u32 = 0x8;
pub const BIND_RT: u32 = 0x20;
pub const BIND_DS: u32 = 0x40;
pub const BIND_UAV: u32 = 0x80;
pub const MISC_DRAWINDIRECT_ARGS: u32 = 0x10;

/// How texel data from the engine becomes the texture's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upload {
    Direct,
    /// A8_UNORM: one byte per texel, sampled as (0, 0, 0, a); expanded to RGBA8.
    A8,
    /// R16G16(B16A16)_UNORM on a GPU without 16-bit normalized formats:
    /// kept as half floats, each value converted (the same 0..1).
    Unorm16,
    /// BCn blocks decoded on the CPU (the device has no BC compression);
    /// colour re-encoded as ETC2 where the texture's format is compressed.
    Bc(Bc),
}

/// A BCn format decoded on the CPU (see `bc.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // the formats' names
pub enum Bc {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6h,
    Bc7,
}

#[derive(Clone, Copy, Debug)]
pub struct TexFormat {
    pub format: F,
    /// The sRGB (or linear) twin, for reinterpreting views.
    pub view: Option<F>,
    pub upload: Upload,
}

/// What the device can do, as far as formats and samplers go.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub features: Features,
    /// 16-bit normalized formats, filterable.
    pub norm16: bool,
    /// Indirect draws (missing on some mobile GPUs).
    pub indirect: bool,
    /// 32-bit float textures filterable (not on many mobile GPUs).
    pub float32_filter: bool,
    /// Depth bias clamp (missing on some mobile GPUs).
    pub depth_bias_clamp: bool,
}

impl Caps {
    pub fn new(adapter: &wgpu::Adapter, features: Features) -> Caps {
        let norm16 = features.contains(Features::TEXTURE_FORMAT_16BIT_NORM)
            && [F::Rgba16Unorm, F::Rg16Unorm].iter().all(|f| {
                let ff = adapter.get_texture_format_features(*f);
                ff.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
            });
        let indirect = adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION);
        // As wgpu validates bind groups: by the adapter's own format features
        // when it reports them, else by FLOAT32_FILTERABLE.
        let float32_filter = [F::R32Float, F::Rg32Float, F::Rgba32Float].iter().all(|f| {
            if features.contains(Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
                adapter.get_texture_format_features(*f).flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
            } else {
                features.contains(Features::FLOAT32_FILTERABLE)
            }
        });
        let depth_bias_clamp =
            adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::DEPTH_BIAS_CLAMP);
        Caps { features, norm16, indirect, float32_filter, depth_bias_clamp }
    }

    pub fn border(&self) -> bool {
        self.features.contains(Features::ADDRESS_MODE_CLAMP_TO_BORDER)
    }
}

const fn fmt(format: F) -> TexFormat {
    TexFormat { format, view: None, upload: Upload::Direct }
}

const fn twin(format: F, view: F) -> TexFormat {
    TexFormat { format, view: Some(view), upload: Upload::Direct }
}

/// The texture format for a DXGI format. `depth` = bound as a depth-stencil
/// target (typeless formats pick their depth variant). Typeless formats pick
/// the format the engine views them as.
pub fn texture(dxgi: u32, depth: bool, caps: &Caps) -> TexFormat {
    let tf = native_texture(dxgi, depth, caps);
    if tf.format.is_compressed() && !caps.features.contains(Features::TEXTURE_COMPRESSION_BC) {
        return decoded(tf, caps.features.contains(Features::TEXTURE_COMPRESSION_ETC2));
    }
    tf
}

/// The format BCn data is decoded to: uncompressed, or for colour, ETC2
/// where the GPU has it (re-encoded, a quarter of the memory; see bc.rs).
fn decoded(tf: TexFormat, etc2: bool) -> TexFormat {
    let (bc, format) = match tf.format {
        F::Bc1RgbaUnorm => (Bc::Bc1, F::Rgba8Unorm),
        F::Bc1RgbaUnormSrgb => (Bc::Bc1, F::Rgba8UnormSrgb),
        F::Bc2RgbaUnorm => (Bc::Bc2, F::Rgba8Unorm),
        F::Bc2RgbaUnormSrgb => (Bc::Bc2, F::Rgba8UnormSrgb),
        F::Bc3RgbaUnorm => (Bc::Bc3, F::Rgba8Unorm),
        F::Bc3RgbaUnormSrgb => (Bc::Bc3, F::Rgba8UnormSrgb),
        F::Bc7RgbaUnorm => (Bc::Bc7, F::Rgba8Unorm),
        F::Bc7RgbaUnormSrgb => (Bc::Bc7, F::Rgba8UnormSrgb),
        F::Bc4RUnorm => (Bc::Bc4, F::R8Unorm),
        F::Bc5RgUnorm => (Bc::Bc5, F::Rg8Unorm),
        F::Bc6hRgbUfloat => (Bc::Bc6h, F::Rgba16Float),
        _ => return tf,
    };
    let format = match format {
        F::Rgba8Unorm if etc2 => F::Etc2Rgba8Unorm,
        F::Rgba8UnormSrgb if etc2 => F::Etc2Rgba8UnormSrgb,
        f => f,
    };
    let view = tf.view.map(|v| if v.is_srgb() { format.add_srgb_suffix() } else { format.remove_srgb_suffix() });
    TexFormat { format, view, upload: Upload::Bc(bc) }
}

impl TexFormat {
    /// For BCn data re-encoded as ETC2: decoded only (for sizes ETC2 cannot
    /// take, see `create_texture`).
    pub fn uncompressed(self) -> TexFormat {
        let swap = |f: F| match f {
            F::Etc2Rgba8Unorm => F::Rgba8Unorm,
            F::Etc2Rgba8UnormSrgb => F::Rgba8UnormSrgb,
            f => f,
        };
        TexFormat { format: swap(self.format), view: self.view.map(swap), upload: self.upload }
    }
}

fn native_texture(dxgi: u32, depth: bool, caps: &Caps) -> TexFormat {
    let norm16 = caps.norm16;
    let d32s8 = if caps.features.contains(Features::DEPTH32FLOAT_STENCIL8) {
        F::Depth32FloatStencil8
    } else {
        F::Depth24PlusStencil8
    };
    match dxgi {
        2 => fmt(F::Rgba32Float),
        9 | 10 => fmt(F::Rgba16Float),
        11 if norm16 => fmt(F::Rgba16Unorm),
        11 => TexFormat { format: F::Rgba16Float, view: None, upload: Upload::Unorm16 },
        16 => fmt(F::Rg32Float),
        19 | 20 => fmt(d32s8),
        21 | 22 => fmt(d32s8),
        23 | 24 => fmt(F::Rgb10a2Unorm),
        26 => fmt(F::Rg11b10Ufloat),
        27 | 28 => twin(F::Rgba8Unorm, F::Rgba8UnormSrgb),
        29 => twin(F::Rgba8UnormSrgb, F::Rgba8Unorm),
        31 => fmt(F::Rgba8Snorm),
        33 | 34 => fmt(F::Rg16Float),
        35 if norm16 => fmt(F::Rg16Unorm),
        35 => TexFormat { format: F::Rg16Float, view: None, upload: Upload::Unorm16 },
        39 => fmt(if depth { F::Depth32Float } else { F::R32Float }),
        40 => fmt(F::Depth32Float),
        41 => fmt(F::R32Float),
        44..=47 => fmt(F::Depth24PlusStencil8),
        48 | 49 => fmt(F::Rg8Unorm),
        51 => fmt(F::Rg8Snorm),
        53 => fmt(if depth { F::Depth16Unorm } else { F::R16Float }),
        54 => fmt(F::R16Float),
        55 | 56 => fmt(F::Depth16Unorm),
        60 | 61 => fmt(F::R8Unorm),
        63 => fmt(F::R8Snorm),
        65 => TexFormat { format: F::Rgba8Unorm, view: None, upload: Upload::A8 },
        67 => fmt(F::Rgb9e5Ufloat),
        70 | 71 => twin(F::Bc1RgbaUnorm, F::Bc1RgbaUnormSrgb),
        72 => twin(F::Bc1RgbaUnormSrgb, F::Bc1RgbaUnorm),
        73 | 74 => twin(F::Bc2RgbaUnorm, F::Bc2RgbaUnormSrgb),
        75 => twin(F::Bc2RgbaUnormSrgb, F::Bc2RgbaUnorm),
        76 | 77 => twin(F::Bc3RgbaUnorm, F::Bc3RgbaUnormSrgb),
        78 => twin(F::Bc3RgbaUnormSrgb, F::Bc3RgbaUnorm),
        79 | 80 => fmt(F::Bc4RUnorm),
        82 | 83 => fmt(F::Bc5RgUnorm),
        87 | 88 | 90 => twin(F::Bgra8Unorm, F::Bgra8UnormSrgb),
        91 => twin(F::Bgra8UnormSrgb, F::Bgra8Unorm),
        94 | 95 => fmt(F::Bc6hRgbUfloat),
        97 | 98 => twin(F::Bc7RgbaUnorm, F::Bc7RgbaUnormSrgb),
        99 => twin(F::Bc7RgbaUnormSrgb, F::Bc7RgbaUnorm),
        other => {
            crate::log_once(&format!("fmt{other}"), || format!("no wgpu format for DXGI {other} (using rgba8unorm)"));
            fmt(F::Rgba8Unorm)
        }
    }
}

/// The format of a view of a texture of format `base` through a DXGI view format (0 = the texture's own).
pub fn view(dxgi: u32, base: F, caps: &Caps) -> F {
    if dxgi == 0 {
        return base;
    }
    let f = texture(dxgi, base.is_depth_stencil_format(), caps).format;
    // Only sRGB reinterpretation changes the format of a view.
    if f != base && f.remove_srgb_suffix() == base.remove_srgb_suffix() { f } else { base }
}

/// 32-bit float colour formats: not filterable on many mobile GPUs.
pub fn is_float32(f: F) -> bool {
    matches!(f, F::R32Float | F::Rg32Float | F::Rgba32Float)
}

pub fn is_depth(f: F) -> bool {
    f.is_depth_stencil_format()
}

/// Bytes per texel or block, and the block size.
pub fn block(f: F) -> (u32, u32) {
    let (bw, _) = f.block_dimensions();
    let bytes =
        f.block_copy_size(None).or_else(|| f.block_copy_size(Some(wgpu::TextureAspect::DepthOnly))).unwrap_or(4);
    (bytes, bw)
}

/// A DXGI vertex format: wgpu format and whether the shader reads it as a
/// float/uint/sint.
pub fn vertex(dxgi: u32) -> Option<V> {
    Some(match dxgi {
        2 => V::Float32x4,
        6 => V::Float32x3,
        16 => V::Float32x2,
        41 => V::Float32,
        3 => V::Uint32x4,
        7 => V::Uint32x3,
        17 => V::Uint32x2,
        42 => V::Uint32,
        4 => V::Sint32x4,
        8 => V::Sint32x3,
        18 => V::Sint32x2,
        43 => V::Sint32,
        10 => V::Float16x4,
        34 => V::Float16x2,
        11 => V::Unorm16x4,
        35 => V::Unorm16x2,
        12 => V::Uint16x4,
        36 => V::Uint16x2,
        13 => V::Snorm16x4,
        37 => V::Snorm16x2,
        14 => V::Sint16x4,
        38 => V::Sint16x2,
        28 => V::Unorm8x4,
        30 => V::Uint8x4,
        31 => V::Snorm8x4,
        32 => V::Sint8x4,
        49 => V::Unorm8x2,
        50 => V::Uint8x2,
        51 => V::Snorm8x2,
        52 => V::Sint8x2,
        87 => V::Unorm8x4Bgra,
        24 => V::Unorm10_10_10_2,
        _ => return None,
    })
}

pub fn compare(v: u32) -> Option<CompareFunction> {
    Some(match v {
        1 => CompareFunction::Never,
        2 => CompareFunction::Less,
        3 => CompareFunction::Equal,
        4 => CompareFunction::LessEqual,
        5 => CompareFunction::Greater,
        6 => CompareFunction::NotEqual,
        7 => CompareFunction::GreaterEqual,
        8 => CompareFunction::Always,
        _ => return None,
    })
}

pub fn stencil_op(v: u32) -> StencilOperation {
    match v {
        2 => StencilOperation::Zero,
        3 => StencilOperation::Replace,
        4 => StencilOperation::IncrementClamp,
        5 => StencilOperation::DecrementClamp,
        6 => StencilOperation::Invert,
        7 => StencilOperation::IncrementWrap,
        8 => StencilOperation::DecrementWrap,
        _ => StencilOperation::Keep,
    }
}

pub fn blend_factor(v: u32) -> Option<BlendFactor> {
    Some(match v {
        1 => BlendFactor::Zero,
        2 => BlendFactor::One,
        3 => BlendFactor::Src,
        4 => BlendFactor::OneMinusSrc,
        5 => BlendFactor::SrcAlpha,
        6 => BlendFactor::OneMinusSrcAlpha,
        7 => BlendFactor::DstAlpha,
        8 => BlendFactor::OneMinusDstAlpha,
        9 => BlendFactor::Dst,
        10 => BlendFactor::OneMinusDst,
        11 => BlendFactor::SrcAlphaSaturated,
        14 => BlendFactor::Constant,
        15 => BlendFactor::OneMinusConstant,
        16 => BlendFactor::Src1,
        17 => BlendFactor::OneMinusSrc1,
        18 => BlendFactor::Src1Alpha,
        19 => BlendFactor::OneMinusSrc1Alpha,
        _ => return None,
    })
}

pub fn blend_op(v: u32) -> BlendOperation {
    match v {
        2 => BlendOperation::Subtract,
        3 => BlendOperation::ReverseSubtract,
        4 => BlendOperation::Min,
        5 => BlendOperation::Max,
        _ => BlendOperation::Add,
    }
}

/// D3D11_PRIMITIVE_TOPOLOGY: (topology, is a strip).
pub fn topology(v: u32) -> (PrimitiveTopology, bool) {
    match v {
        1 => (PrimitiveTopology::PointList, false),
        2 => (PrimitiveTopology::LineList, false),
        3 => (PrimitiveTopology::LineStrip, true),
        5 => (PrimitiveTopology::TriangleStrip, true),
        _ => (PrimitiveTopology::TriangleList, false),
    }
}

/// D3D11_TEXTURE_ADDRESS_MODE (BORDER = 4 falls back to clamping without the
/// border feature).
pub fn address(v: u32, border: bool) -> AddressMode {
    match v {
        2 | 5 => AddressMode::MirrorRepeat,
        3 => AddressMode::ClampToEdge,
        4 if border => AddressMode::ClampToBorder,
        4 => AddressMode::ClampToEdge,
        _ => AddressMode::Repeat,
    }
}

/// DXGI_FORMAT_R16_UINT
pub const INDEX_U16: u32 = 57;

/// WebGPU names of the formats that appear in pipeline recipes.
const NAMES: &[(F, &str)] = &[
    (F::Rgba32Float, "rgba32float"),
    (F::Rgba16Float, "rgba16float"),
    (F::Rgba16Unorm, "rgba16unorm"),
    (F::Rg32Float, "rg32float"),
    (F::Rgb10a2Unorm, "rgb10a2unorm"),
    (F::Rg11b10Ufloat, "rg11b10ufloat"),
    (F::Rgba8Unorm, "rgba8unorm"),
    (F::Rgba8UnormSrgb, "rgba8unorm-srgb"),
    (F::Rgba8Snorm, "rgba8snorm"),
    (F::Bgra8Unorm, "bgra8unorm"),
    (F::Bgra8UnormSrgb, "bgra8unorm-srgb"),
    (F::Rg16Float, "rg16float"),
    (F::Rg16Unorm, "rg16unorm"),
    (F::R32Float, "r32float"),
    (F::R32Uint, "r32uint"),
    (F::R32Sint, "r32sint"),
    (F::Rg8Unorm, "rg8unorm"),
    (F::Rg8Snorm, "rg8snorm"),
    (F::R16Float, "r16float"),
    (F::R8Unorm, "r8unorm"),
    (F::R8Snorm, "r8snorm"),
    (F::R8Uint, "r8uint"),
    (F::Rgba8Uint, "rgba8uint"),
    (F::Rgba32Uint, "rgba32uint"),
    (F::Rgb9e5Ufloat, "rgb9e5ufloat"),
    (F::Depth32Float, "depth32float"),
    (F::Depth32FloatStencil8, "depth32float-stencil8"),
    (F::Depth24PlusStencil8, "depth24plus-stencil8"),
    (F::Depth16Unorm, "depth16unorm"),
];

pub fn name(f: F) -> &'static str {
    NAMES.iter().find(|(x, _)| *x == f).map_or("?", |(_, n)| n)
}

pub fn by_name(name: &str) -> Option<F> {
    NAMES.iter().find(|(_, n)| *n == name).map(|(f, _)| *f)
}

/// WGSL storage texel format names.
pub fn storage(name: &str) -> Option<F> {
    by_name(name).or(match name {
        "rgba32sint" => Some(F::Rgba32Sint),
        "rg32uint" => Some(F::Rg32Uint),
        "rgba16uint" => Some(F::Rgba16Uint),
        "rgba8sint" => Some(F::Rgba8Sint),
        _ => None,
    })
}

/// The WGSL name of a storage format (for rewriting declarations).
pub fn wgsl_storage_name(f: F) -> Option<&'static str> {
    NAMES.iter().find(|(x, _)| *x == f).map(|(_, n)| *n).filter(|n| !n.contains('-') && !n.starts_with("depth"))
}
