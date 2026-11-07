//! The touch feedback (`shin::TouchEffect`, one global instance `*0x710022b8c8`, ctor `0x7100115ae0`): a ripple
//! where the screen is tapped and stars thrown off a drag, drawn over every scene. It has its own
//! [`GestureRecognizer`] (no long press) on the pointer and never takes the input from the screens below.
//! Setting v17 ("visual touch feedback") gates new effects; ones already playing finish.

use glam::{Mat4, Vec2, Vec4, vec2, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_input::Pointer;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};

use super::gesture::{Gesture, GestureRecognizer};
use crate::{random::MinStdRand, render::PreRenderContext};

/// A drag throws stars when it moved this far (canvas px) or this many frames passed since the last ones
const SPAWN_DISTANCE: f32 = 10.0;
const SPAWN_FRAMES: f32 = 9.0;
const STARS_PER_SPAWN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Ripple {
    at: Vec2,
    t: f32,
}

impl Ripple {
    /// Half the quad's size: grows fast, then settles at 80 px
    fn radius(&self) -> f32 {
        80.0 * (1.0 - (1.0 - self.t).powi(3))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Star {
    /// Quadrant of the 2x2 `drag` atlas
    index: u32,
    position: Vec2,
    /// Canvas px per frame
    velocity: Vec2,
    /// In half turns
    rotation: f32,
    spin: f32,
    scale: f32,
    t: f32,
}

impl Star {
    /// Size and alpha both fall off as `(1 - t)^1.6`
    fn fade(&self) -> f32 {
        (1.0 - self.t).powf(1.6)
    }
}

pub struct TouchEffect {
    gestures: GestureRecognizer,
    ripples: Vec<Ripple>,
    stars: Vec<Star>,
    /// Frames since stars were last thrown
    since_spawn: f32,
    random: MinStdRand,
    /// The finished frame, which the ripples refract; rendered only while one plays
    scene: Option<RenderTexture>,
}

impl TouchEffect {
    pub fn new(seed: u32) -> Self {
        Self {
            gestures: GestureRecognizer::new(false),
            ripples: Vec::new(),
            stars: Vec::new(),
            since_spawn: 0.0,
            random: MinStdRand::new(seed),
            scene: None,
        }
    }

    fn uniform(&mut self, from: f32, to: f32) -> f32 {
        from + (to - from) * self.random.next()
    }

    /// `0x7100115d70`, after the scene's update: `frames` is the frame time at 60 fps; `enabled` is setting v17
    pub fn update(&mut self, pointer: Pointer, frames: f32, enabled: bool) {
        self.ripples.retain_mut(|ripple| {
            ripple.t += frames / 30.0;
            ripple.t <= 1.0
        });
        self.stars.retain_mut(|star| {
            // the original moves by the velocity, then slows it by 0.8, once per 60 Hz frame; over `frames` that
            // sums to the velocity times (1 - 0.8^frames) / 0.2, so other frame rates trace the same path
            let decay = 0.8f32.powf(frames);
            star.position += star.velocity * ((1.0 - decay) / (1.0 - 0.8));
            star.velocity *= decay;
            star.rotation += star.spin * frames;
            star.t += frames / 60.0;
            star.t <= 1.0
        });
        self.since_spawn += frames;

        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                // `0x7100116490`
                Gesture::Tap(at) if enabled => self.ripples.push(Ripple { at, t: 0.0 }),
                // `0x7100116600`: thrown from where the move started, when the pointer moves 10 px per 60 Hz frame
                // (at most once a frame, on average) or 9 frames have passed
                Gesture::Drag(from, to)
                    if enabled
                        && ((from.distance(to) >= SPAWN_DISTANCE * frames
                            && self.since_spawn >= 0.75)
                            || self.since_spawn > SPAWN_FRAMES) =>
                {
                    // the remainder carries over, so faster frame rates throw as many stars a second
                    self.since_spawn = (self.since_spawn - 1.0).min(0.0);
                    for _ in 0..STARS_PER_SPAWN {
                        let star = Star {
                            index: ((self.random.next() * 4.0) as u32).min(3),
                            position: from,
                            velocity: vec2(self.uniform(-16.0, 16.0), self.uniform(-16.0, 16.0)),
                            rotation: self.uniform(-1.0, 1.0),
                            spin: self.uniform(-0.05, 0.05),
                            scale: self.uniform(0.5, 0.75),
                            t: 0.0,
                        };
                        self.stars.push(star);
                    }
                }
                _ => {}
            }
        }
    }

