#import types::{MaskVertex, SpriteUniformParams}

// Pieces of the dissolve layer effect, re-implemented from the behaviour of the original engine's shader program
// 0x2f: each piece shows its patch of the (premultiplied) layer picture, cut out by the alpha of the "breakup" mask.

@group(0) @binding(0)
var<uniform> params: SpriteUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;
@group(0) @binding(3)
var mask_texture: texture_2d<f32>;
@group(0) @binding(4)
var mask_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
    @location(1) mask_position: vec2<f32>,
}

@vertex
fn vertex_main(input: MaskVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 0.0, 1.0);
    output.texture_position = input.texture_position;
    output.mask_position = input.mask_position;

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let mask = textureSample(mask_texture, mask_sampler, input.mask_position).a;
    return textureSample(source_texture, source_sampler, input.texture_position) * mask;
}
