use std::sync::Arc;

use derivative::Derivative;
use from_variants::FromVariants;
use glam::vec4;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{
        Scenario,
        info::{BustupId, BustupInfoItem, MovieId, MovieInfoItem, PictureId, PictureInfoItem},
        instruction_elements::{TypedNumberArray, UntypedNumberArray, lower_number_array},
    },
    primitives::color::FloatColor4,
    vm::command::types::{LayerType, Volume},
};
use shin_render::{PassKind, render_pass::RenderPass, shaders::types::RenderClone};
use tracing::{debug, warn};

use crate::{
    asset::{
        texture_archive::NamedTextures,
        bustup::{Bustup, BustupArgs, CharacterId},
        movie::Movie,
        picture::Picture,
        system::AssetServer,
    },
    layer::{
        DrawableLayer, Layer, LayerProperties, render_params::TransformParams,
        user::movie_layer::MovieArgs,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

#[expect(unused)]
mod bustup_layer;
mod cake_layer;
mod last_layer;
pub mod minigame;
mod murderer;
#[expect(unused)]
pub mod movie_layer;
mod null_layer;
pub(crate) mod picture_layer;
mod quiz_layer;
mod rain_layer;
mod tile_layer;

pub use self::{
    bustup_layer::BustupLayer,
    movie_layer::MovieLayer,
    null_layer::NullLayer,
    picture_layer::PictureLayer,
    cake_layer::{CakeAssets, CakeLayer},
    last_layer::{LastLayer, LastTextures},
    murderer::{MurdererAssets, MurdererLayer},
    quiz_layer::{QuizLayer, QuizTextures},
    rain_layer::{EffectTextures, RainLayer},
    tile_layer::TileLayer,
};

#[derive(Derivative, RenderClone, FromVariants)]
#[derivative(Debug)]
pub enum UserLayer {
    #[derivative(Debug = "transparent")]
    Null(NullLayer),
    #[derivative(Debug = "transparent")]
    Picture(#[render_clone(needs_render)] PictureLayer),
    #[derivative(Debug = "transparent")]
    Bustup(#[render_clone(needs_render)] BustupLayer),
    #[derivative(Debug = "transparent")]
    Tile(#[render_clone(needs_render)] TileLayer),
    #[derivative(Debug = "transparent")]
    Movie(#[render_clone(needs_render)] MovieLayer),
    #[derivative(Debug = "transparent")]
    Rain(#[render_clone(needs_render)] RainLayer),
    #[derivative(Debug = "transparent")]
    Quiz(#[render_clone(needs_render)] QuizLayer),
    #[derivative(Debug = "transparent")]
    Cake(#[render_clone(needs_render)] CakeLayer),
    #[derivative(Debug = "transparent")]
    Last(#[render_clone(needs_render)] LastLayer),
    #[derivative(Debug = "transparent")]
    Murderer(#[render_clone(needs_render)] MurdererLayer),
}

impl UserLayer {
    pub async fn load(
        device: &wgpu::Device,
        asset_server: &AssetServer,
        audio_manager: &Arc<AudioManager>,
        scenario: &Scenario,
        layer_ty: LayerType,
        params: UntypedNumberArray,
    ) -> Self {
        // TODO: this API is not ideal, as we are blocking the main thread for layer loading
        // ideally we want to mimic the API of LayerLoader in the original game
        match layer_ty {
            LayerType::Null => NullLayer::new().into(),
            LayerType::Tile => {
                let (color, offset_x, offset_y, width, height, ..): TypedNumberArray =
                    lower_number_array(params);
                let color = FloatColor4::from_4bpp_property(color);
                let rect = vec4(
                    offset_x as f32,
                    offset_y as f32,
                    width as f32,
                    height as f32,
                );

                TileLayer::new(color, rect).into()
            }
            LayerType::Picture => {
                let (pic_id, ..): TypedNumberArray<PictureId> = lower_number_array(params);
                let pic_info @ PictureInfoItem { name, linked_cg_id } =
                    scenario.info_tables().picture_info(pic_id);
                debug!("Load picture: {} -> {} {:?}", pic_id, name, linked_cg_id);
                let pic = asset_server
                    .load::<Picture, _>(pic_info.path())
                    .await
                    .expect("Failed to load picture");
                PictureLayer::new(pic, Some(name.to_string())).into()
            }
            LayerType::Bustup => {
                let (bup_id, ..): TypedNumberArray<BustupId> = lower_number_array(params);
                let bup_info @ BustupInfoItem {
                    name,
                    emotion,
                    lipsync_character_id,
                } = scenario.info_tables().bustup_info(bup_id);
                debug!(
                    "Load bustup: {} -> {} {} {}",
                    bup_id, name, emotion, lipsync_character_id
                );
                let bup = asset_server
                    .load_with_args::<Bustup, _>(bup_info.path(), BustupArgs {
                        expression: emotion.to_string(),
                        // TODO: do this conversion on info load
                        character_id: CharacterId::new(*lipsync_character_id as i32),
                        disable_animations: false,
                    })
                    .await
                    .expect("Failed to load bustup");

                BustupLayer::new(bup, Some(name.to_string()), *lipsync_character_id).into()
            }
            LayerType::Movie => {
                let (movie_id, volume, repeat, ..): TypedNumberArray<MovieId, Volume> =
                    lower_number_array(params);
                let movie_info @ &MovieInfoItem {
                    ref name,
                    linked_picture_id,
                    volume_source,
                    transparency,
                    linked_bgm_id,
                } = scenario.info_tables().movie_info(movie_id);
                let pic_path = scenario
                    .info_tables()
                    .picture_info(linked_picture_id)
                    .path();
                debug!(
                    "Load movie: {movie_id} -> {name} {linked_picture_id} {volume_source:?} {transparency:?} {linked_bgm_id:?}"
                );
                let movie = asset_server
                    .load::<Movie, _>(movie_info.path())
                    .await
                    .expect("Failed to load movie");

                let still_picture = Some(
                    asset_server
                        .load::<Picture, _>(pic_path)
                        .await
                        .expect("Failed to load still picture"),
                );

                let args = MovieArgs {
                    volume_source,
                    transparency,
                    local_volume: volume,
                    repeat: repeat & 1 != 0,
                };

                // MovieLayer::new blocks until ffmpeg yields the first frame, and the ffmpeg pipe
                // tasks run on the async-io pool we are on now, so build it on the compute pool
                let (device, audio_manager) = (device.clone(), audio_manager.clone());
                shin_tasks::compute::spawn(move || {
                    MovieLayer::new(&device, &audio_manager, movie, args, still_picture)
                })
                .await
                .into()
            }
            LayerType::Rain => {
                // kind: 0 rain, 1 petals, 2 snow, 3 gold snow
                let (kind, min_distance, max_distance, ..): TypedNumberArray =
                    lower_number_array(params);
                let textures = asset_server
                    .load::<EffectTextures, _>(crate::asset::asset_paths::EFFECTS)
                    .await
                    .expect("Failed to load effects.txa");

                RainLayer::new(textures, kind, min_distance, max_distance).into()
            }
            LayerType::Quiz => {
                // the loader (`0x7100084900`) builds a minigame by kind: 0 the quiz, 1 `CakeLayer`,
                // 2 `MurdererLayer` (the culprit game), 3 `LastLayer`; main8 loads each
                let (kind, param, ..): TypedNumberArray = lower_number_array(params);
                match kind {
                    0 => {
                        let textures = asset_server
                            .load::<QuizTextures, _>(crate::asset::asset_paths::QUIZ)
                            .await
                            .expect("Failed to load quiz.txa");
                        QuizLayer::new(textures, param as u32).into()
                    }
                    1 => {
                        let assets = asset_server
                            .load::<CakeAssets, _>(crate::asset::asset_paths::CAKE)
                            .await
                            .expect("Failed to load cake.txa");
                        CakeLayer::new(assets).into()
                    }
                    3 => {
                        let textures = asset_server
                            .load::<LastTextures, _>(crate::asset::asset_paths::LAST)
                            .await
                            .expect("Failed to load last.txa");
                        LastLayer::new(textures).into()
                    }
                    2 => {
                        let load = |path: String| async move {
                            asset_server
                                .load::<NamedTextures, _>(&path)
                                .await
                                .unwrap_or_else(|e| panic!("Failed to load {path}: {e:?}"))
                        };
                        let mut busts = Vec::new();
                        for name in MurdererAssets::bust_names() {
                            busts.push((name, load(format!("/chars/m_{name}.txa")).await));
                        }
                        let fonts = crate::adv::assets::AdvFonts::load(asset_server)
                            .await
                            .expect("Failed to load the fonts");
                        let assets = MurdererAssets {
                            fonts,
                            textures: load(crate::asset::asset_paths::MURDERER.to_string()).await,
                            system: load(crate::asset::asset_paths::SYSTEX.to_string()).await,
                            busts,
                        };
                        MurdererLayer::new(Arc::new(assets), param).into()
                    }
                    _ => {
                        // the loader builds nothing for other kinds
                        warn!("Quiz layer kind {kind} does not exist; loading an empty layer");
                        NullLayer::new().into()
                    }
                }
            }
            _ => {
                todo!("Layer type not implemented: {:?}", layer_ty);
            }
        }
    }
}

impl UserLayer {
    /// The minigame a QUIZ command can wait on, for the quiz layer type
    pub fn as_minigame_mut(&mut self) -> Option<&mut dyn minigame::Minigame> {
        match self {
            Self::Quiz(layer) => Some(layer.inner_mut()),
            Self::Cake(layer) => Some(layer.inner_mut()),
            Self::Last(layer) => Some(layer.inner_mut()),
            Self::Murderer(layer) => Some(layer.inner_mut()),
            _ => None,
        }
    }
}

impl AdvUpdatable for UserLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        match self {
            Self::Null(layer) => layer.update(context),
            Self::Picture(layer) => layer.update(context),
            Self::Bustup(layer) => layer.update(context),
            Self::Tile(layer) => layer.update(context),
            Self::Movie(layer) => layer.update(context),
            Self::Rain(layer) => layer.update(context),
            Self::Quiz(layer) => layer.update(context),
            Self::Cake(layer) => layer.update(context),
            Self::Last(layer) => layer.update(context),
            Self::Murderer(layer) => layer.update(context),
        }
    }
}

impl DrawableLayer for UserLayer {
    fn properties(&self) -> &LayerProperties {
        match self {
            Self::Null(layer) => layer.properties(),
            Self::Picture(layer) => layer.properties(),
            Self::Bustup(layer) => layer.properties(),
            Self::Tile(layer) => layer.properties(),
            Self::Movie(layer) => layer.properties(),
            Self::Rain(layer) => layer.properties(),
            Self::Quiz(layer) => layer.properties(),
            Self::Cake(layer) => layer.properties(),
            Self::Last(layer) => layer.properties(),
            Self::Murderer(layer) => layer.properties(),
        }
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        match self {
            Self::Null(layer) => layer.properties_mut(),
            Self::Picture(layer) => layer.properties_mut(),
            Self::Bustup(layer) => layer.properties_mut(),
            Self::Tile(layer) => layer.properties_mut(),
            Self::Movie(layer) => layer.properties_mut(),
            Self::Rain(layer) => layer.properties_mut(),
            Self::Quiz(layer) => layer.properties_mut(),
            Self::Cake(layer) => layer.properties_mut(),
            Self::Last(layer) => layer.properties_mut(),
            Self::Murderer(layer) => layer.properties_mut(),
        }
    }
}

impl Layer for UserLayer {
    fn fast_forward(&mut self) {
        match self {
            Self::Null(layer) => layer.fast_forward(),
            Self::Picture(layer) => layer.fast_forward(),
            Self::Bustup(layer) => layer.fast_forward(),
            Self::Tile(layer) => layer.fast_forward(),
            Self::Movie(layer) => layer.fast_forward(),
            Self::Rain(layer) => layer.fast_forward(),
            Self::Quiz(layer) => layer.fast_forward(),
            Self::Cake(layer) => layer.fast_forward(),
            Self::Last(layer) => layer.fast_forward(),
            Self::Murderer(layer) => layer.fast_forward(),
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        match self {
            Self::Null(layer) => layer.get_stencil_bump(),
            Self::Picture(layer) => layer.get_stencil_bump(),
            Self::Bustup(layer) => layer.get_stencil_bump(),
            Self::Tile(layer) => layer.get_stencil_bump(),
            Self::Movie(layer) => layer.get_stencil_bump(),
            Self::Rain(layer) => layer.get_stencil_bump(),
            Self::Quiz(layer) => layer.get_stencil_bump(),
            Self::Cake(layer) => layer.get_stencil_bump(),
            Self::Last(layer) => layer.get_stencil_bump(),
            Self::Murderer(layer) => layer.get_stencil_bump(),
        }
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        match self {
            Self::Null(layer) => layer.pre_render(context, transform),
            Self::Picture(layer) => layer.pre_render(context, transform),
            Self::Bustup(layer) => layer.pre_render(context, transform),
            Self::Tile(layer) => layer.pre_render(context, transform),
            Self::Movie(layer) => layer.pre_render(context, transform),
            Self::Rain(layer) => layer.pre_render(context, transform),
            Self::Quiz(layer) => layer.pre_render(context, transform),
            Self::Cake(layer) => layer.pre_render(context, transform),
            Self::Last(layer) => layer.pre_render(context, transform),
            Self::Murderer(layer) => layer.pre_render(context, transform),
        }
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        match self {
            Self::Null(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Picture(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Bustup(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Tile(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Movie(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Rain(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Quiz(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Cake(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Last(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
            Self::Murderer(layer) => layer.render(pass, transform, stencil_ref, pass_kind),
        }
    }
}
