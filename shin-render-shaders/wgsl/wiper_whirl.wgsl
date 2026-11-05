#import types::{PosTexVertex, WiperDefaultUniformParams}

// Whirl transition (WIPE type 8), re-implemented from the behaviour of the original engine's shader program 0x28:
// the picture turns one full rotation around the screen centre with concentric ripples that peak mid-transition,
// while cross-fading from the old to the new screen.
// params.alpha = (progress, direction +-1, x scale, y scale)

@group(0) @binding(0)
var<uniform> params: WiperDefaultUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;
@group(0) @binding(3)
var target_texture: texture_2d<f32>;
@group(0) @binding(4)
var target_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: PosTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.texture_position = input.texture_position;

    return output;
}

// smoothstep from 0 to 1
fn ease(t: f32) -> f32 {
    let c = clamp(t, 0.0, 1.0);
    return c * c * (3.0 - 2.0 * c);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let progress = params.alpha.x;
    let direction = params.alpha.y;
    let scale = params.alpha.zw;

    let offset = (input.texture_position - vec2<f32>(0.5, 0.5)) * scale;
    let fade = ease(progress);
    // ten ripple periods per unit of distance from the centre, 0.3 turns at most
    let ripple = ease(1.0 - 2.0 * abs(progress - 0.5)) * sin(length(offset) * 31.4159265) * 0.3;
    let angle = (fade * direction + ripple) * 6.28318531;

    let s = sin(angle);
    let c = cos(angle);
    let rotated = vec2<f32>(offset.x * c - offset.y * s, offset.x * s + offset.y * c);
    let uv = rotated / scale + vec2<f32>(0.5, 0.5);

    let sampled_source = textureSample(source_texture, source_sampler, uv);
    let sampled_target = textureSample(target_texture, target_sampler, uv);

    return mix(sampled_source, sampled_target, fade);
}
