//! Glass transition (`WIPE` type 9), re-implemented from the behaviour of `shin::GlassWiper` in the Switch v1.0.3
//! executable (constructor `0x710006b930`, update `0x710006c980`, render `0x710006cae0`, shader program `0x29`); see
//! `docs/engine-internals.md` §7. The old screen breaks into 168 thin triangular prisms that flash, fly towards the
//! camera, spin and fall while fading out, uncovering the new screen behind them.

use glam::{Mat3, Mat4, Vec2, Vec3, vec2, vec3, vec4};
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, CullFace, DrawPrimitive, LayerBlendType, RenderProgramWithArguments,
    RenderRequestBuilder, SHIN_TO_WGPU_MATRIX,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::TextureSource,
        vertices::{PosNormTexVertex, PosTexVertex},
    },
};

use crate::{
    random::MinStdRand,
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};

const SCREEN: Vec2 = vec2(1920.0, 1080.0);
/// The screen is cut along a jittered 12 x 7 grid, each cell into two triangles
const COLUMNS: usize = 12;
const ROWS: usize = 7;
const CELL: Vec2 = vec2(SCREEN.x / COLUMNS as f32, SCREEN.y / ROWS as f32);
/// The physics always spans this many normalised steps, whatever the transition's duration
const STEPS: f32 = 60.0;
/// Taken off every shard's vertical speed at each whole step
const GRAVITY: f32 = 0.35;
/// Shards are 8 units thick: front face at z = 0, back face at z = -8
const HALF_THICKNESS: f32 = 4.0;
/// The camera sits this far in front of the screen plane, which then fills the view exactly
const CAMERA_DISTANCE: f32 = 1000.0;

#[derive(Debug, Clone)]
struct Shard {
    /// Triangle list around the shard's centre. As in the original data, normals point into the shard.
    vertices: Vec<PosNormTexVertex>,
    position: Vec3,
    rotation: Vec3,
    velocity: Vec3,
    angular_velocity: Vec3,
    /// Extra upward speed per normalised step, lowered by [`GRAVITY`] at each whole step
    vertical_speed: f32,
}

impl Shard {
    /// `corners` are screen pixels (y down); `time_scale` converts per-step speeds to per-tick ones
    fn new(corners: [Vec2; 3], rng: &mut MinStdRand, time_scale: f32) -> Self {
        // centred world space with y up
        let world = |p: Vec2, z: f32| vec3(p.x - SCREEN.x / 2.0, SCREEN.y / 2.0 - p.y, z);
        let front = corners.map(|p| world(p, 0.0));
        let back = corners.map(|p| world(p, -2.0 * HALF_THICKNESS));
        let uv = corners.map(|p| p / SCREEN);

        let centroid = (front[0] + front[1] + front[2]) / 3.0;
        // the pivot is moved by up to a quarter cell either way
        let offset = (vec2(rng.next(), rng.next()) * 0.5 - 0.25) * CELL;
        let centre = vec3(centroid.x + offset.x, centroid.y + offset.y, -HALF_THICKNESS);

        let vertex = |position: Vec3, normal: Vec3, i: usize| PosNormTexVertex {
            position: position - centre,
            normal,
            texture_position: uv[i],
        };
        // The corners come clockwise (seen from the camera), so this fixed order winds every face counter-clockwise
        // seen from outside. Like the original, the order is not fixed up when the jitter folds a triangle over: such
        // shards come out inside-out and culling shows their back.
        let mut vertices = Vec::with_capacity(24);
        vertices.extend([0, 2, 1].map(|i| vertex(front[i], Vec3::NEG_Z, i)));
        vertices.extend([0, 1, 2].map(|i| vertex(back[i], Vec3::Z, i)));
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            let normal = (front[j] - front[i])
                .cross(front[i] - back[i])
                .normalize();
            vertices.extend([
                vertex(front[i], normal, i),
                vertex(front[j], normal, j),
                vertex(back[j], normal, j),
                vertex(front[i], normal, i),
                vertex(back[j], normal, j),
                vertex(back[i], normal, i),
            ]);
        }

        let angular_velocity =
            (vec3(rng.next(), rng.next(), rng.next()) * 0.10471667 - 0.052358337) * time_scale;

        Self {
            vertices,
            position: centre,
            rotation: Vec3::ZERO,
            // outwards from the screen centre and towards the camera
            velocity: vec3(centre.x * 0.4 / 60.0, centre.y * 0.4 / 60.0, 15.0) * time_scale,
            angular_velocity,
            // lower shards are thrown up harder
            vertical_speed: 2.0 * (SCREEN.y - centre.y) / SCREEN.y,
        }
    }

    fn world_vertices(&self) -> impl Iterator<Item = PosNormTexVertex> + '_ {
        // rotate about x, then y, then z, then move (the original's row-vector Rx * Ry * Rz * T)
        let rotation = Mat3::from_rotation_z(self.rotation.z)
            * Mat3::from_rotation_y(self.rotation.y)
            * Mat3::from_rotation_x(self.rotation.x);
        self.vertices.iter().map(move |v| PosNormTexVertex {
            position: rotation * v.position + self.position,
            normal: rotation * v.normal,
            texture_position: v.texture_position,
        })
    }
}

