#import types::{PosTexVertex, EffectUniformParams}

// The mosaic layer effect, re-implemented from the behaviour of the original engine's shader program 0x2a: every
// pixel samples the centre of its block, the block grid being aligned on `centre`.
// params[0] = (block width, block height in uv units, their inverses); params[1] = (centre, unused). The original
// always passes a centre of (0.5, 0.5).

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

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let block_size = params.params[0].xy;
    let blocks_per_unit = params.params[0].zw;
    let centre = params.params[1].xy;

    let block = floor((input.texture_position - centre) * blocks_per_unit) + 0.5;
    return textureSample(source_texture, source_sampler, block * block_size + centre);
}
