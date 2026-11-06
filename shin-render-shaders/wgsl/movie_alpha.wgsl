#import types::{PosTexVertex, MovieUniformParams}

// A movie with an alpha mask (`MovieTransparencyMode::Transparent`): the top of each frame is the picture, the bottom
// its alpha as a grey picture. As the original (`0x71000bf040`), each part is (h - 7) / 2 rows (integer division)
// with an 8-row gap: the picture at rows 0.., the alpha from row (h - 7) / 2 + 8

@group(0) @binding(0)
var<uniform> params: MovieUniformParams;

@group(0) @binding(1)
var luma_texture: texture_2d<f32>;
@group(0) @binding(2)
var luma_sampler: sampler;
@group(0) @binding(3)
var chroma_texture: texture_2d<f32>;
@group(0) @binding(4)
var chroma_sampler: sampler;

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

fn yuv_to_rgb(position: vec2<f32>) -> vec3<f32> {
    let luma = textureSample(luma_texture, luma_sampler, position).x;
    let chroma = textureSample(chroma_texture, chroma_sampler, position).xy;

    let biased_yuv = vec3<f32>(luma, chroma.x, chroma.y) - params.color_bias.xyz;

    return vec3<f32>(
        dot(biased_yuv, params.color_transform[0].xyz),
        dot(biased_yuv, params.color_transform[1].xyz),
        dot(biased_yuv, params.color_transform[2].xyz),
    );
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // params.split = (part / h, (part + 8) / h, 0, 0)
    let picture = vec2<f32>(input.texture_position.x, input.texture_position.y * params.split.x);
    let mask = vec2<f32>(input.texture_position.x, params.split.y + input.texture_position.y * params.split.x);

    let color = yuv_to_rgb(picture);
    let alpha = clamp(yuv_to_rgb(mask).x, 0.0, 1.0);

    return vec4<f32>(color, alpha) * params.color;
}
