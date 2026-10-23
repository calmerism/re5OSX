// HUD radar mask. D3D b5, s2 and t2 keep their original register bindings.
struct MaskConstants {
    values: array<vec4<f32>, 4>,
}
@group(1) @binding(5) var<uniform> mask: MaskConstants;
@group(1) @binding(18) var mask_sampler: sampler;
@group(1) @binding(34) var mask_texture: texture_2d<f32>;

@fragment
fn main(@location(1) tint: vec4<f32>, @location(2) coords: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = coords.xy;
    let half_texel = mask.values[0].xy * 0.5;
    let filtered = (
        textureSample(mask_texture, mask_sampler, uv) +
        textureSample(mask_texture, mask_sampler, uv + half_texel * vec2<f32>(-1.0, -1.0)) +
        textureSample(mask_texture, mask_sampler, uv + half_texel * vec2<f32>( 1.0, -1.0)) +
        textureSample(mask_texture, mask_sampler, uv + half_texel * vec2<f32>(-1.0,  1.0)) +
        textureSample(mask_texture, mask_sampler, uv + half_texel * vec2<f32>( 1.0,  1.0))
    ) * 0.2;
    let coverage = dot(filtered, mask.values[2]);
    let alpha = mix(mask.values[3].x, mask.values[3].y, coverage);
    return tint * alpha;
}