    /// Renders the finished frame (`scene`) for the ripples to refract, while any plays
    pub fn capture(&mut self, ctx: &mut PreRenderContext, scene: impl FnOnce(&mut RenderPass)) {
        if self.ripples.is_empty() {
            self.scene = None;
            return;
        }
        let texture = self
            .scene
            .get_or_insert_with(|| ctx.new_render_texture("touch_effect/scene".to_string()));
        let depth_stencil = ctx.depth_stencil;
        let mut pass = ctx.begin_pass(
            texture.as_texture_target(),
            Some(depth_stencil),
            "touch_effect/scene",
        );
        pass.clear(Some(FloatColor4::BLACK.into_unorm()), Some(0), Some(1.0));
        scene(&mut pass);
    }

    /// `0x7100116070`, right after the scene is drawn: the ripples, then the stars (`drag` from `/systex.txa`)
    pub fn render(&self, pass: &mut RenderPass, drag: Option<&GpuTexture>) {
        let projection = super::projection();
        if let Some(scene) = &self.scene {
            for ripple in &self.ripples {
                let r = ripple.radius();
                let vertex = |x: f32, y: f32| {
                    let position = ripple.at + vec2(x, y) * r;
                    PosTexVertex {
                        position,
                        texture_position: position / vec2(1920.0, 1080.0),
                    }
                };
                let vertices = [
                    vertex(-1.0, -1.0),
                    vertex(1.0, -1.0),
                    vertex(-1.0, 1.0),
                    vertex(1.0, 1.0),
                ];
                // blend 5: premultiplied
                let builder = RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::LayerPremultiplied1);
                pass.run(builder.build(
                    RenderProgramWithArguments::TapEffect {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        scene: scene.as_texture_source(),
                        transform: projection,
                        params: Vec4::new(ripple.at.x, ripple.at.y, r, ripple.t),
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
        let Some(drag) = drag else { return };
        // additive: (SrcAlpha, One)
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
        for star in &self.stars {
            let fade = star.fade();
            let size = star.scale * fade;
            let transform = projection
                * Mat4::from_translation(star.position.extend(0.0))
                * Mat4::from_rotation_z(star.rotation * std::f32::consts::PI)
                * Mat4::from_scale(vec3(size, size, 1.0));
            let u = (star.index & 1) as f32 * 0.5;
            let v = ((star.index >> 1) & 1) as f32 * 0.5;
            super::draw_texture(
                pass,
                drag.as_source(),
                transform,
                [-40.0, -40.0, 40.0, 40.0],
                [u, v, u + 0.5, v + 0.5],
                FloatColor4::WHITE.with_alpha(fade),
                builder,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, held: bool) -> Pointer {
        Pointer {
            position: Some(Vec2::new(x, y)),
            held,
        }
    }

    #[test]
    fn a_tap_ripples_for_thirty_frames() {
        let mut effect = TouchEffect::new(1);
        effect.update(at(100.0, 200.0, true), 1.0, true);
        effect.update(at(100.0, 200.0, false), 1.0, true);
        assert_eq!(
            effect.ripples,
            [Ripple {
                at: Vec2::new(100.0, 200.0),
                t: 0.0
            }]
        );
        effect.update(at(100.0, 200.0, false), 15.0, true);
        assert_eq!(effect.ripples[0].radius(), 80.0 * (1.0 - 0.125));
        effect.update(at(100.0, 200.0, false), 15.0, true);
        assert_eq!(effect.ripples.len(), 1, "t = 1 is still drawn");
        effect.update(at(100.0, 200.0, false), 1.0, true);
        assert!(effect.ripples.is_empty());
    }

    #[test]
    fn the_setting_stops_new_effects_only() {
        let mut effect = TouchEffect::new(1);
        effect.update(at(0.0, 0.0, true), 1.0, true);
        effect.update(at(0.0, 0.0, false), 1.0, true);
        effect.update(at(0.0, 0.0, true), 1.0, false);
        effect.update(at(0.0, 0.0, false), 1.0, false);
        effect.update(at(0.0, 0.0, true), 1.0, false);
        effect.update(at(0.0, 100.0, true), 1.0, false);
        assert_eq!(effect.ripples.len(), 1);
        assert!(effect.stars.is_empty());
    }

    #[test]
    fn drags_throw_four_stars_by_distance_or_time() {
        let mut effect = TouchEffect::new(1);
        effect.update(at(0.0, 0.0, true), 1.0, true);
        // the first drag starts at the down point, 20 px away: thrown from there
        effect.update(at(0.0, 20.0, true), 1.0, true);
        assert_eq!(effect.stars.len(), 4);
        assert!(
            effect
                .stars
                .iter()
                .all(|s| s.position == Vec2::ZERO && s.index < 4)
        );
        for star in &effect.stars {
            assert!(star.velocity.abs().max_element() <= 16.0);
            assert!((0.5..=0.75).contains(&star.scale));
            assert!(star.spin.abs() <= 0.05 && star.rotation.abs() <= 1.0);
        }
        // a slow drag throws nothing until more than 9 frames have passed
        let mut y = 20.0;
        for _ in 0..9 {
            y += 2.0;
            effect.update(at(0.0, y, true), 1.0, true);
        }
        assert_eq!(effect.stars.len(), 4);
        effect.update(at(0.0, y + 2.0, true), 1.0, true);
        assert_eq!(effect.stars.len(), 8);
        // holding still throws nothing
        for _ in 0..20 {
            effect.update(at(0.0, y + 2.0, true), 1.0, true);
        }
        assert_eq!(effect.stars.len(), 8);
    }

    #[test]
    fn stars_slow_down_and_fade_over_a_second() {
        let mut effect = TouchEffect::new(1);
        effect.stars.push(Star {
            index: 0,
            position: Vec2::ZERO,
            velocity: Vec2::new(10.0, 0.0),
            rotation: 0.0,
            spin: 0.05,
            scale: 0.5,
            t: 0.0,
        });
        effect.update(at(0.0, 0.0, false), 1.0, true);
        let star = effect.stars[0];
        assert_eq!(star.position, Vec2::new(10.0, 0.0));
        assert!((star.velocity.x - 8.0).abs() < 1e-5);
        assert_eq!(star.rotation, 0.05);
        effect.update(at(0.0, 0.0, false), 58.0, true);
        assert_eq!(effect.stars.len(), 1);
        effect.update(at(0.0, 0.0, false), 2.0, true);
        assert!(effect.stars.is_empty());
    }

    #[test]
    fn stars_and_spawns_do_not_depend_on_the_frame_rate() {
        let star = Star {
            index: 0,
            position: Vec2::ZERO,
            velocity: Vec2::new(10.0, 0.0),
            rotation: 0.0,
            spin: 0.05,
            scale: 0.5,
            t: 0.0,
        };
        let mut at_60 = TouchEffect::new(1);
        let mut at_240 = TouchEffect::new(1);
        at_60.stars.push(star);
        at_240.stars.push(star);
        for _ in 0..10 {
            at_60.update(at(0.0, 0.0, false), 1.0, true);
        }
        for _ in 0..40 {
            at_240.update(at(0.0, 0.0, false), 0.25, true);
        }
        let (a, b) = (at_60.stars[0], at_240.stars[0]);
        assert!(a.position.distance(b.position) < 1e-3, "{a:?} {b:?}");
        assert!((a.rotation - b.rotation).abs() < 1e-5);

        // a fast drag throws stars as often a second at 60 and 240 Hz: once a 60 Hz frame
        // (over half a second, before any fades out)
        let count = |frames: f32| {
            let mut effect = TouchEffect::new(1);
            effect.update(at(0.0, 0.0, true), frames, true);
            let mut y = 0.0;
            for _ in 0..(30.0 / frames) as usize {
                y += 20.0 * frames;
                effect.update(at(0.0, y, true), frames, true);
            }
            effect.stars.len()
        };
        let (slow, fast) = (count(1.0), count(0.25));
        assert!(
            slow.abs_diff(fast) <= 4,
            "{slow} stars at 60 Hz, {fast} at 240 Hz"
        );
    }
}
