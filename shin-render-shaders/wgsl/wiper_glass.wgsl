#import types::{PosNormTexVertex, WiperDefaultUniformParams}

// Shards of the glass transition (WIPE type 9), re-implemented from the behaviour of the original engine's shader
// program 0x29. The CPU already moved the vertices and rotated the normals into world space; `transform` is the
// camera's view-projection. Normals point into the shard (as in the original data), so the fixed light comes from
// above and towards the camera. params.alpha = (alpha, flash, unused, unused)

@group(0) @binding(0)
var<uniform> params: WiperDefaultUniformParams;

@group(0) @binding(1)
var source_texture: texture_2d<f32>;
@group(0) @binding(2)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
    @location(1) light: f32,
}

@vertex
fn vertex_main(input: PosNormTexVertex) -> VertexOutput {
    var output: VertexOutput;

    output.clip_position = params.transform * vec4<f32>(input.position, 1.0);
    output.texture_position = input.texture_position;
    // 0.25 / sqrt(2): a quarter-strength light along (0, 1, 1)
    output.light = -0.1767767 * (input.normal.y + input.normal.z) + params.alpha.y;

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(source_texture, source_sampler, input.texture_position);
    return vec4<f32>(color.rgb + vec3<f32>(input.light), params.alpha.x);
}
