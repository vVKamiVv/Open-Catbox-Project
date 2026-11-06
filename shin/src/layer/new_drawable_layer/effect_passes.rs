use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Mat4, Vec2, Vec4, vec2, vec3};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder, StencilFunction, StencilOperation,
    StencilPipelineState, StencilState,
    quad_vertices::build_quad_vertices,
    render_texture::RenderTexture,
    shaders::types::{
        buffer::VertexSource,
        vertices::{MaskVertex, PosColTexVertex, PosTexVertex},
    },
    shin_orthographic_projection_matrix,
};

use crate::{
    layer::LayerProperties,
    random::MinStdRand,
    render::{
        PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, centered_projection_matrix,
        top_left_projection_matrix,
    },
};

// Dissolve, re-implemented from the behaviour of the effect chain in the Switch v1.0.3 executable (`0x71000762e0`,
// the part guarded by property 68); see research/docs/engine-internals.md §11. The layer falls apart into 20-pixel
// pieces, starting at the corner opposite the one property 69 selects: an intact triangle anchored in that corner
// shrinks while the pieces beyond it spin away and vanish.

/// Pieces form a grid of 96 x 54 cells of 20 pixels over the 1920 x 1080 canvas
const DISSOLVE_COLUMNS: u32 = 96;
const DISSOLVE_ROWS: u32 = 54;
const DISSOLVE_CELL: f32 = 20.0;
/// A cell's diagonal distance from the far corner, `(column + row) / 148`, runs from 0 to 1
const DISSOLVE_DIAGONAL: f32 = 148.0;
/// The intact triangle's legs at progress 1 (just short of the 3000 pixels of the canvas diagonal sum)
const DISSOLVE_TRIANGLE_SIZE: f32 = 2971.72;

/// Maps the dissolve intensity (property 68 / 1000) to the effect's progress: 1 intact, -2 fully dissolved
fn dissolve_progress(intensity: f32) -> f32 {
    1.0 - 3.0 * intensity.min(1.0)
}

/// The still-intact part: a right triangle anchored in the corner `mode` selects (bit 0: right, bit 1: top), drawn
/// with the plain sprite program while it is between progress -1 and 1
fn dissolve_intact_triangle(progress: f32, mode: u32) -> Option<[PosColTexVertex; 3]> {
    if !(-1.0..1.0).contains(&progress) {
        return None;
    }
    let size = (progress + 1.0) * 0.5 * DISSOLVE_TRIANGLE_SIZE;
    let canvas = VIRTUAL_CANVAS_SIZE_VEC;
    let (x0, x1) = if mode & 1 == 0 {
        (0.0, size)
    } else {
        (canvas.x - size, canvas.x)
    };
    let (y0, y1) = if mode & 2 == 0 {
        (canvas.y - size, canvas.y)
    } else {
        (0.0, size)
    };
    let corner = |x: f32, y: f32| PosColTexVertex {
        position: vec3(x, y, 0.0),
        color: UnormColor::WHITE,
        texture_position: vec2(x, y) / canvas,
    };
    let quad = [
        corner(x0, y0),
        corner(x1, y0),
        corner(x0, y1),
        corner(x1, y1),
    ];
    // the three quad corners that make the triangle around the anchored corner (table at 0x7100170f70)
    let triangle = [[0, 3, 2], [2, 1, 3], [0, 1, 2], [0, 1, 3]][(mode & 3) as usize];
    Some(triangle.map(|i| quad[i]))
}

