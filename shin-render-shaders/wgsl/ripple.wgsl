#import types::{PosTexVertex, WiperDefaultUniformParams}

// The ripple layer effect, re-implemented from the behaviour of the original engine's shader program 0x2e: circular
// waves around the picture centre push each pixel's sampling point along its radius.
// params.alpha = (waves per unit of doubled radius, amplitude in uv units, phase, unused). The original also passes a
// centre and a scale, always (0.5, 0.5) and (1, 1).

@group(0) @binding(0)
var<uniform> params: WiperDefaultUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

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

const CENTRE: vec2<f32> = vec2<f32>(0.5, 0.5);

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let frequency = params.alpha.x;
    let amplitude = params.alpha.y;
    let phase = params.alpha.z;

    let offset = input.texture_position - CENTRE;
    let radius = length(offset);
    let wave = sin((radius * 2.0 - phase) * frequency * 6.28318548);
    let uv = offset / radius * (radius + wave * amplitude) + CENTRE;

    return textureSample(source_texture, source_sampler, uv);
}
