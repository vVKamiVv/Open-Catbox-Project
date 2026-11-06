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
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};

use crate::{
    adv::hud::OUTLINE,
    app::AppAction,
    layer::{text_block::TextBlock, user::minigame::Sprite},
    render::top_left_projection_matrix,
};

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

/// A text object in `tint` with its outline in `border`, its padded top-left corner at the origin of `transform`
pub fn draw_text(
    pass: &mut RenderPass,
    text: &TextBlock,
    transform: Mat4,
    tint: FloatColor4,
    border: FloatColor4,
) {
    text.render(
        pass,
        builder(),
        transform * Mat4::from_translation(vec3(OUTLINE, OUTLINE, 0.0)),
        tint,
        border,
    );
}

/// A textured quad whose colour goes from `top` to `bottom` (the list edges' fades)
pub fn gradient_sprite(
    pass: &mut RenderPass,
    texture: &GpuTexture,
    transform: Mat4,
    [x0, y0, x1, y1]: [f32; 4],
    [u0, v0, u1, v1]: [f32; 4],
    top: FloatColor4,
    bottom: FloatColor4,
) {
    let (top, bottom) = (top.into_unorm(), bottom.into_unorm());
    let vertex = |x: f32, y: f32, u: f32, v: f32, color| PosColTexVertex {
        position: vec3(x, y, 0.0),
        color,
        texture_position: glam::vec2(u, v),
    };
    let vertices = [
        vertex(x0, y0, u0, v0, top),
        vertex(x1, y0, u1, v0, top),
        vertex(x0, y1, u0, v1, bottom),
        vertex(x1, y1, u1, v1, bottom),
    ];
    pass.run(builder().build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: texture.as_source(),
            transform,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

/// The engine's vertical scroll bar (`0x71000ef580`): a track `view` long at `origin` and a thumb for `scroll` of
/// `height`, from systex's `scroll` (64 x 64: track on the left half, thumb on the right, each in 8 px caps and a
/// stretched middle); nothing when everything fits
pub fn draw_scroll_bar(
    pass: &mut RenderPass,
    texture: &GpuTexture,
    transform: Mat4,
    (x, y): (f32, f32),
    view: f32,
    scroll: f32,
    height: f32,
) {
    let scroll = scroll.trunc();
    if height <= view || scroll < 0.0 || scroll + view > height {
        return;
    }
    let length = view - 16.0;
    // the bar's own coordinates start 8 px down so its caps fit inside the view
    let top = y + 8.0;
    let mut piece = |u0: f32, y0: f32, y1: f32, v0: f32| {
        Sprite::new(texture, [x, top + y0, x + 16.0, top + y1])
            .uv([u0, v0, u0 + 0.5, v0 + 0.25])
            .draw(pass, builder(), transform);
    };
    for (u0, start, end) in [
        (0.0, 0.0, length),
        (
            0.5,
            length * scroll / height,
            length * (scroll + view) / height,
        ),
    ] {
        piece(u0, start - 8.0, start, 0.0);
        piece(u0, start, end, 0.25);
        piece(u0, end, end + 8.0, 0.5);
    }
}

/// Limits drawing to `rect` (1920x1080 top-left space) on a target of `target_size` pixels, or lifts the limit
pub fn clip(pass: &mut RenderPass, rect: Option<[f32; 4]>, target_size: (u32, u32)) {
    let Some([x0, y0, x1, y1]) = rect else {
        pass.set_scissor(None, target_size);
        return;
    };
    let (w, h) = (target_size.0 as f32, target_size.1 as f32);
    let px = |x: f32, full: f32, max: f32| (x / full * max).round().clamp(0.0, max) as u32;
    let (left, top) = (px(x0, 1920.0, w), px(y0, 1080.0, h));
    let (right, bottom) = (px(x1, 1920.0, w), px(y1, 1080.0, h));
    pass.set_scissor(
        Some((
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )),
        target_size,
    );
}

/// A quad of `source` (a render texture) over `rect`, its `uv` part
pub fn draw_texture(
    pass: &mut RenderPass,
    source: shin_render::shaders::types::texture::TextureSource,
    transform: Mat4,
    [x0, y0, x1, y1]: [f32; 4],
    [u0, v0, u1, v1]: [f32; 4],
    builder: RenderRequestBuilder,
) {
    let color = FloatColor4::WHITE.into_unorm();
    let vertex = |x: f32, y: f32, u: f32, v: f32| PosColTexVertex {
        position: vec3(x, y, 0.0),
        color,
        texture_position: glam::vec2(u, v),
    };
    let vertices = [
        vertex(x0, y0, u0, v0),
        vertex(x1, y0, u1, v0),
        vertex(x0, y1, u0, v1),
        vertex(x1, y1, u1, v1),
    ];
    pass.run(builder.build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: source,
            transform,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