/// The pieces: every cell whose own progress `t = progress + 2 * distance` is in (0, 1.1] becomes a square with the
/// "breakup" mask. Cells at `t` >= 1 sit still at full size, 32 pixels (overlapping, to hide the triangle's jagged
/// edge); below 1 they shrink with `t` while flying out in a random direction away from the intact corner.
fn dissolve_pieces(progress: f32, mode: u32, seed: u32) -> (Vec<MaskVertex>, Vec<u16>) {
    // the direction pieces fly in, in turns (y points down)
    let base_angle = [0.875, 0.625, 0.125, 0.375][(mode & 3) as usize];
    let canvas = VIRTUAL_CANVAS_SIZE_VEC;
    let mut rng = MinStdRand::new(seed);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for row in 0..DISSOLVE_ROWS {
        for column in 0..DISSOLVE_COLUMNS {
            let far_column = if mode & 1 == 0 {
                DISSOLVE_COLUMNS - column
            } else {
                column
            };
            let far_row = if mode & 2 == 0 {
                row
            } else {
                DISSOLVE_ROWS - row
            };
            let distance = (far_column + far_row) as f32 / DISSOLVE_DIAGONAL;
            // drawn for every cell, whether it shows or not
            let angle_random = rng.next();
            let radius_random = rng.next();

            let t = progress + 2.0 * distance;
            let offset = if t <= 0.0 || t > 1.1 {
                continue;
            } else if t < 1.0 {
                let angle = (base_angle + angle_random * 0.48 - 0.24) * TAU;
                let radius = radius_random * 800.0 + 200.0;
                Vec2::from_angle(angle) * radius * (1.0 - ((1.0 - t) * FRAC_PI_2).cos())
            } else {
                Vec2::ZERO
            };

            let centre = (vec2(column as f32, row as f32) + 0.5) * DISSOLVE_CELL;
            let half = t * 32.0 * 0.5;
            let base = vertices.len() as u16;
            for (corner, mask) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
                .map(|(x, y)| (vec2(x, y), vec2((x + 1.0) / 2.0, (y + 1.0) / 2.0)))
            {
                let position = centre + corner * half;
                vertices.push(MaskVertex {
                    position: position + offset,
                    // the picture patch is taken where the piece started
                    texture_position: position / canvas,
                    mask_position: mask,
                });
            }
            indices.extend([base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }
    }
    (vertices, indices)
}

fn stencil_against_one(
    function: StencilFunction,
    pass_operation: StencilOperation,
) -> DepthStencilState {
    DepthStencilState {
        depth: Default::default(),
        stencil: StencilState {
            pipeline: StencilPipelineState {
                function,
                pass_operation,
                ..Default::default()
            },
            stencil_reference: 1,
        },
    }
}

/// Blur tap weights, as the bit patterns of the original's constants; they sum to 1
const BLUR_WEIGHTS: [u32; 4] = [0x3eceed6b, 0x3ea127be, 0x3e437dbe, 0x3db8afdf];

fn blur_weights() -> Vec4 {
    Vec4::from_array(BLUR_WEIGHTS.map(f32::from_bits))
}

/// Tap offsets of one blur direction in uv units: 0.5, 1.5, 2.5 and 3.5 times the radius in canvas pixels
fn blur_offsets(radius: f32, direction: Vec2) -> [Vec4; 2] {
    let tap = |k: f32| direction * k * radius / VIRTUAL_CANVAS_SIZE_VEC;
    let (a, b, c, d) = (tap(0.5), tap(1.5), tap(2.5), tap(3.5));
    [Vec4::new(a.x, a.y, b.x, b.y), Vec4::new(c.x, c.y, d.x, d.y)]
}

/// Projects the canvas onto the top-left quarter of a target: the original's half-size viewport (`0x71000cf3c0`)
fn quarter_projection_matrix() -> Mat4 {
    let size = VIRTUAL_CANVAS_SIZE_VEC * 2.0;
    shin_orthographic_projection_matrix(0.0, size.x, size.y, 0.0, -1.0, 1.0)
}

/// Copies `from` (its top-left `uv_scale` part) over the canvas with the plain sprite program, opaque
fn sprite_copy(
    context: &mut PreRenderContext,
    from: &RenderTexture,
    to: &mut RenderTexture,
    transform: Mat4,
    uv_scale: f32,
    label: &str,
) {
    let mut pass = context.begin_pass(to.as_texture_target(), None, label);
    let vertices = &build_quad_vertices(|t| PosColTexVertex {
        position: (t * VIRTUAL_CANVAS_SIZE_VEC).extend(0.0),
        color: UnormColor::WHITE,
        texture_position: t * uv_scale,
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData { vertices },
                    sprite: from.as_texture_source(),
                    transform,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

/// Blurs `source` in place (property 66 = `radius` x 1000), as the original does: shrink to half size
/// (`0x7100078360`), blur horizontally then vertically with program `0x2b` at half size, scale back up
/// (`0x7100078580`). Each step renders into `scratch` and swaps the two.
pub fn apply_blur(
    context: &mut PreRenderContext,
    source: &mut RenderTexture,
    scratch: &mut RenderTexture,
    radius: f32,
) {
    let quarter = quarter_projection_matrix();

    sprite_copy(
        context,
        source,
        scratch,
        quarter,
        1.0,
        "NewDrawableLayer/blur_down",
    );
    std::mem::swap(source, scratch);

    for direction in [Vec2::X, Vec2::Y] {
        let mut pass =
            context.begin_pass(scratch.as_texture_target(), None, "NewDrawableLayer/blur");
        let vertices = &build_quad_vertices(|t| PosTexVertex {
            position: t * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: t * 0.5,
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::Blur {
                        vertices: VertexSource::VertexData { vertices },
                        texture: source.as_texture_source(),
                        transform: quarter,
                        offsets: blur_offsets(radius, direction),
                        weights: blur_weights(),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
        drop(pass);
        std::mem::swap(source, scratch);
    }

    sprite_copy(
        context,
        source,
        scratch,
        top_left_projection_matrix(),
        0.5,
        "NewDrawableLayer/blur_up",
    );
    std::mem::swap(source, scratch);
}

/// The system menu's backdrop (`0x7100013af0`): `source` blurred horizontally into `scratch`, then vertically back,
/// at full size with taps 0.75, 2.25, 3.75 and 5.25 px apart (radius 1.5) and the blur's weights
pub fn apply_backdrop_blur(context: &mut PreRenderContext, source: &mut RenderTexture, scratch: &mut RenderTexture) {
    backdrop_blur_pass(context, source, scratch, Vec2::X);
    backdrop_blur_pass(context, scratch, source, Vec2::Y);
}

fn backdrop_blur_pass(
    context: &mut PreRenderContext,
    from: &RenderTexture,
    to: &mut RenderTexture,
    direction: Vec2,
) {
    let vertices = &build_quad_vertices(|t| PosTexVertex {
        position: t * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: t,
    });
    let mut pass = context.begin_pass(to.as_texture_target(), None, "backdrop_blur");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Blur {
                    vertices: VertexSource::VertexData { vertices },
                    texture: from.as_texture_source(),
                    transform: top_left_projection_matrix(),
                    offsets: blur_offsets(1.5, direction),
                    weights: blur_weights(),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

/// Ripple parameters for the shader: (frequency, amplitude, phase, 0). `amplitude` is property 82 in canvas pixels,
/// `wavelength` property 83 (in thousandths of a doubled uv radius), `phase` the running phase
fn ripple_params(amplitude: f32, wavelength: f32, phase: f32) -> Vec4 {
    let wavelength = (wavelength as i32) as f32 * 0.001;
    let frequency = if wavelength != 0.0 {
        1.0 / wavelength
    } else {
        0.0
    };
    Vec4::new(frequency, amplitude / VIRTUAL_CANVAS_SIZE_VEC.x, phase, 0.0)
}

/// Draws `source` rippled into `target` with program `0x2e`
pub fn apply_ripple(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    amplitude: f32,
    wavelength: f32,
    phase: f32,
) {
    let mut pass = context.begin_pass(target.as_texture_target(), None, "NewDrawableLayer/ripple");
    let vertices = &build_quad_vertices(|t| PosTexVertex {
        position: t * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: t,
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Ripple {
                    vertices: VertexSource::VertexData { vertices },
                    texture: source.as_texture_source(),
                    transform: top_left_projection_matrix(),
                    params: ripple_params(amplitude, wavelength, phase),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

/// Program `0x2a`'s parameters for a mosaic of `size` (property 67): blocks of `size + 1` pixels aligned on the
/// picture centre
fn mosaic_params(size: i32) -> [Vec4; 2] {
    let block = (size + 1) as f32 / VIRTUAL_CANVAS_SIZE_VEC;
    [
        Vec4::new(block.x, block.y, 1.0 / block.x, 1.0 / block.y),
        Vec4::new(0.5, 0.5, 0.0, 0.0),
    ]
}

/// Program `0x2d`'s parameters: amplitudes (properties 76 and 79) in uv units, waves per uv from the wavelengths
/// (77 and 80, in pixels; 0 = flat) and the running phases
fn raster_params(amplitude: Vec2, wavelength: Vec2, phase: Vec2) -> [Vec4; 2] {
    // the horizontal wave shifts rows, so its wavelength runs along the height, and the other way round
    let span = Vec2::new(VIRTUAL_CANVAS_SIZE_VEC.y, VIRTUAL_CANVAS_SIZE_VEC.x);
    let frequency = Vec2::select(wavelength.cmpne(Vec2::ZERO), span / wavelength, Vec2::ZERO);
    [
        Vec4::new(
            amplitude.x / VIRTUAL_CANVAS_SIZE_VEC.x,
            amplitude.y / VIRTUAL_CANVAS_SIZE_VEC.y,
            frequency.x,
            frequency.y,
        ),
        phase.extend(0.0).extend(0.0),
    ]
}

/// Draws `source` into `target` with one of the full-screen effect programs (mosaic, raster)
fn apply_full_screen(
    context: &mut PreRenderContext,
    target: &mut RenderTexture,
    label: &str,
    program: RenderProgramWithArguments,
) {
    let mut pass = context.begin_pass(target.as_texture_target(), None, label);
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(program, DrawPrimitive::TrianglesStrip),
    );
}

fn full_screen_quad() -> [PosTexVertex; 4] {
    build_quad_vertices(|t| PosTexVertex {
        position: t * VIRTUAL_CANVAS_SIZE_VEC,
        texture_position: t,
    })
}

/// Draws `source` as a mosaic of `size` into `target` with program `0x2a`
pub fn apply_mosaic(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    size: i32,
) {
    let vertices = &full_screen_quad();
    let program = RenderProgramWithArguments::Mosaic {
        vertices: VertexSource::VertexData { vertices },
        texture: source.as_texture_source(),
        transform: top_left_projection_matrix(),
        params: mosaic_params(size),
    };
    apply_full_screen(context, target, "NewDrawableLayer/mosaic", program);
}

/// Draws `source` waved by the raster effect into `target` with program `0x2d`
pub fn apply_raster(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    amplitude: Vec2,
    wavelength: Vec2,
    phase: Vec2,
) {
    let vertices = &full_screen_quad();
    let program = RenderProgramWithArguments::Raster {
        vertices: VertexSource::VertexData { vertices },
        texture: source.as_texture_source(),
        transform: top_left_projection_matrix(),
        params: raster_params(amplitude, wavelength, phase),
    };
    apply_full_screen(context, target, "NewDrawableLayer/raster", program);
}

/// Draws `source` dissolved by `intensity` (0..1) into `target`
pub fn apply_dissolve(
    context: &mut PreRenderContext,
    source: &RenderTexture,
    target: &mut RenderTexture,
    intensity: f32,
    mode: u32,
    seed: u32,
) {
    let progress = dissolve_progress(intensity);
    let depth_stencil = context.depth_stencil;
    let mask = context.effect_textures.breakup.as_source();
    let transform = top_left_projection_matrix();

    let mut pass = context.begin_pass(
        target.as_texture_target(),
        Some(depth_stencil),
        "NewDrawableLayer/dissolve",
    );
    pass.clear(Some(UnormColor(0)), Some(0), None);

    if let Some(vertices) = dissolve_intact_triangle(progress, mode) {
        // marks the triangle in the stencil, so the pieces stay outside it
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil(stencil_against_one(
                    StencilFunction::Always,
                    StencilOperation::Replace,
                ))
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: source.as_texture_source(),
                        transform,
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }

    let (vertices, indices) = dissolve_pieces(progress, mode, seed);
    if !indices.is_empty() {
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil(stencil_against_one(
                    StencilFunction::NotEqual,
                    StencilOperation::Keep,
                ))
                .color_blend_type(ColorBlendType::LayerPremultiplied1)
                .build(
                    RenderProgramWithArguments::Dissolve {
                        vertices: VertexSource::VertexAndIndexData {
                            vertices: &vertices,
                            indices: &indices,
                        },
                        texture: source.as_texture_source(),
                        mask,
                        transform,
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }
}

pub fn apply_ghosting(
    context: &mut PreRenderContext,
    props: &LayerProperties,
    render_texture_src: &mut RenderTexture,
    render_texture_prev_frame: &RenderTexture,
    alpha: f32,
) {
    let mut pass = context.begin_pass(
        render_texture_src.as_texture_target(),
        None,
        "NewDrawableLayer/ghosting",
    );

    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::LayerPremultiplied1)
            .build(
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &build_quad_vertices(|t| PosTexVertex {
                            position: ((t * 2.0) - 1.0) * VIRTUAL_CANVAS_SIZE_VEC / 2.0,
                            texture_position: t,
                        }),
                    },
                    texture: render_texture_prev_frame.as_texture_source(),
                    transform: centered_projection_matrix() * props.get_ghosting_transform(),
                    color_multiplier: FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).premultiply(),
                    fragment_shader_param: Vec4::ZERO,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intact_triangle_shrinks_into_its_corner() {
        // half way: legs of half the full size, right angle in the bottom-left corner for mode 0
        let triangle = dissolve_intact_triangle(0.0, 0).unwrap();
        let points = triangle.map(|v| v.position.truncate());
        let legs = DISSOLVE_TRIANGLE_SIZE / 2.0;
        assert!(points.contains(&vec2(0.0, 1080.0)));
        assert!(points.contains(&vec2(0.0, 1080.0 - legs)));
        assert!(points.contains(&vec2(legs, 1080.0)));
        // mode 3: top-right corner
        let points = dissolve_intact_triangle(0.0, 3)
            .unwrap()
            .map(|v| v.position.truncate());
        assert!(points.contains(&vec2(1920.0, 0.0)));
        // the texture is sampled where each corner is
        assert!(triangle.iter().all(|v| {
            let (uv, position) = (v.texture_position, v.position);
            uv == position.truncate() / VIRTUAL_CANVAS_SIZE_VEC
        }));
        // gone once fully dissolved, not drawn before the effect starts
        assert!(dissolve_intact_triangle(dissolve_progress(1.0), 0).is_none());
        assert!(dissolve_intact_triangle(dissolve_progress(0.0), 0).is_none());
    }

    #[test]
    fn blur_taps_follow_the_original() {
        assert!((blur_weights().element_sum() - 1.0).abs() < 1e-4);
        // radius 2: taps at 1, 3, 5 and 7 canvas pixels, horizontally
        let [ab, cd] = blur_offsets(2.0, Vec2::X);
        let pixels = Vec4::new(ab.x, ab.z, cd.x, cd.z) * VIRTUAL_CANVAS_SIZE_VEC.x;
        assert!((pixels - Vec4::new(1.0, 3.0, 5.0, 7.0)).abs().max_element() < 1e-4);
        assert_eq!((ab.y, ab.w, cd.y, cd.w), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn mosaic_params_follow_the_original() {
        // size 9: 10-pixel blocks
        let [block, centre] = mosaic_params(9);
        assert!(block.abs_diff_eq(Vec4::new(10.0 / 1920.0, 10.0 / 1080.0, 192.0, 108.0), 1e-4));
        assert_eq!(centre, Vec4::new(0.5, 0.5, 0.0, 0.0));
    }

    #[test]
    fn raster_params_follow_the_original() {
        let [waves, phases] = raster_params(
            Vec2::new(19.2, 10.8),
            Vec2::new(540.0, 0.0),
            Vec2::new(0.25, 0.5),
        );
        // 0x2d: (p76 / 1920, p79 / 1080, 1 / (p77 / 1080), 1 / (p80 / 1920)), a zero wavelength staying 0
        assert!(waves.abs_diff_eq(Vec4::new(0.01, 0.01, 2.0, 0.0), 1e-6));
        assert_eq!(phases, Vec4::new(0.25, 0.5, 0.0, 0.0));
    }

    #[test]
    fn ripple_params_follow_the_original() {
        // the script's ripple: amplitude 40, wavelength 200
        let params = ripple_params(40.0, 200.0, 0.25);
        assert_eq!(params, Vec4::new(5.0, 40.0 / 1920.0, 0.25, 0.0));
        // a zero wavelength leaves the frequency at 0 instead of dividing by zero
        assert_eq!(ripple_params(40.0, 0.0, 0.0).x, 0.0);
    }

    #[test]
    fn fully_dissolved_layers_leave_only_specks() {
        // the diagonal distance slightly exceeds 1 in the corner cells, so (as in the original) a few remain, but
        // they are smaller than a pixel
        let (vertices, _) = dissolve_pieces(dissolve_progress(1.0), 0, 7);
        assert!(vertices.chunks(4).all(|piece| {
            let (a, b) = (piece[0].position, piece[3].position);
            (b - a).max_element() < 1.0
        }));
        assert!(dissolve_intact_triangle(dissolve_progress(1.0), 0).is_none());
    }

    #[test]
    fn pieces_fly_away_from_the_intact_corner() {
        for mode in 0..4 {
            let (vertices, indices) = dissolve_pieces(0.0, mode, 7);
            assert_eq!(indices.len() / 6, vertices.len() / 4);
            // the offset of each piece is its displacement from the picture patch it shows
            let mean_offset = vertices
                .chunks(4)
                .map(|piece| {
                    piece[0].position - piece[0].texture_position * VIRTUAL_CANVAS_SIZE_VEC
                })
                .sum::<Vec2>()
                / (vertices.len() / 4) as f32;
            // bit 0 anchors on the right (pieces go left), bit 1 on the top (pieces go down)
            assert_eq!(
                mean_offset.x > 0.0,
                mode & 1 == 0,
                "mode {mode}: {mean_offset}"
            );
            assert_eq!(
                mean_offset.y < 0.0,
                mode & 2 == 0,
                "mode {mode}: {mean_offset}"
            );
        }
    }

    #[test]
    fn untouched_layers_only_show_the_far_edge_still() {
        // just after the effect starts every piece is still in place; only cells near the far corner are drawn
        let (vertices, _) = dissolve_pieces(dissolve_progress(0.001), 0, 7);
        assert!(!vertices.is_empty());
        assert!(vertices.chunks(4).all(|piece| {
            (piece[0].position - piece[0].texture_position * VIRTUAL_CANVAS_SIZE_VEC).length() < 0.1
        }));
    }
}
