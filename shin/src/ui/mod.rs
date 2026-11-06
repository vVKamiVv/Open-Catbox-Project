//! Drawing and timing helpers the menus share, in the original's 1920x1080 top-left space.

use enum_map::EnumMap;
use glam::{Mat4, vec3};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColVertex},
};

use crate::{app::AppAction, layer::user::minigame::Sprite, render::top_left_projection_matrix};

/// The blend most menu draws use (the original's blend 2: straight alpha)
pub fn builder() -> RenderRequestBuilder {
    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1)
}

/// The projection of the menus' space
pub fn projection() -> Mat4 {
    top_left_projection_matrix()
}

/// A flat-coloured rectangle (left, top, right, bottom)
pub fn fill(
    pass: &mut RenderPass,
    transform: Mat4,
    [x0, y0, x1, y1]: [f32; 4],
    color: FloatColor4,
) {
    if color.a <= 0.0 {
        return;
    }
    let color = color.into_unorm();
    let vertex = |x: f32, y: f32| PosColVertex {
        position: vec3(x, y, 0.0),
        color,
    };
    let vertices = [
        vertex(x0, y0),
        vertex(x1, y0),
        vertex(x0, y1),
        vertex(x1, y1),
    ];
    pass.run(builder().build(
        RenderProgramWithArguments::Fill {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            transform,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

/// A texture stretched over a rectangle as three columns and three rows: the corners keep their size, the edges and
/// the centre stretch. `insets` are the corners' sizes (left, right, top, bottom) in pixels, `us` and `vs` the
/// texture coordinates of the four column and row edges.
#[derive(Debug, Clone, Copy)]
pub struct NineSlice {
    pub insets: [f32; 4],
    pub us: [f32; 4],
    pub vs: [f32; 4],
}

impl NineSlice {
    pub fn draw(
        &self,
        pass: &mut RenderPass,
        texture: &GpuTexture,
        transform: Mat4,
        [x0, y0, x1, y1]: [f32; 4],
        color: FloatColor4,
    ) {
        let [left, right, top, bottom] = self.insets;
        let xs = [x0, x0 + left, x1 - right, x1];
        let ys = [y0, y0 + top, y1 - bottom, y1];
        for row in 0..3 {
            for column in 0..3 {
                let mut sprite =
                    Sprite::new(texture, [xs[column], ys[row], xs[column + 1], ys[row + 1]]).uv([
                        self.us[column],
                        self.vs[row],
                        self.us[column + 1],
                        self.vs[row + 1],
                    ]);
                sprite.color = color;
                sprite.draw(pass, builder(), transform);
            }
        }
    }
}

/// Linear interpolation of two colours (`0x7100056770`)
pub fn lerp_color(a: FloatColor4, b: FloatColor4, t: f32) -> FloatColor4 {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    FloatColor4::from_rgba(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b), mix(a.a, b.a))
}

/// Queues a step to `target` over `frames` frames
pub fn tween(tweener: &mut Tweener, target: f32, frames: f32, easing: Easing) {
    tweener.enqueue(
        target,
        shin_core::time::Tween {
            duration: Ticks::from_f32(frames),
            easing,
        },
    );
}

/// Queues a hold of `frames` frames at the value the tweener is heading for (tween flag 0x80 with target 0)
pub fn hold(tweener: &mut Tweener, frames: f32) {
    let value = tweener.target_value();
    tween(tweener, value, frames, Easing::Linear);
}

/// Advances a tweener by `frames` frames
pub fn advance(tweener: &mut Tweener, frames: f32) {
    tweener.update(Ticks::from_f32(frames));
}

/// The pad as a menu reads it (`0x71000c4900`): pressed buttons, and directions with the engine's key repeat
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuInput {
    pub confirm: bool,
    pub cancel: bool,
    pub x: bool,
    pub y: bool,
    pub l: bool,
    pub r: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub held_up: bool,
    pub held_down: bool,
    pub held_left: bool,
    pub held_right: bool,
    /// The right stick's vertical deflection (-1 up, 1 down)
    pub scroll: f32,
    pub pointer: Pointer,
}

impl MenuInput {
    pub fn new(state: &EnumMap<AppAction, ActionState>, pointer: Pointer) -> Self {
        Self {
            confirm: state[AppAction::Enter].is_clicked,
            cancel: state[AppAction::Cancel].is_clicked,
            x: state[AppAction::ButtonX].is_clicked,
            y: state[AppAction::ButtonY].is_clicked,
            l: state[AppAction::AutoToggle].is_clicked,
            r: state[AppAction::SkipToggle].is_clicked,
            up: state[AppAction::AnyUp].is_clicked_or_repeated,
            down: state[AppAction::AnyDown].is_clicked_or_repeated,
            left: state[AppAction::AnyLeft].is_clicked_or_repeated,
            right: state[AppAction::AnyRight].is_clicked_or_repeated,
            held_up: state[AppAction::AnyUp].is_held,
            held_down: state[AppAction::AnyDown].is_held,
            held_left: state[AppAction::AnyLeft].is_held,
            held_right: state[AppAction::AnyRight].is_held,
            scroll: state[AppAction::PageDown].is_held as i32 as f32
                - state[AppAction::PageUp].is_held as i32 as f32,
            pointer,
        }
    }

    /// Nothing pressed: what a task under the top one sees (the pointer's position stays, released)
    pub fn released(pointer: Pointer) -> Self {
        Self {
            pointer: Pointer {
                position: pointer.position,
                held: false,
            },
            ..Default::default()
        }
    }
}