/// Jittered lattice of (ROWS + 1) x (COLUMNS + 1) screen points whose outline stays on the screen edges
fn build_lattice(rng: &mut MinStdRand) -> [[Vec2; COLUMNS + 1]; ROWS + 1] {
    let mut points = [[Vec2::ZERO; COLUMNS + 1]; ROWS + 1];
    for (row, line) in points.iter_mut().enumerate() {
        for (column, point) in line.iter_mut().enumerate() {
            // up to 0.4 cells either way
            let jitter = (vec2(rng.next(), rng.next()) * 0.8 - 0.4) * CELL;
            *point = vec2(column as f32, row as f32) * CELL + jitter;
        }
    }
    for line in points.iter_mut() {
        line[0].x = 0.0;
        line[COLUMNS].x = SCREEN.x;
    }
    for point in points[0].iter_mut() {
        point.y = 0.0;
    }
    for point in points[ROWS].iter_mut() {
        point.y = SCREEN.y;
    }
    points
}

fn build_shards(seed: u32, time_scale: f32) -> Vec<Shard> {
    let mut rng = MinStdRand::new(seed);
    let lattice = build_lattice(&mut rng);
    let mut shards = Vec::with_capacity(ROWS * COLUMNS * 2);
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let top_left = lattice[row][column];
            let top_right = lattice[row][column + 1];
            let bottom_left = lattice[row + 1][column];
            let bottom_right = lattice[row + 1][column + 1];
            for corners in [
                [top_left, top_right, bottom_left],
                [bottom_right, bottom_left, top_right],
            ] {
                shards.push(Shard::new(corners, &mut rng, time_scale));
            }
        }
    }
    shards
}

/// (alpha, flash) of the shards: they flash white when the screen breaks and fade out over the last quarter
fn shard_fade(progress: f32) -> (f32, f32) {
    let alpha = if progress < 0.75 {
        1.0
    } else {
        1.0 - ((progress - 0.75) * 4.0).powi(2)
    };
    let flash = if progress < 0.5 {
        (1.0 - 2.0 * progress) / 2.0
    } else {
        0.0
    };
    (alpha, flash)
}

fn camera_matrix() -> Mat4 {
    // the vertical field of view makes the screen plane fill the view exactly: 2 * atan(540 / 1000)
    let fov = 2.0 * (SCREEN.y / 2.0 / CAMERA_DISTANCE).atan();
    SHIN_TO_WGPU_MATRIX
        * Mat4::perspective_rh_gl(fov, SCREEN.x / SCREEN.y, 10.0, 10000.0)
        * Mat4::from_translation(vec3(0.0, 0.0, -CAMERA_DISTANCE))
}

#[derive(Debug, Clone)]
pub struct GlassWiperImpl {
    shards: Vec<Shard>,
    /// Normalised steps per tick
    time_scale: f32,
    /// Normalised time elapsed, 0 to [`STEPS`]
    step_time: f32,
}

impl GlassWiperImpl {
    fn new(duration: Ticks, seed: u32) -> Self {
        let time_scale = STEPS / duration.as_f32();
        Self {
            shards: build_shards(seed, time_scale),
            time_scale,
            step_time: 0.0,
        }
    }

    fn step(&mut self, delta_ticks: f32) {
        let start = self.step_time;
        let end = start + self.time_scale * delta_ticks;
        for shard in &mut self.shards {
            shard.rotation += shard.angular_velocity * delta_ticks;
            shard.position += shard.velocity * delta_ticks;
            // the vertical speed only changes at whole steps, so integrate it piecewise
            let mut t = start;
            while t < end {
                if end.floor() == t.floor() {
                    shard.position.y += (end - t) * shard.vertical_speed;
                    break;
                }
                let next = t.floor() + 1.0;
                shard.position.y += (next - t) * shard.vertical_speed;
                shard.vertical_speed -= GRAVITY;
                t = next;
            }
        }
        self.step_time = end;
    }
}

impl AdvUpdatable for GlassWiperImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        if context.are_animations_allowed {
            self.step(context.delta_ticks.as_f32());
        }
    }
}

