#import types::{PosTexVertex, MovieUniformParams}

// A movie with an alpha mask (`MovieTransparencyMode::Transparent`): the top half of each frame is the picture, the
// bottom half its alpha as a grey picture

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
    let picture = vec2<f32>(input.texture_position.x, input.texture_position.y * 0.5);
    let mask = vec2<f32>(input.texture_position.x, 0.5 + input.texture_position.y * 0.5);

    let color = yuv_to_rgb(picture);
    let alpha = clamp(yuv_to_rgb(mask).x, 0.0, 1.0);

    return vec4<f32>(color, alpha);
}
