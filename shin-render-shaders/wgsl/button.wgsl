#import types::{PosColTexVertex, EffectUniformParams}

// A sprite tinted toward two colours, re-implemented from the behaviour of the original engine's shader program 5:
// the texture's colour is mixed toward params[0].rgb by params[0].a, then toward params[1].rgb by params[1].a, then
// multiplied by the vertex colour. The quiz uses it for its hover glow and its pick flash.

@group(0) @binding(0)
var<uniform> params: EffectUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) texture_position: vec2<f32>,
}

@vertex
fn vertex_main(input: PosColTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 1.0);
    // unlike the sprite shader, no swizzle: callers pass RGBA (`FloatColor4::into_unorm`)
    output.color = input.color;
    output.texture_position = input.texture_position;

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(source_texture, source_sampler, input.texture_position);
    let first = mix(sampled.rgb, params.params[0].rgb, params.params[0].a);
    let second = mix(first, params.params[1].rgb, params.params[1].a);

    return vec4<f32>(second * input.color.rgb, sampled.a * input.color.a);
}