impl TimedWiper for GlassWiperImpl {
    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    ) {
        // the new screen, unchanged, behind the shards
        let quad = &build_quad_vertices(|t| PosTexVertex {
            position: t,
            texture_position: t,
        });
        pass.run(
            render_request_builder
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::WiperDefault {
                        vertices: VertexSource::VertexData { vertices: quad },
                        texture_source,
                        texture_target,
                        transform: normalized_projection_matrix(),
                        alpha: 1.0,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );

        if progress >= 1.0 {
            return;
        }

        let vertices: Vec<_> = self.shards.iter().flat_map(Shard::world_vertices).collect();
        let (alpha, flash) = shard_fade(progress);
        pass.run(
            render_request_builder
                .layer_color_blend(LayerBlendType::Type1)
                // the original's cull state 2 (`0x71000cf6e0`); in shin's y-flipped clip space it hides faces
                // turned away from the camera
                .cull_faces(CullFace::Front)
                .build(
                    RenderProgramWithArguments::WiperGlass {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture_source,
                        transform: camera_matrix(),
                        params: vec4(alpha, flash, 0.0, 0.0),
                    },
                    DrawPrimitive::Triangles,
                ),
        );
    }
}

pub type GlassWiper = TimedWiperWrapper<GlassWiperImpl>;

impl GlassWiper {
    pub fn new(duration: Ticks) -> Self {
        // the original seeds from a system random source; any seed works
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos());
        Self::from_inner(GlassWiperImpl::new(duration, seed), duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_outline_stays_on_the_screen_edges() {
        let lattice = build_lattice(&mut MinStdRand::new(12345));
        for (row, line) in lattice.iter().enumerate() {
            for (column, point) in line.iter().enumerate() {
                let expected = vec2(column as f32, row as f32) * CELL;
                assert!((*point - expected).abs().cmple(CELL * 0.4 + 0.01).all());
            }
            assert_eq!((line[0].x, line[COLUMNS].x), (0.0, SCREEN.x));
        }
        assert!(lattice[0].iter().all(|p| p.y == 0.0));
        assert!(lattice[ROWS].iter().all(|p| p.y == SCREEN.y));
    }

    #[test]
    fn shards_tile_the_screen_and_face_outwards() {
        let shards = build_shards(42, 1.0);
        assert_eq!(shards.len(), 168);
        // signed, so folded-over shards cancel the overlap they cause
        let mut front_area = 0.0;
        for shard in &shards {
            assert_eq!(shard.vertices.len(), 24);
            // the two faces hold all six corners
            let middle = shard.vertices[..6].iter().map(|v| v.position).sum::<Vec3>() / 6.0;
            let outward_windings = shard.vertices.chunks(3).map(|triangle| {
                let [a, b, c] = [0, 1, 2].map(|i| triangle[i].position);
                (b - a).cross(c - a).dot((a + b + c) / 3.0 - middle) > 0.0
            });
            let front = &shard.vertices[..3];
            let [a, b, c] = [0, 1, 2].map(|i| front[i].position);
            let folded = (b - a).cross(c - a).z < 0.0;
            // every face agrees: counter-clockwise from outside, or consistently inside-out when folded
            assert!(outward_windings.into_iter().all(|outward| outward != folded));
            assert!(front.iter().all(|v| { v.normal } == Vec3::NEG_Z));

            // the front face sits on the screen plane
            assert!(front.iter().all(|v| v.position.z + shard.position.z == 0.0));
            front_area += (b - a).cross(c - a).z / 2.0;
        }
        assert!((front_area - SCREEN.x * SCREEN.y).abs() < 1.0, "{front_area}");
    }

    #[test]
    fn vertical_speed_is_integrated_independently_of_frame_rate() {
        let mut whole = GlassWiperImpl::new(Ticks::from_f32(60.0), 7);
        let start = whole.shards[0].vertical_speed;
        let mut halves = whole.clone();
        for _ in 0..60 {
            whole.step(1.0);
        }
        for _ in 0..120 {
            halves.step(0.5);
        }
        for (a, b) in whole.shards.iter().zip(&halves.shards) {
            assert!((a.position - b.position).abs().max_element() < 0.01);
            assert!((a.vertical_speed - b.vertical_speed).abs() < 1e-4);
        }
        // one gravity hit per whole step
        assert!((whole.shards[0].vertical_speed - (start - 60.0 * GRAVITY)).abs() < 1e-3);
    }

    #[test]
    fn fade_follows_the_original_curve() {
        assert_eq!(shard_fade(0.0), (1.0, 0.5));
        assert_eq!(shard_fade(0.25), (1.0, 0.25));
        assert_eq!(shard_fade(0.6), (1.0, 0.0));
        assert_eq!(shard_fade(0.875), (0.75, 0.0));
        assert_eq!(shard_fade(1.0), (0.0, 0.0));
    }

    #[test]
    fn camera_maps_the_screen_plane_onto_the_whole_screen() {
        let corner = camera_matrix() * vec3(960.0, 540.0, 0.0).extend(1.0);
        let ndc = corner.truncate() / corner.w;
        assert!((ndc.x - 1.0).abs() < 1e-4);
        // shin's clip space is y-flipped: the top of the screen lands on -1 like game y = 0 does
        assert!((ndc.y + 1.0).abs() < 1e-4);
    }
}
