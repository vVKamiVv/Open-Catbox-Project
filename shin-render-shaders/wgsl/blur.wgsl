#import types::{PosTexVertex, BlurUniformParams}

// One pass of the blur layer effect, re-implemented from the behaviour of the original engine's shader program 0x2b:
// eight taps at +-offset for four offsets, each pair weighted by half its weight (the weights sum to 1, no centre tap)

@group(0) @binding(0)
var<uniform> params: BlurUniformParams;

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

fn tap_pair(uv: vec2<f32>, offset: vec2<f32>, weight: f32) -> vec4<f32> {
    let pair = textureSample(source_texture, source_sampler, uv - offset)
        + textureSample(source_texture, source_sampler, uv + offset);
    return 0.5 * weight * pair;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.texture_position;
    return tap_pair(uv, params.offsets[0].xy, params.weights.x)
        + tap_pair(uv, params.offsets[0].zw, params.weights.y)
        + tap_pair(uv, params.offsets[1].xy, params.weights.z)
        + tap_pair(uv, params.offsets[1].zw, params.weights.w);
}
