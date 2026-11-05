//! Particle layer ([`LayerType::Rain`](shin_core::vm::command::types::LayerType::Rain)): rain, petals and snow.
//!
//! Re-implemented from the behaviour of `shin::RainLayer` in the Switch v1.0.3 executable (vtable `0x71001ddf90`,
//! constructor `0x71000c99a0`, step `0x71000c9df0`, spawn `0x71000ca0b0`, render `0x71000ca730`).
//!
//! The first load parameter picks the particle kind, the other two the distance range. Every particle gets a random
//! distance in that range (biased towards far away) and a perspective scale of `1000 / distance`. Its quad is scaled
//! by `RainSpeed / 1000 * scale` and tilted by `RainDirection`, and it falls at a speed proportional to the scale
//! squared, so near particles are both bigger and much faster. `RainIntensity` sets how many spawn per frame.
//! Particles live in a 1920x1080 top-left space, wrap horizontally and die below the bottom edge.

use std::{
    f32::consts::{FRAC_PI_3, PI},
    fmt::Debug,
    sync::Arc,
};

use glam::{Vec2, vec2};
use shin_core::vm::command::types::LayerProperty;
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{RenderClone, buffer::VertexSource, vertices::PosColTexVertex},
};

use crate::{
    asset::texture_archive::TextureArchive,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

/// All of `/effects.txa`: the derive rejects archives containing textures the struct doesn't declare
#[derive(TextureArchive)]
#[allow(dead_code)] // breakup and focusline belong to other effects
pub struct EffectTextures {
    #[txa(name = "rain")]
    pub rain: GpuTexture,
    #[txa(name = "snow")]
    pub snow: GpuTexture,
    #[txa(name = "goldsnow")]
    pub gold_snow: GpuTexture,
    #[txa(name = "hanabira")]
    pub petal: GpuTexture,
    #[txa(name = "breakup")]
    pub breakup: GpuTexture,
    #[txa(name = "focusline")]
    pub focus_line: GpuTexture,
}

const SCREEN_SIZE: Vec2 = vec2(1920.0, 1080.0);
/// `RainDirection` 1000 tilts the particles by this much (60 degrees)
const MAX_TILT: f32 = FRAC_PI_3;
/// Each particle's tilt varies randomly by up to this fraction of the layer's tilt
const TILT_JITTER: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParticleKind {
    Rain,
    Petal,
    Snow,
    GoldSnow,
}

impl ParticleKind {
    fn from_param(kind: i32) -> Self {
        match kind {
            ..=0 => Self::Rain,
            1 => Self::Petal,
            2 => Self::Snow,
            _ => Self::GoldSnow,
        }
    }

    /// Half size of the untransformed quad
    fn half_size(self) -> Vec2 {
        match self {
            Self::Rain => vec2(4.0, 150.0),
            Self::Petal => vec2(30.0, 30.0),
            Self::Snow | Self::GoldSnow => vec2(15.0, 15.0),
        }
    }

    /// Fall speed per frame before scaling
    fn base_speed(self) -> f32 {
        match self {
            Self::Rain => 400.0,
            Self::Petal => 8.0,
            Self::Snow | Self::GoldSnow => 5.0,
        }
    }

    fn texture(self, textures: &EffectTextures) -> &GpuTexture {
        match self {
            Self::Rain => &textures.rain,
            Self::Petal => &textures.petal,
            Self::Snow => &textures.snow,
            Self::GoldSnow => &textures.gold_snow,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Particle {
    /// Center, in 1920x1080 top-left space; x wraps into [0, 1920)
    position: Vec2,
    /// Per frame
    velocity: Vec2,
    /// Quad scale: `RainSpeed / 1000 * 1000 / distance`
    size: f32,
    /// Quad rotation; equals the fall tilt except for petals, which also spin
    rotation: f32,
    /// Bounding radius of the scaled quad, used for culling and wrapping
    extent: f32,
    /// Normalized distance in [0, 1); larger is farther away and drawn first
    depth: f32,
}

#[derive(Clone, RenderClone)]
pub struct RainLayerImpl {
    textures: Arc<EffectTextures>,
    kind: ParticleKind,
    min_distance: f32,
    max_distance: f32,
    intensity: f32,
    speed: f32,
    direction: f32,
    /// `Prop27`: the simulation only advances while it is non-zero
    running: bool,
    /// Fractional particles carried over between frames
    spawn_accumulator: f32,
    /// Particles that left the screen so far; the pre-warm runs until this is non-zero
    removed_count: u32,
    needs_prewarm: bool,
    pending_frames: f32,
    particles: Vec<Particle>,
    rng_state: u32,
}

impl RainLayerImpl {
    pub fn new(textures: Arc<EffectTextures>, kind: i32, min_distance: i32, max_distance: i32) -> Self {
        let (near, far) = (min_distance.min(max_distance), min_distance.max(max_distance));
        let (near, far) = (near.clamp(100, 10000), far.clamp(100, 10000));
        Self {
            textures,
            kind: ParticleKind::from_param(kind),
            min_distance: near as f32,
            max_distance: far as f32,
            intensity: 1000.0,
            speed: 1000.0,
            direction: 0.0,
            running: true,
            spawn_accumulator: 1.0,
            removed_count: 0,
            needs_prewarm: true,
            pending_frames: 0.0,
            particles: Vec::new(),
            // the original seeds from a system random source; any non-zero state below 2^31 - 1 works
            rng_state: ((near as u32).wrapping_mul(7919) ^ (far as u32)) % 0x7fff_fffe + 1,
        }
    }

    /// `minstd_rand` (multiplier 48271, modulus 2^31 - 1) as the original uses, mapped to [0, 1)
    fn random(&mut self) -> f32 {
        self.rng_state = ((self.rng_state as u64 * 48271) % 0x7fff_ffff) as u32;
        (self.rng_state - 1) as f32 / 2_147_483_648.0
    }

    /// The layer-wide tilt from `RainDirection`, clamped to +-60 degrees
    fn tilt(&self) -> f32 {
        let direction = (self.direction * 0.001).min(1.0);
        if direction < -1.0 { -MAX_TILT } else { direction * MAX_TILT }
    }

    fn spawn(&mut self, count: u32) {
        let tilt = self.tilt();
        let speed = self.speed * 0.001;
        let radius = self.kind.half_size().length();
        let mut spawned = 0;
        while spawned < count {
            let depth = self.random();
            let distance = depth * (self.max_distance - self.min_distance) + self.min_distance;
            // rejection sampling: a particle at `distance` survives with probability distance / max_distance,
            // so far particles outnumber near ones like they would in a perspective volume
            let lateral = self.random() * self.max_distance;
            if lateral > distance {
                continue;
            }
            spawned += 1;

            let scale = 1000.0 / distance;
            let particle_tilt = tilt + tilt * (self.random() * 2.0 - 1.0) * TILT_JITTER;
            let size = speed * scale;
            let extent = size * radius;
            // the quad's local +y axis after rotation; particles fall along it
            let axis = vec2(-particle_tilt.sin(), particle_tilt.cos()) * self.kind.base_speed() * size;
            let rotation = if self.kind == ParticleKind::Petal {
                particle_tilt + (self.random() * 2.0 - 1.0) * PI
            } else {
                particle_tilt
            };
            self.particles.push(Particle {
                // start just above the screen, shifted back along the fall direction
                position: vec2(lateral / distance * SCREEN_SIZE.x - axis.x / axis.y * extent, -extent),
                velocity: axis * scale,
                size,
                rotation,
                extent,
                depth,
            });
        }
        self.particles.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    }

    /// Advances the simulation by `frames` 60 Hz frames
    fn step(&mut self, frames: f32) {
        self.spawn_accumulator +=
            self.intensity * 0.001 * (self.max_distance / 1000.0) * frames;
        let whole = self.spawn_accumulator.trunc();
        self.spawn_accumulator -= whole;
        self.spawn(whole as u32);

        let mut removed = 0;
        self.particles.retain_mut(|p| {
            p.position.y += p.velocity.y * frames;
            if p.position.y < p.extent + SCREEN_SIZE.y {
                p.position.x = (p.position.x + p.velocity.x * frames).rem_euclid(SCREEN_SIZE.x);
                true
            } else {
                removed += 1;
                false
            }
        });
        self.removed_count += removed;
    }
}

pub type RainLayer = NewDrawableLayerWrapper<RainLayerImpl>;

impl RainLayer {
    pub fn new(textures: Arc<EffectTextures>, kind: i32, min_distance: i32, max_distance: i32) -> Self {
        Self::from_inner(RainLayerImpl::new(textures, kind, min_distance, max_distance))
    }
}

impl NewDrawableLayerNeedsSeparatePass for RainLayerImpl {}

impl NewDrawableLayer for RainLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        &DrawableParams {
            color_multiplier,
            blend_type,
            ..
        }: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent || self.particles.is_empty() || color_multiplier.a <= 0.0 {
            return;
        }

        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );

        let color = color_multiplier.into_unorm();
        let half_size = self.kind.half_size();
        let mut vertices = Vec::with_capacity(self.particles.len() * 6);
        let mut push_quad = |p: &Particle, x_offset: f32| {
            let center = p.position + vec2(x_offset, 0.0) - SCREEN_SIZE / 2.0;
            let (sin, cos) = p.rotation.sin_cos();
            let right = vec2(cos, sin) * half_size.x * p.size;
            let down = vec2(-sin, cos) * half_size.y * p.size;
            let vertex = |position: Vec2, texture_position: Vec2| PosColTexVertex {
                position: position.extend(0.0),
                color,
                texture_position,
            };
            let top_left = vertex(center - right - down, vec2(0.0, 0.0));
            let top_right = vertex(center + right - down, vec2(1.0, 0.0));
            let bottom_left = vertex(center - right + down, vec2(0.0, 1.0));
            let bottom_right = vertex(center + right + down, vec2(1.0, 1.0));
            vertices.extend([
                top_left,
                top_right,
                bottom_left,
                top_right,
                bottom_right,
                bottom_left,
            ]);
        };
        for p in &self.particles {
            push_quad(p, 0.0);
            // horizontal wrap: also draw the part that crosses an edge on the other side
            if p.position.x - p.extent < 0.0 {
                push_quad(p, SCREEN_SIZE.x);
            }
            if p.position.x + p.extent > SCREEN_SIZE.x {
                push_quad(p, -SCREEN_SIZE.x);
            }
        }

        pass.push_debug("RainLayer");
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, false)
                .color_blend_type(ColorBlendType::from_regular_layer(blend_type))
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: self.kind.texture(&self.textures).as_source(),
                        transform: transform.compute_final_transform(),
                    },
                    DrawPrimitive::Triangles,
                ),
        );
        pass.pop_debug();
    }

    /// Runs the simulation here rather than in `update` because the wrapper updates properties after the inner layer,
    /// and the original reads them at the start of its own update
    fn update_from_props(&mut self, props: &LayerProperties) {
        self.intensity = props.get_value(LayerProperty::RainIntensity);
        self.speed = props.get_value(LayerProperty::RainSpeed);
        self.direction = props.get_value(LayerProperty::RainDirection);
        self.running = props.get_value(LayerProperty::Prop27) as i32 != 0;

        if self.needs_prewarm {
            self.needs_prewarm = false;
            // the original steps whole frames until the first particle has left the screen, so rain is already
            // falling when the scene appears; the cap stops a zero intensity from looping forever
            for _ in 0..100_000 {
                if self.removed_count != 0 {
                    break;
                }
                self.step(1.0);
            }
        }
        let frames = std::mem::take(&mut self.pending_frames);
        if self.running {
            self.step(frames);
        }
    }
}

