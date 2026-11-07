use std::{fmt::Debug, sync::Arc};

use glam::{Mat4, vec3};
use parking_lot::Mutex;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::info::{MovieTransparencyMode, MovieVolumeSource},
    primitives::update::{FrameId, UpdateTracker},
    vm::command::types::Volume,
};
use shin_render::{
    LayerBlendType, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone, RenderCloneCtx,
        texture::{DepthStencilTarget, TextureTarget},
    },
};
use shin_tasks::ComputeTask;
use shin_video::VideoPlayerHandle;
use tracing::warn;

use crate::{
    asset::{movie::Movie, picture::Picture},
    layer::{
        DrawableLayer, Layer, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{
            NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState,
        },
        properties::LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::PictureLayer,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

struct Shared {
    update_tracker: UpdateTracker,
    volume_source: MovieVolumeSource,
    local_volume: Volume,
    /// A repeating movie's next run, starting in the background
    restart: Option<ComputeTask<anyhow::Result<VideoPlayerHandle>>>,
}

impl Shared {
    fn new(volume_source: MovieVolumeSource, local_volume: Volume) -> Self {
        Self {
            update_tracker: UpdateTracker::new(),
            volume_source,
            local_volume,
            restart: None,
        }
    }

    fn update(&mut self, frame_id: FrameId, handle: &VideoPlayerHandle) {
        if !self.update_tracker.update(frame_id) {
            return;
        }

        // the settings volume of the movie's volume source
        let settings_volume = Volume(crate::audio::sys_se::movie_setting_gain(self.volume_source));
        let final_volume = self.local_volume * settings_volume;

        handle.set_volume(final_volume);
    }
}

#[derive(RenderClone)]
pub struct MovieLayerImpl {
    movie_label: String,
    video_player: Arc<VideoPlayerHandle>,
    still_picture: Option<Arc<Picture>>,
    shared: Arc<Mutex<Shared>>,
    transparency: MovieTransparencyMode,
    repeat: bool,
    /// For playing a repeating movie again
    movie: Arc<Movie>,
    audio_manager: Arc<AudioManager>,
}

pub struct MovieArgs {
    pub volume_source: MovieVolumeSource,
    pub transparency: MovieTransparencyMode,
    pub local_volume: Volume,
    pub repeat: bool,
}

struct MovieBackendArgs {
    pub is_bgm: bool,
    pub transparency: MovieTransparencyMode,
    pub repeat: bool,
}

pub type MovieLayer = NewDrawableLayerWrapper<MovieLayerImpl>;

impl MovieLayer {
    pub fn new(
        device: &wgpu::Device,
        audio_manager: &Arc<AudioManager>,
        movie: Arc<Movie>,
        MovieArgs {
            volume_source,
            transparency,
            local_volume,
            repeat,
        }: MovieArgs,
        // NB: the original engine uses PictureLayer here
        // we have decoupled asset type from the layer type though
        still_picture: Option<Arc<Picture>>,
    ) -> Self {
        NewDrawableLayerWrapper::from_inner(MovieLayerImpl {
            movie_label: movie.label().to_string(),
            video_player: Arc::new(
                movie
                    .play(device, audio_manager)
                    .expect("Failed to play movie"),
            ),
            still_picture,
            shared: Arc::new(Mutex::new(Shared::new(volume_source, local_volume))),
            transparency,
            repeat,
            movie,
            audio_manager: audio_manager.clone(),
        })
    }

    /// The movie's own volume (the layer's, before the setting): the opening movie fades it out when skipped
    pub fn set_local_volume(&self, volume: f32) {
        self.inner_ref().shared.lock().local_volume = Volume(volume);
    }

    pub fn is_finished(&self) -> bool {
        self.inner_ref().video_player.is_finished()
    }
}

impl NewDrawableLayerNeedsSeparatePass for MovieLayerImpl {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        // NB: this if is not present in the original implementation
        // instead, it always forces an additional render pass to convert YUV to RGB
        // we try to be better and optimistically render it in one pass

        // In actuality, I think we can do even better, by adding support of some of these effects to the movie shader
        // but that's probably not very useful (?)
        if props.get_clip_mode() != DrawableClipMode::None
            || props.is_fragment_shader_nontrivial()
            || props.is_blending_nontrivial()
        {
            return true;
        }

        if !self.video_player.is_finished() {
            return false;
        }

        // a finished movie without the loop bit gives way to its linked still picture (`0x71000c3400`,
        // `0x71000c38c0`); the stills are the movies' own last frames (`no05` -> `no05_00039`), so keeping the
        // last frame looks the same and needs no separate pass
        false
    }
}

impl NewDrawableLayer for MovieLayerImpl {
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        _clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        // TODO: I think some of these conditions conflict with the needs_separate_pass logic
        // (aka they will never be true)
        let target_pass = if self.transparency != MovieTransparencyMode::Opaque
            || drawable.blend_type != LayerBlendType::Type1
            || drawable.color_multiplier.a < 1.0
        {
            PassKind::Transparent
        } else {
            PassKind::Opaque
        };

        if pass_kind != target_pass {
            return;
        }

        let Some(frame) = self.video_player.get_frame() else {
            return;
        };

        // NB: the original engine uses a generic layer shader here, because it does YUV->RGB conversion in a separate pass
        // we try to do better, so we do the conversion in the main pass

        let transform =
            transform.compute_final_transform() * Mat4::from_translation(vec3(-960.0, -540.0, 0.0));

        // drawn like any layer (the original uses the generic layer shader): with the layer's blend, its color
        // and opacity, and the movie's own alpha when it carries one
        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, false);
        let builder = match target_pass {
            PassKind::Opaque => builder,
            PassKind::Transparent => builder.layer_color_blend(drawable.blend_type),
        };
        frame.render(
            pass,
            builder,
            transform,
            {
                let c = drawable.color_multiplier;
                glam::vec4(c.r, c.g, c.b, c.a)
            },
            self.transparency == MovieTransparencyMode::Transparent,
        );
    }
}

impl NewDrawableLayerFastForward for MovieLayerImpl {
    fn fast_forward(&mut self) {
        // the original (`0x71000c33d0`) may end a movie that does not repeat here; unchecked, see alignment-plan.md
    }
}

impl AdvUpdatable for MovieLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.video_player
            .update(ctx.frame_id, ctx.delta_ticks, ctx.queue);

        // a repeating movie (LAYERLOAD's repeat bit) plays again from the start; starting a player waits for the
        // decoder's first frame, so the next run starts in the background
        if !self.repeat || !self.video_player.is_finished() {
            return;
        }
        let mut shared = self.shared.lock();
        let Some(task) = &mut shared.restart else {
            let (movie, device, audio_manager) = (
                self.movie.clone(),
                ctx.device.clone(),
                self.audio_manager.clone(),
            );
            shared.restart = Some(shin_tasks::compute::spawn(move || {
                movie.play(&device, &audio_manager)
            }));
            return;
        };
        let Some(result) = task.poll_naive() else {
            return;
        };
        shared.restart = None;
        match result {
            Ok(player) => {
                tracing::debug!("Movie {} plays again", self.movie_label);
                self.video_player = Arc::new(player);
            }
            Err(e) => {
                warn!("Could not play {} again: {e:?}", self.movie_label);
                self.repeat = false;
            }
        }
    }
}

impl Debug for MovieLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MovieLayer")
            .field(&self.movie_label)
            .finish()
    }
}
