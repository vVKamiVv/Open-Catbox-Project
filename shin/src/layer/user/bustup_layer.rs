use std::{fmt::Debug, hash::BuildHasher, sync::Arc};

use glam::{Mat4, Vec3, vec3};
use shin_core::vm::command::types::LayerProperty;
use shin_render::{
    PassKind, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone,
        texture::{DepthStencilTarget, TextureTarget},
    },
};

use crate::{
    asset::bustup::Bustup,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::picture_layer::{PictureBlockParams, PictureBlockPassKind},
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

/// How long a blink lasts, in ticks (`0x7100036a00`)
const BLINK_TICKS: f32 = 20.0;

#[derive(Clone, RenderClone)]
pub struct BustupLayerImpl {
    bustup: Arc<Bustup>,
    label: String,
    mouth_state: u32,
    eyes_state: u32,
    /// The character whose voices move this bustup's mouth
    lipsync_character_id: u16,
    /// Property 27: the mouth and the blinking only animate while it is non-zero
    animate: bool,
    /// The eye frames of a blink: 0, 1, ... up to the last eye block and back down (`0x7100035350`)
    blink_frames: Vec<u32>,
    /// Ticks since the last blink, and the wait before the next one
    blink_timer: f32,
    blink_wait: f32,
    /// `minstd_rand` state for the blink waits
    rng_state: u32,
}

impl BustupLayerImpl {
    pub fn new(bustup: Arc<Bustup>, bustup_name: Option<String>, lipsync_character_id: u16) -> Self {
        let eye_count = bustup.eye_blocks.len() as u32;
        let blink_frames = (0..eye_count).chain((0..eye_count.saturating_sub(1)).rev()).collect();
        // the original seeds each bustup from the engine's random source
        let seed = std::collections::hash_map::RandomState::new().hash_one(0u8) as u32 % 0x7fff_fffe + 1;
        let mut layer = Self {
            bustup,
            label: bustup_name.unwrap_or_else(|| "unnamed".to_string()),
            mouth_state: 0,
            eyes_state: 0,
            lipsync_character_id,
            animate: true,
            blink_frames,
            blink_timer: 0.0,
            blink_wait: 0.0,
            rng_state: seed,
        };
        layer.next_blink();
        layer
    }

    /// Draws the wait before the next blink: `(1 - r^2) * 240 + 80` ticks (`0x710015a640`, `0x710015aa48`)
    fn next_blink(&mut self) {
        self.rng_state = ((self.rng_state as u64 * 48271) % 0x7fff_ffff) as u32;
        let r = (self.rng_state - 1) as f32 / 2147483648.0;
        self.blink_wait = (1.0 - r * r) * 240.0 + 80.0;
        self.blink_timer = 0.0;
    }

    #[tracing::instrument(skip_all)]
    fn render_impl(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
    ) {
        let translation = Mat4::from_translation(vec3(
            -self.bustup.origin_x as f32,
            -self.bustup.origin_y as f32,
            0.0,
        ));
        // NOTE: this scale is a ¿device-specific? scale (which is 1.0 on switch)
        let scale = Mat4::from_scale(Vec3::ONE);

        let transform = transform * scale * translation;

        pass.push_debug(&format!(
            "BustupLayer[{}]/{}",
            &self.label,
            match params.pass_kind {
                PictureBlockPassKind::OpaqueOnly => "opaque",
                PictureBlockPassKind::TransparentOnly => "transparent",
                PictureBlockPassKind::OpaqueAndTransparent => "opaque_and_transparent",
            }
        ));

        pass.push_debug("Base");
        for block in &self.bustup.base_blocks {
            super::picture_layer::render_block(block, pass, builder, params, transform);
        }
        pass.pop_debug();

        if let Some(block) = &self.bustup.face1 {
            pass.push_debug("Face1");
            super::picture_layer::render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if let Some(block) = &self.bustup.face2 {
            pass.push_debug("Face2");
            super::picture_layer::render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if !self.bustup.mouth_blocks.is_empty() {
            pass.push_debug("Mouth");
            super::picture_layer::render_block(
                &self.bustup.mouth_blocks[self.mouth_state as usize],
                pass,
                builder,
                params,
                transform,
            );
            pass.pop_debug();
        }

        if !self.bustup.eye_blocks.is_empty() {
            pass.push_debug("Eyes");
            super::picture_layer::render_block(
                &self.bustup.eye_blocks[self.eyes_state as usize],
                pass,
                builder,
                params,
                transform,
            );
            pass.pop_debug();
        }

        pass.pop_debug();
    }
}

pub type BustupLayer = NewDrawableLayerWrapper<BustupLayerImpl>;

impl BustupLayer {
    pub fn new(picture: Arc<Bustup>, picture_name: Option<String>, lipsync_character_id: u16) -> Self {
        Self::from_inner(BustupLayerImpl::new(
            picture,
            picture_name,
            lipsync_character_id,
        ))
    }
}

impl NewDrawableLayerNeedsSeparatePass for BustupLayerImpl {}

impl NewDrawableLayer for BustupLayerImpl {
    fn update_from_props(&mut self, props: &LayerProperties) {
        self.animate = props.get_value(LayerProperty::Prop27) as i32 != 0;
        // `0x7100036900`: a speaking bustup pans its character's voices to the screen x of its own origin (not
        // composed with the parents) over the half-width
        if self.animate && !self.bustup.mouth_blocks.is_empty() {
            let x = props
                .get_transform_params()
                .compute_final_transform()
                .project_point3(Vec3::ZERO)
                .x;
            let pan = if x > -1.0 { x.min(1.0) } else { -1.0 };
            crate::audio::request_voice_pan(self.lipsync_character_id, pan);
        }
    }

    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let Some(params) = PictureBlockParams::setup(pass_kind, drawable) else {
            return;
        };

        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );

        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, false);
        let transform = transform.compute_final_transform();

        self.render_impl(pass, builder, params, transform);
    }
}

impl NewDrawableLayerFastForward for BustupLayerImpl {
    fn fast_forward(&mut self) {}
}

/// `BustupLayer::update` (`0x7100036a00`): lip-sync and blinking while property 27 is set
impl AdvUpdatable for BustupLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        if !self.animate {
            self.mouth_state = 0;
            self.eyes_state = 0;
            self.blink_timer = 0.0;
            return;
        }

        let mouth_count = self.bustup.mouth_blocks.len() as i32;
        let level = crate::audio::lipsync_level(self.lipsync_character_id);
        let mouth = if mouth_count == 0 || level < 0 {
            0
        } else {
            (level * mouth_count / 3).min(mouth_count - 1) as u32
        };
        self.mouth_state = mouth;

        self.blink_timer += ctx.delta_ticks.as_f32();
        if self.blink_frames.is_empty() {
            self.eyes_state = 0;
        } else if self.blink_timer >= self.blink_wait + BLINK_TICKS {
            self.next_blink();
            self.eyes_state = 0;
        } else if self.blink_timer >= self.blink_wait {
            let progress = (self.blink_timer - self.blink_wait) / BLINK_TICKS;
            let index = (progress * self.blink_frames.len() as f32) as usize;
            self.eyes_state = self.blink_frames.get(index).copied().unwrap_or(0);
        } else {
            self.eyes_state = 0;
        }
    }
}

impl Debug for BustupLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BustupLayer").field(&self.label).finish()
    }
}
