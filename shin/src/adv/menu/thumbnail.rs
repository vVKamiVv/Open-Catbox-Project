//! A bookmark's thumbnail (`ThumbMaker`, `0x7100103840` / `0x71001048a0`): the replayed scene's layers are loaded
//! into a screen layer of their own, rendered, and shrunk by halves (the original renders at 960 x 540 and halves
//! to 480 x 270 before drawing it at 288 x 162).
//!
//! shortcut: movie layers are left out of thumbnails (loading one would start its sound), and the scene is rendered
//! at the window's size before being halved twice instead of at 960 x 540.

use std::sync::Arc;

use shin_core::{
    primitives::{color::UnormColor, update::FrameId},
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use super::TaskCx;
use crate::{
    adv::{restore::LayerRestore, vm_state::layers::LayersState},
    layer::{Layer as _, ScreenLayer, render_layer_without_bg, render_params::TransformParams},
    render::{PreRenderContext, top_left_projection_matrix},
    update::{AdvUpdatable, AdvUpdateContext},
};

/// The part of the returned texture that holds the thumbnail: its top-left quarter of a quarter
pub const THUMBNAIL_UV: [f32; 4] = [0.0, 0.0, 0.25, 0.25];

pub struct ThumbnailBuild {
    screen: ScreenLayer,
    restore: LayerRestore,
}

impl ThumbnailBuild {
    /// Starts loading the scene of `model`
    pub fn start(
        cx: &mut TaskCx,
        scenario: &Arc<shin_core::format::scenario::Scenario>,
        model: &LayersState,
    ) -> Self {
        let mut screen = ScreenLayer::new(4);
        let restore = LayerRestore::start(
            cx.pre_render.device,
            cx.asset_server,
            cx.audio,
            scenario,
            model,
            &mut screen,
            false,
        );
        Self { screen, restore }
    }

    /// Once everything has loaded: the scene rendered and shrunk, the thumbnail in [`THUMBNAIL_UV`] of it
    pub fn poll(&mut self, cx: &mut TaskCx) -> Option<RenderTexture> {
        if !self.restore.poll(&mut self.screen) {
            return None;
        }
        let ctx = &mut *cx.pre_render;
        let update = AdvUpdateContext {
            frame_id: FrameId::default(),
            delta_ticks: Ticks::ZERO,
            asset_server: cx.asset_server,
            device: ctx.device,
            queue: ctx.queue,
            are_animations_allowed: true,
        };
        self.screen.update(&update);
        let transform = TransformParams::default();
        self.screen.pre_render(ctx, &transform);

        let mut full = ctx.new_render_texture("thumbnail/scene".to_string());
        let mut half = ctx.new_render_texture("thumbnail/half".to_string());
        let depth_stencil = ctx.depth_stencil;
        {
            let mut pass = ctx.begin_pass(
                full.as_texture_target(),
                Some(depth_stencil),
                "thumbnail/scene",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            render_layer_without_bg(&mut pass, &transform, &self.screen, 0);
        }
        // 1920 x 1080 -> 960 x 540 -> 480 x 270, in the top-left corner of the canvas-sized textures
        shrink(ctx, &full, &mut half, [0.0, 0.0, 1.0, 1.0], 960.0);
        shrink(ctx, &half, &mut full, [0.0, 0.0, 0.5, 0.5], 480.0);
        Some(full)
    }
}

/// Copies the `uv` part of `from` into the top-left corner of `to`, `width` wide in 1920 x 1080 space, filtered
fn shrink(
    ctx: &mut PreRenderContext,
    from: &RenderTexture,
    to: &mut RenderTexture,
    [u0, v0, u1, v1]: [f32; 4],
    width: f32,
) {
    let height = width * 1080.0 / 1920.0;
    let color = UnormColor::WHITE;
    let vertex = |x: f32, y: f32, u: f32, v: f32| PosColTexVertex {
        position: glam::vec3(x, y, 0.0),
        color,
        texture_position: glam::vec2(u, v),
    };
    let vertices = [
        vertex(0.0, 0.0, u0, v0),
        vertex(width, 0.0, u1, v0),
        vertex(0.0, height, u0, v1),
        vertex(width, height, u1, v1),
    ];
    let mut pass = ctx.begin_pass(to.as_texture_target(), None, "thumbnail/shrink");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: from.as_texture_source(),
                    transform: top_left_projection_matrix(),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
