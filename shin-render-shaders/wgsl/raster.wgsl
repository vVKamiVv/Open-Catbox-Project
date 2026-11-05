#import types::{PosTexVertex, EffectUniformParams}

// The raster layer effect, re-implemented from the behaviour of the original engine's shader program 0x2d: two sine
// waves, one shifting rows sideways (its phase running down the picture), one shifting columns up and down (its
// phase running across).
// params[0] = (horizontal amplitude, vertical amplitude in uv units, horizontal waves per uv of height, vertical
// waves per uv of width); params[1] = (horizontal phase, vertical phase, unused).

@group(0) @binding(0)
var<uniform> params: EffectUniformParams;

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

const TAU: f32 = 6.28318548;

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let amplitude = params.params[0].xy;
    let frequency = params.params[0].zw;
    let phase = params.params[1].xy;
    let uv = input.texture_position;

    let shift = vec2<f32>(
        sin((uv.y * frequency.x + phase.x) * TAU) * amplitude.x,
        sin((uv.x * frequency.y + phase.y) * TAU) * amplitude.y,
    );
    return textureSample(source_texture, source_sampler, uv + shift);
}