impl NewDrawableLayerFastForward for RainLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for RainLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.pending_frames += ctx.delta_ticks.as_f32();
    }
}

impl Debug for RainLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RainLayer")
            .field("kind", &self.kind)
            .field("distance", &(self.min_distance..self.max_distance))
            .field("particles", &self.particles.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_param_is_clamped_like_the_original() {
        assert_eq!(ParticleKind::from_param(-5), ParticleKind::Rain);
        assert_eq!(ParticleKind::from_param(1), ParticleKind::Petal);
        assert_eq!(ParticleKind::from_param(2), ParticleKind::Snow);
        assert_eq!(ParticleKind::from_param(7), ParticleKind::GoldSnow);
    }

    #[test]
    fn minstd_sequence_matches_reference() {
        // std::minstd_rand from seed 1 produces 48271, 182605794, 1291394886
        let mut state = 1u32;
        let mut next = || {
            state = ((state as u64 * 48271) % 0x7fff_ffff) as u32;
            state
        };
        assert_eq!([next(), next(), next()], [48271, 182605794, 1291394886]);
    }

    #[test]
    fn rain_quad_bounding_radius_matches_original_constant() {
        // the executable stores 150.05333 for rain at 0x710015a3a0
        assert!((ParticleKind::Rain.half_size().length() - 150.05333).abs() < 1e-3);
    }
}
