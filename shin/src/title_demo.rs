//! The opening movie before the title (`shin::TitleDemo`, ctor `0x71001143c0`, update `0x7100114670`, render
//! `0x71001146d0`): movie table entry 0 (`sakucs_op`) full screen. Confirm, + or a tap skips it with a 90-frame fade
//! of the picture and the sound; at its end the title opens as after the boot.

use std::sync::Arc;

use enum_map::EnumMap;
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::SaveData,
        scenario::{info::MovieId, instruction_elements::FromNumber},
    },
    primitives::color::{FloatColor4, UnormColor},
    time::{Easing, Tweener},
    vm::command::types::Volume,
};
use shin_input::{ActionState, Pointer};
use shin_render::render_pass::RenderPass;
use tracing::warn;

use crate::{
    adv::assets::AdvAssets,
    app::AppAction,
    asset::{movie::Movie, system::AssetServer},
    audio::play_sys_se,
    layer::{
        Layer as _, render_layer_without_bg,
        render_params::TransformParams,
        user::movie_layer::{MovieArgs, MovieLayer},
    },
    ui,
    update::{AdvUpdatable as _, AdvUpdateContext, UpdateContext},
};

pub struct TitleDemo {
    audio: Arc<AudioManager>,
    pub global: SaveData,
    movie: MovieLayer,
    /// The picture's and the sound's level (`+0xb0`): 1, down to 0 over 90 frames when skipped
    fade: Tweener,
    leaving: bool,
    pointer_held: bool,
}

impl TitleDemo {
    /// `TitleDemo::First`: movie table entry 0, unlocked in the galleries; the save data back when there is no movie
    /// to play
    pub fn new(
        device: &wgpu::Device,
        asset_server: &AssetServer,
        audio: Arc<AudioManager>,
        assets: &AdvAssets,
        mut global: SaveData,
    ) -> Result<Self, SaveData> {
        let scenario = &assets.scenario;
        let movie_id = MovieId::from_number(0);
        let info = scenario.info_tables().movie_info(movie_id);
        let movie = match asset_server.load_sync::<Movie>(info.path()) {
            Ok(movie) => movie,
            Err(e) => {
                warn!("Could not load the opening movie: {e:?}");
                return Err(global);
            }
        };
        crate::adv::unlocks::movie_played(&mut global.save_vectors, scenario, movie_id);
        let movie = MovieLayer::new(
            device,
            &audio,
            movie,
            MovieArgs {
                volume_source: info.volume_source,
                transparency: info.transparency,
                local_volume: Volume(1.0),
                repeat: false,
            },
            None,
        );
        Ok(Self {
            audio,
            global,
            movie,
            fade: Tweener::new(1.0),
            leaving: false,
            pointer_held: false,
        })
    }

    /// True once the movie has ended or faded out
    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        state: &EnumMap<AppAction, ActionState>,
        pointer: Pointer,
    ) -> bool {
        let settings = self.global.settings;
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        crate::audio::sys_se::set_movie_setting_volumes(
            settings.v0_bgmvol,
            settings.v1_sfxvol,
            settings.v2_voicevol,
        );

        // `0x7100114b40`: confirm, + (engine bit 8) or a tap leaves with "decide"
        let released = self.pointer_held && !pointer.held;
        self.pointer_held = pointer.held;
        if !self.leaving
            && (state[AppAction::Enter].is_clicked || state[AppAction::Plus].is_clicked || released)
        {
            play_sys_se(
                &self.audio,
                context.asset_server,
                "decide",
                Volume::default(),
            );
            self.leaving = true;
            ui::tween(&mut self.fade, 0.0, 90.0, Easing::Linear);
        }

        ui::advance(&mut self.fade, context.delta_ticks.as_f32());
        self.movie.set_local_volume(self.fade.value());
        let update = AdvUpdateContext {
            frame_id: context.frame_id,
            delta_ticks: context.delta_ticks,
            asset_server: context.asset_server,
            device: context.pre_render.device,
            queue: context.pre_render.queue,
            are_animations_allowed: true,
        };
        self.movie.update(&update);
        self.movie
            .pre_render(context.pre_render, &TransformParams::default());

        self.movie.is_finished() || (self.leaving && self.fade.is_idle())
    }

    /// The movie over black, darkened by the fade
    pub fn render(&self, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        render_layer_without_bg(pass, &TransformParams::default(), &self.movie, 0);
        let dark = 1.0 - self.fade.value();
        ui::fill(
            pass,
            ui::projection(),
            [0.0, 0.0, 1920.0, 1080.0],
            FloatColor4::BLACK.with_alpha(dark),
        );
    }
}
