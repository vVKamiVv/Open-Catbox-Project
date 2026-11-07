use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use enum_map::{Enum, EnumMap};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::{GameDataEntry, RouteData, SaveData},
        scenario::instruction_elements::CodeAddress,
    },
    primitives::update::FrameId,
    time::Ticks,
    vm::command::types::{MaskFlags, MaskParam},
};
use shin_input::{Action, ActionState, Pointer, RawInputState};
use shin_render::{RenderRequestBuilder, render_pass::RenderPass};
use shin_window::{AppContext, DisplaySettings, RenderContext, ShinApp};
use tracing::{debug, error, info, warn};
use winit::dpi::PhysicalSize;

use shin_core::primitives::color::FloatColor4;
use shin_render::render_texture::RenderTexture;

use crate::{
    adv::{Adv, assets::AdvAssets, menu::MenuAssets},
    character_box::CharacterBox,
    movie_box::MovieBox,
    picture_box::PictureBox,
    music_box::MusicBox,
    scenario_select::{Bridge, ScenarioSelect, SelectExit},
    title::{Title, TitleExit},
    title_demo::TitleDemo,
    ui,
    audio::BgmPlayer,
    asset::{
        mask::MaskTexture,
        system::{AssetLoadContext, AssetServer, cache::AssetCache, locate_assets},
    },
    capture::{self, CaptureTargets, PendingCapture},
    cli::Cli,
    fps_counter::FpsCounter,
    layer::user::EffectTextures,
    render::{
        PreRenderContext,
        overlay::{OverlayManager, OverlayVisitable},
    },
    port_settings, savedata,
    update::UpdateContext,
    wiper::{mask::MaskWiperImpl, timed::TimedWiper},
};

/// The player's inputs, in the roles of the Switch's buttons (`0x71000c4300`); a gamepad maps by position (Nintendo
/// layout: A, the east button, confirms; B, the south one, cancels), the keyboard and mouse stand in for them. What
/// triggers each is in [`crate::bindings`], remappable on the Controls screen.
#[derive(Debug, Enum, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    ToggleFullscreen,
    /// A: confirm, advance
    Enter,
    /// B: cancel
    Cancel,
    AnyDown,
    AnyUp,
    AnyLeft,
    AnyRight,
    /// The right stick, which scrolls texts
    PageUp,
    PageDown,
    /// A PC extra: skip while held (still under the skip rules)
    HoldSkip,
    /// L: toggles auto mode
    AutoToggle,
    /// R: toggles skip mode
    SkipToggle,
    /// X (backlog) and Y (system menu); either also stops auto and skip. The keyboard's X and Y keys
    ButtonX,
    ButtonY,
    /// + and - (the keyboard's = and -): the scenario select's "Unlock all", the story's chapter jumps
    Plus,
    Minus,
    /// The galleries' Save; in the story it saves to the slot the save menu would open at (F5, P, ZR)
    Save,
    /// Port-only: the story's roll back, by default the menus' up (read only where up would roll back)
    RollBack,
    /// `--dev` only (ignored otherwise): F3 the overlay window, F6 pause, F7 one frame, F8/F9 slower/faster, F10 1x
    DevOverlay,
    DevPause,
    DevStep,
    DevSlower,
    DevFaster,
    DevSpeedReset,
}

impl Action for AppAction {
    /// Through the bindings: the defaults in `bindings.rs`, or what the player set on the Controls screen
    fn lower(raw_input_state: &RawInputState) -> EnumMap<Self, bool> {
        crate::bindings::lower(raw_input_state)
    }
}

pub struct App {
    frame_id: FrameId,
    audio_manager: Arc<AudioManager>,
    asset_server: Arc<AssetServer>,
    effect_textures: Arc<EffectTextures>,
    scene: Scene,
    /// The previous screen fading out over the new one, when the scene changed
    fade: Option<SceneFade>,
    /// The music player while a scene without one shows (the opening movie, the Movie Box, the bridge)
    idle_bgm: Option<BgmPlayer>,
    /// The save data while the bridge plays, which has none of its own
    bridge_global: Option<SaveData>,
    /// What a new adventure screen is built from
    adv_assets: AdvAssets,
    capture_dir: PathBuf,
    /// A captured frame queued for readback; saved on the next update, after the GPU has run the copy
    pending_capture: Option<PendingCapture>,
    /// Where the save file is written; `None` when no location is known
    savedata_path: Option<PathBuf>,
    /// `--dev`
    dev: Option<DevTools>,
    /// The tap ripples and drag stars over every scene (`*0x710022b8c8`)
    touch_effect: ui::touch_effect::TouchEffect,
}

impl ShinApp for App {
    type Parameters = Cli;
    type EventType = ();
    type ActionType = AppAction;

    fn init(context: AppContext<Self>, cli: Self::Parameters) -> anyhow::Result<Self> {
        let audio_manager = Arc::new(AudioManager::new());

        let (asset_io, assets_dir) = locate_assets(cli.assets_dir.as_deref(), cli.patch_dir.as_deref()).context("Failed to locate assets. Consult the README for instructions on how to set up the game.")?;
        crate::asset::executable::load(
            &[
                cli.exefs.clone(),
                std::env::var_os("SHIN_EXEFS").map(PathBuf::from),
                // wherever the assets were found (next to the executable, the shared data dir, ...)
                Some(assets_dir.join("exefs")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        );

        debug!("Asset IO: {:#?}", asset_io);

        let asset_server = Arc::new(AssetServer::new(asset_io.into(), AssetLoadContext {
            wgpu_device: context.wgpu.device.clone(),
            wgpu_queue: context.wgpu.queue.clone(),
            bustup_cache: AssetCache::new(),
        }));

        // TODO: do not block the game loop (?)
        let adv_assets = shin_tasks::block_on(AdvAssets::load(&asset_server)).unwrap();
        ui::prompts::init(&context.wgpu.device, &context.wgpu.queue);

        let mut savedata_path = cli.savedata.clone().or_else(savedata::default_path);
        let save = match savedata_path.as_deref().map(savedata::load) {
            Some(Ok(save)) => save,
            Some(Err(e)) => {
                // never overwrite a save that could not be read
                error!("{e:?}; nothing will be saved this time");
                savedata_path = None;
                SaveData::new()
            }
            None => SaveData::new(),
        };
        let effect_textures = adv_assets.effect_textures.clone();
        let dev = cli
            .dev
            .then(|| DevTools::new(&context.wgpu.device, context.render.surface_texture_format));
        // the game boots to the title; debugging options and --new-game/--load go straight to the story
        let straight_to_story = cli.new_game
            || cli.load.is_some()
            || cli.fast_forward_to.is_some()
            || cli.unsafe_entry_point.is_some()
            || !cli.capture_messages.is_empty();
        if !straight_to_story {
            // the opening movie, then the title (`FirstBoot` -> `TitleDemo` -> `Title(-1)`)
            let (scene, fade) = match TitleDemo::new(
                &context.wgpu.device,
                &asset_server,
                audio_manager.clone(),
                &adv_assets,
                save,
            ) {
                Ok(demo) => (Scene::TitleDemo(demo), None),
                Err(save) => {
                    let title = Title::new(
                        &asset_server,
                        audio_manager.clone(),
                        MenuAssets::new(&adv_assets),
                        save,
                        -1,
                        BgmPlayer::new(audio_manager.clone()),
                    )?;
                    // the title fades in from the black of the boot over 90 frames (`Title::First`)
                    (Scene::Title(title), Some(SceneFade::new(None, 90.0)))
                }
            };
            return Ok(Self {
                frame_id: FrameId::default(),
                audio_manager,
                asset_server,
                effect_textures,
                scene,
                fade,
                idle_bgm: None,
                bridge_global: None,
                adv_assets,
                capture_dir: cli.capture_dir,
                pending_capture: None,
                savedata_path,
                dev,
                touch_effect: ui::touch_effect::TouchEffect::new(new_game_seed()),
            });
        }

        let entry = match cli.load {
            Some(slot) => slot
                .entry(&save)
                .with_context(|| format!("the save file has nothing in {slot:?}"))?,
            None => GameDataEntry {
                // scenario 0: the first episode
                scenario_id: cli.episode - 1,
                random_seed: new_game_seed_for(&cli),
                save_position: 0,
                route_data: RouteData::default(),
            },
        };

        let mut adv = Adv::new(
            audio_manager.clone(),
            asset_server.clone(),
            adv_assets.clone(),
            save,
            entry,
            cli.unsafe_entry_point.map(CodeAddress),
            BgmPlayer::new(audio_manager.clone()),
        );

        if let Some(addr) = cli.fast_forward_to {
            debug!("Fast forwarding to 0x{:x}", addr);
            adv.fast_forward_to(CodeAddress(addr));
        }

        if !cli.capture_messages.is_empty() {
            std::fs::create_dir_all(&cli.capture_dir)
                .with_context(|| format!("creating {}", cli.capture_dir.display()))?;
            adv.capture_messages(CaptureTargets::new(
                cli.capture_messages.clone(),
                cli.capture_settle,
            ));
            // captures have the canvas size: ask for the original's 1920x1080
            let _ = context
                .winit
                .window
                .request_inner_size(PhysicalSize::new(1920, 1080));
        }

        // let picture_name = "/picture/text001.pic";
        //
        // let picture = asset_server.load_sync::<Picture>(picture_name).unwrap();
        //
        // let mut tile_layer_bottom = TileLayer::new(
        //     FloatColor4::PASTEL_PINK,
        //     vec4(-1088.0, -612.0, 2176.0, 1224.0),
        // );
        // tile_layer_bottom
        //     .properties_mut()
        //     .set_layer_id(LayerId::new(1));
        //
        // let mut tile_layer_top = TileLayer::new(
        //     FloatColor4::PASTEL_GREEN,
        //     vec4(-300.0, -300.0, 600.0, 600.0),
        // );
        // {
        //     let props = tile_layer_top.properties_mut();
        //     props.set_layer_id(LayerId::new(3));
        //     props
        //         .property_tweener_mut(LayerProperty::MulColorAlpha)
        //         .fast_forward_to(200.0);
        // }
        //
        // let mut picture_layer = PictureLayer::new(picture, Some(picture_name.to_string()));
        // {
        //     let props = picture_layer.properties_mut();
        //     props.set_layer_id(LayerId::new(2));
        //     props
        //         .property_tweener_mut(LayerProperty::TranslateZ)
        //         .fast_forward_to(1500.0);
        //     props
        //         .property_tweener_mut(LayerProperty::MulColorAlpha)
        //         .fast_forward_to(800.0);
        // }
        //
        // let mut root_layer_group = RootLayerGroup::new();
        //
        // let screen_layer = root_layer_group.screen_layer_mut();
        //
        // let page_layer = screen_layer.page_layer_mut();
        //
        // let layer_group = page_layer.get_plane_mut(PlaneId::new(0));
        // layer_group.add_layer(LayerbankId::new(1), tile_layer_bottom.into());
        // layer_group.add_layer(LayerbankId::new(0), picture_layer.into());
        // layer_group.add_layer(LayerbankId::new(2), tile_layer_top.into());
        //
        // {
        //     let tweener = page_layer
        //         .properties_mut()
        //         .property_tweener_mut(LayerProperty::MulColorRed);
        //
        //     tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //
        //     // tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(0.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(0.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        // }

        Ok(Self {
            frame_id: FrameId::default(),
            audio_manager,
            asset_server,
            effect_textures,
            scene: Scene::Adv(adv),
            fade: None,
            idle_bgm: None,
            bridge_global: None,
            adv_assets,
            capture_dir: cli.capture_dir,
            pending_capture: None,
            savedata_path,
            dev,
            touch_effect: ui::touch_effect::TouchEffect::new(new_game_seed()),
        })
    }

    /// Loads the PC settings (`settings.json` next to the save file), which the window is created with
    fn initial_display(cli: &Cli) -> DisplaySettings {
        let savedata = cli.savedata.clone().or_else(savedata::default_path);
        port_settings::load(savedata.as_deref().map(port_settings::path_next_to));
        if cli.fullscreen {
            port_settings::override_for_run(|s| {
                s.window_mode = port_settings::WindowMode::Borderless
            });
        }
        port_settings::get().display()
    }

    fn display(&self) -> Option<DisplaySettings> {
        Some(port_settings::get().display())
    }

    fn custom_event(&mut self, _context: AppContext<Self>, _event: Self::EventType) {
        todo!()
    }

    fn update(
        &mut self,
        context: AppContext<Self>,
        input: EnumMap<Self::ActionType, ActionState>,
        pointer: Pointer,
        elapsed_time: Duration,
        command_encoder: &mut wgpu::CommandEncoder,
    ) {
        // while the Controls screen waits for a key, the key is its alone
        let input = if crate::bindings::is_capturing() {
            EnumMap::default()
        } else {
            input
        };
        if input[AppAction::ToggleFullscreen].is_clicked {
            // F11 flips between the window and borderless fullscreen, as the PC tab's window mode
            port_settings::update(|s| {
                s.window_mode = match s.window_mode {
                    port_settings::WindowMode::Windowed => port_settings::WindowMode::Borderless,
                    _ => port_settings::WindowMode::Windowed,
                }
            });
        }
        // the one place the frame's time enters the game
        let elapsed_time = match &mut self.dev {
            Some(dev) => dev.game_time(&input, elapsed_time),
            None => elapsed_time,
        };
        // paused (and not stepping), the game sees no input either, so a click cannot advance a line
        let (input, pointer) = if self.dev.as_ref().is_some_and(|dev| dev.paused) && elapsed_time.is_zero() {
            let released = Pointer {
                position: pointer.position,
                held: false,
            };
            (EnumMap::default(), released)
        } else {
            (input, pointer)
        };

        if let Some(pending) = self.pending_capture.take() {
            match pending.save(&context.wgpu.device, &self.capture_dir) {
                Ok(path) => info!("Captured {}", path.display()),
                Err(e) => error!("Saving a capture failed: {e:?}"),
            }
            let more = match &mut self.scene {
                Scene::Adv(adv) => adv.capture_saved(),
                _ => false,
            };
            if !more {
                info!("All requested lines are captured");
                context.event_loop.exit();
            }
        }

        // if input[AppAction::Act].is_clicked {
        //     let screen_layer = self.root_layer_group.screen_layer_mut();
        //
        //     screen_layer.pageback(false);
        //     screen_layer
        //         .page_layer_mut()
        //         .properties_mut()
        //         .property_tweener_mut(LayerProperty::MulColorGreen)
        //         .fast_forward_to(2000.0);
        //     screen_layer.apply_transition(Some(DefaultWiper::new(Ticks::from_seconds(1.0)).into()));
        // }

        // a clone, so the scene can be switched while the context lives
        let effect_textures = self.effect_textures.clone();
        let asset_server = self.asset_server.clone();
        let mut pre_render_context = PreRenderContext {
            device: &context.wgpu.device,
            queue: &context.wgpu.queue,
            resize_source: &context.winit.resize_source,
            sampler_store: &context.render.sampler_store,
            depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),
            effect_textures: &effect_textures,

            pipeline_storage: &mut context.render.pipelines,
            dynamic_buffer: &mut context.render.dynamic_buffer,
            encoder: command_encoder,
        };

        ui::prompts::refresh(&pre_render_context, &self.adv_assets.fonts.system_font);

        let mut update_context = UpdateContext {
            frame_id: self.frame_id,
            delta_ticks: Ticks::from_duration(elapsed_time),
            asset_server: &asset_server,
            pre_render: &mut pre_render_context,
        };

        if let Some(fade) = &mut self.fade {
            fade.elapsed += update_context.delta_ticks.as_f32();
            fade.updates += 1;
            if fade.elapsed >= fade.duration {
                debug!(
                    "A {}-frame scene fade took {} ms over {} updates",
                    fade.duration,
                    fade.started.elapsed().as_millis(),
                    fade.updates
                );
                self.fade = None;
            }
        }

        let mut write = false;
        let mut next = None;
        match &mut self.scene {
            Scene::Adv(adv) => {
                adv.update(&mut update_context, input, pointer);
                if input[AppAction::Save].is_clicked {
                    let slot = adv.save_data().save_menu_position as usize;
                    adv.save_to_slot(slot.min(99));
                    write = true;
                }
                write |= adv.take_write_request();
                if let Some((entry, crossfade)) = adv.take_load_request() {
                    // `0x7100016410`: the scene becomes a new adventure screen that loads the bookmark
                    info!("Loading the bookmark at save point {}", entry.save_position);
                    next = Some(Next::Story(entry, crossfade));
                } else if adv.take_exit_to_title() {
                    // `ADV::ExitGame`: `CreateTitle(-1)`
                    next = Some(Next::Title);
                }
            }
            Scene::Title(title) => {
                title.update(&mut update_context, &input, pointer);
                write |= title.take_write_request();
                match title.take_exit() {
                    Some(TitleExit::Load(entry)) => next = Some(Next::Story(entry, true)),
                    Some(TitleExit::ScenarioSelect) => next = Some(Next::ScenarioSelect),
                    Some(TitleExit::MusicBox) => next = Some(Next::MusicBox),
                    Some(TitleExit::MovieBox) => next = Some(Next::MovieBox),
                    Some(TitleExit::PictureBox) => next = Some(Next::PictureBox),
                    Some(TitleExit::CharacterBox) => next = Some(Next::CharacterBox),
                    None => {}
                }
            }
            Scene::CharacterBox(characters) => {
                characters.update(&mut update_context, &input, pointer);
                if characters.is_done() {
                    // `CreateTitle(10)`: back with the cursor on the Character Box
                    next = Some(Next::TitleAt(10));
                }
            }
            Scene::PictureBox(pictures) => {
                pictures.update(&mut update_context, &input, pointer);
                if pictures.is_done() {
                    // `CreateTitle(6)`: back with the cursor on the Picture Box
                    next = Some(Next::TitleAt(6));
                }
            }
            Scene::MovieBox(movies) => {
                movies.update(&mut update_context, &input, pointer);
                if movies.is_done() {
                    // `CreateTitle(8)`: back with the cursor on the Movie Box
                    next = Some(Next::TitleAt(8));
                }
            }
            Scene::MusicBox(music) => {
                music.update(&mut update_context, &input, pointer);
                if music.is_done() {
                    // `CreateTitle(7)`: back with the cursor on the Music Box
                    next = Some(Next::TitleAt(7));
                }
            }
            Scene::ScenarioSelect(select) => {
                select.update(&mut update_context, &input, pointer);
                match select.take_exit() {
                    Some(SelectExit::Title) => next = Some(Next::TitleAt(0)),
                    Some(SelectExit::Start(scenario)) => next = Some(Next::Bridge(scenario)),
                    Some(SelectExit::Rebuild) => next = Some(Next::ScenarioSelect),
                    None => {}
                }
            }
            Scene::TitleDemo(demo) => {
                if demo.update(&mut update_context, &input, pointer) {
                    next = Some(Next::Title);
                }
            }
            Scene::Bridge(bridge) => {
                let scenario = bridge.update(update_context.delta_ticks.as_f32());
                bridge.pre_render(update_context.pre_render);
                if let Some(scenario) = scenario {
                    // `0x71000e4630`: the new game's first entry
                    next = Some(Next::Story(
                        GameDataEntry {
                        scenario_id: scenario as i32,
                        random_seed: new_game_seed(),
                        save_position: 0,
                        route_data: RouteData::default(),
                    },
                        true,
                    ));
                }
            }
        }
        if write {
            self.write_save();
        }
        let frames = update_context.delta_ticks.as_f32();
        if let Some(next) = next {
            self.switch_scene(&mut pre_render_context, next);
        }
        // a masked fade wipes to the new screen, which it needs as a picture of its own
        let Self { fade, scene, .. } = self;
        if let Some((_, to)) = fade.as_mut().and_then(|fade| fade.mask.as_mut()) {
            let depth_stencil = pre_render_context.depth_stencil;
            let mut pass = pre_render_context.begin_pass(
                to.as_texture_target(),
                Some(depth_stencil),
                "scene_fade/new",
            );
            scene.render(&mut pass);
        }
        // after the scene's update, in every scene; the ripples refract the finished frame, which they need as a
        // picture of its own
        let enabled = self.save_data().settings.v17_showtoucheffect;
        let Self { fade, scene, touch_effect, .. } = self;
        touch_effect.update(pointer, frames, enabled);
        touch_effect.capture(&mut pre_render_context, |pass| {
            scene.render(pass);
            if let Some(fade) = fade {
                fade.render(pass);
            }
        });

        if self.pending_capture.is_none() {
            if let Scene::Adv(adv) = &self.scene {
                if let Some(message_id) = adv.capture_due() {
                    self.pending_capture = Some(capture::render(
                        &mut pre_render_context,
                        adv,
                        message_id,
                    ));
                }
            }
        }

        // let update_context = AdvUpdateContext {
        //     delta_time: Ticks::from_duration(elapsed_time),
        //     asset_server: &self.asset_server,
        //     are_animations_allowed: true,
        // };
        //
        // self.root_layer_group.update(&update_context);
        //
        // let transform = TransformParams::default();
        //
        // let mut encoder =
        //     context
        //         .wgpu
        //         .device
        //         .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        //             label: Some("App::update"),
        //         });
        //
        // let mut pre_render_context = PreRenderContext {
        //     device: &context.wgpu.device,
        //     queue: &context.wgpu.queue,
        //     resize_source: &context.winit.resize_source,
        //     sampler_store: &context.render.sampler_store,
        //     depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),
        //
        //     pipeline_storage: &mut context.render.pipelines,
        //     dynamic_buffer: &mut context.render.dynamic_buffer,
        //     encoder: &mut encoder,
        // };
        //
        // self.root_layer_group
        //     .pre_render(&mut pre_render_context, &transform);

        self.frame_id.advance();
    }

    /// The original writes the file when returning to the title; closing the window is the port's way back
    fn exiting(&mut self) {
        self.write_save();
    }

    #[tracing::instrument(skip_all)]
    fn render(&mut self, _context: RenderContext, pass: &mut RenderPass) {
        self.scene.render(pass);
        if let Some(fade) = &self.fade {
            fade.render(pass);
        }
        // over everything, right after the scene (`0x7100116070`)
        let drag = self.adv_assets.system_textures.contains("drag");
        self.touch_effect
            .render(pass, drag.then(|| self.adv_assets.system_textures.get("drag")));

        // render_layer(pass, &transform, &self.adv, FloatColor4::BLACK, 0);
    }

    /// `--dev`: runs the overlays' egui pass, which takes the clicks egui wants before the game sees them
    fn filter_input(&mut self, context: AppContext<Self>, input: &mut RawInputState) {
        crate::bindings::record_held(input);
        ui::prompts::set_device(input.last_device);
        let Some(dev) = &mut self.dev else { return };
        let window = &context.winit.window;
        let size = window.inner_size();
        dev.overlay
            .start_update(input, (size.width, size.height), window.scale_factor() as f32);
        let (fps, paused, speed) = (&dev.fps, dev.paused, dev.speed);
        let scene = &self.scene;
        dev.overlay.visit_overlays(|collector| {
            collector.overlay(
                "Time",
                |_ctx, top_left| {
                    if paused {
                        top_left.label(format!("PAUSED at {speed}x (F6 resume, F7 step)"));
                    } else if speed != 1.0 {
                        top_left.label(format!("Speed {speed}x (F10 resets)"));
                    }
                },
                true,
            );
            fps.visit_overlay(collector);
            if let Scene::Adv(adv) = scene {
                adv.visit_overlay(collector);
            }
        });
        dev.overlay
            .finish_update(&context.wgpu.device, &context.wgpu.queue, input);
    }

    fn render_overlay(
        &mut self,
        context: RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        if let Some(dev) = &mut self.dev {
            dev.overlay
                .render(&context.wgpu.device, &context.wgpu.queue, encoder, target);
        }
    }
}

/// `--dev`: the debug overlays and the control of the game's time
struct DevTools {
    overlay: OverlayManager,
    fps: FpsCounter,
    paused: bool,
    /// A power of two from 1/8 to 8
    speed: f32,
}

impl DevTools {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        Self {
            overlay: OverlayManager::new(device, surface_format),
            fps: FpsCounter::new(),
            paused: false,
            speed: 1.0,
        }
    }

    /// Handles the dev keys and turns the real frame time into the game's
    // shortcut: only the scenes' delta is scaled; audio, videos and wall-clock timers run on at 1x. Stretch them
    // in shin-audio/shin-video if slowed-down sound is ever needed
    fn game_time(&mut self, input: &EnumMap<AppAction, ActionState>, elapsed: Duration) -> Duration {
        self.fps.record(elapsed);
        if input[AppAction::DevOverlay].is_clicked {
            self.overlay.toggle_window();
        }
        if input[AppAction::DevPause].is_clicked {
            self.paused = !self.paused;
        }
        if input[AppAction::DevSlower].is_clicked {
            self.speed = (self.speed / 2.0).max(0.125);
        }
        if input[AppAction::DevFaster].is_clicked {
            self.speed = (self.speed * 2.0).min(8.0);
        }
        if input[AppAction::DevSpeedReset].is_clicked {
            self.speed = 1.0;
        }
        if !self.paused {
            elapsed.mul_f32(self.speed)
        } else if input[AppAction::DevStep].is_clicked {
            Duration::from_secs_f64(1.0 / 60.0)
        } else {
            Duration::ZERO
        }
    }
}

impl App {
    fn write_save(&self) {
        let Some(path) = &self.savedata_path else {
            warn!("No location for the save file; not saving");
            return;
        };
        if let Err(e) = savedata::write(path, self.save_data()) {
            error!("Saving failed: {e:?}");
        }
    }

    /// The save data, wherever the current scene keeps it
    fn save_data(&self) -> &SaveData {
        match &self.scene {
            Scene::TitleDemo(demo) => &demo.global,
            Scene::Title(title) => &title.global,
            Scene::ScenarioSelect(select) => &select.global,
            Scene::MusicBox(music) => &music.global,
            Scene::MovieBox(movies) => &movies.global,
            Scene::PictureBox(pictures) => &pictures.global,
            Scene::CharacterBox(characters) => &characters.global,
            Scene::Adv(adv) => adv.save_data(),
            Scene::Bridge(_) => self.bridge_global.as_ref().expect("the bridge keeps the save data"),
        }
    }

    /// Replaces the scene; the title fades in over the old screen (`Title::First`: 90 frames, as after the boot,
    /// or 60 when coming back), the scenario select over 60 frames
    fn switch_scene(&mut self, ctx: &mut PreRenderContext, next: Next) {
        let global = self.save_data().clone();
        // the music: the title's fades as the next screen comes in, the scenario select's over the bridge
        let music_fade = match next {
            Next::Bridge(_) => 120.0,
            _ => 60.0,
        };
        match &mut self.scene {
            Scene::Adv(adv) => adv.shutdown(),
            Scene::Title(title) => title.shutdown(music_fade),
            Scene::ScenarioSelect(select) => select.shutdown(music_fade),
            // `0x7100032fc0`: the music fades over 30 frames as the Music Box closes
            Scene::MusicBox(music) => music.shutdown(30.0),
            // `Close $_0` (`0x710004cf40`): the BGM fades over 30 frames
            Scene::PictureBox(pictures) => pictures.shutdown(30.0),
            // `0x710003dc60`: the BGM fades over 30 frames
            Scene::CharacterBox(characters) => characters.shutdown(30.0),
            Scene::Bridge(_) | Scene::TitleDemo(_) | Scene::MovieBox(_) => {}
        }
        // one player for every scene, as the original's (`0x7100033eb0` runs outside the scenes)
        let mut bgm = Some(
            self.scene
                .take_bgm_from(&mut self.idle_bgm, &self.audio_manager),
        );
        let mut snapshot = || {
            let mut old = ctx.new_render_texture("scene_fade".to_string());
            let depth_stencil = ctx.depth_stencil;
            let mut pass = ctx.begin_pass(old.as_texture_target(), Some(depth_stencil), "scene_fade");
            self.scene.render(&mut pass);
            drop(pass);
            old
        };
        match next {
            Next::CharacterBox => {
                // `BustupMode::First`: a 60-frame crossfade while the title's music fades over 60 frames
                let old = snapshot();
                match CharacterBox::new(
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    bgm.take().unwrap(),
                ) {
                    Ok(characters) => {
                        self.scene = Scene::CharacterBox(characters);
                        self.fade = Some(SceneFade::new(Some(old), 60.0));
                    }
                    Err(e) => error!("Could not open the Character Box: {e:?}"),
                }
                return;
            }
            Next::PictureBox => {
                // `CGMode::First`: a 60-frame crossfade while the title's music fades over 60 frames
                let old = snapshot();
                match PictureBox::new(
                    &*ctx,
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    bgm.take().unwrap(),
                ) {
                    Ok(pictures) => {
                        self.scene = Scene::PictureBox(pictures);
                        self.fade = Some(SceneFade::new(Some(old), 60.0));
                    }
                    Err(e) => error!("Could not open the Picture Box: {e:?}"),
                }
                return;
            }
            Next::MovieBox => {
                // `MovieMode::First`: a 60-frame crossfade while the title's music fades over 60 frames
                let old = snapshot();
                match MovieBox::new(
                    &*ctx,
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                ) {
                    Ok(movies) => {
                        self.scene = Scene::MovieBox(movies);
                        self.idle_bgm = bgm.take();
                        self.fade = Some(SceneFade::new(Some(old), 60.0));
                    }
                    Err(e) => error!("Could not open the Movie Box: {e:?}"),
                }
                return;
            }
            Next::MusicBox => {
                // `BGMMode::First`: a 60-frame crossfade while the title's music fades over 60 frames
                let old = snapshot();
                match MusicBox::new(
                    &*ctx,
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    bgm.take().unwrap(),
                ) {
                    Ok(music) => {
                        self.scene = Scene::MusicBox(music);
                        self.fade = Some(SceneFade::new(Some(old), 60.0));
                    }
                    Err(e) => error!("Could not open the Music Box: {e:?}"),
                }
                return;
            }
            Next::ScenarioSelect => {
                let old = snapshot();
                match ScenarioSelect::new(
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    bgm.take().unwrap(),
                ) {
                    Ok(select) => {
                        self.scene = Scene::ScenarioSelect(select);
                        // `ScenarioSelect::First`: a soft wipe through `/curtain.msk` (softness 0.5) over 60 frames
                        let mut fade = SceneFade::new(Some(old), 60.0);
                        match self.asset_server.load_sync::<MaskTexture>("/curtain.msk") {
                            Ok(mask) => {
                                let wiper =
                                    MaskWiperImpl::new(mask, MaskParam(0.5), MaskFlags::empty());
                                let to = ctx.new_render_texture("scene_fade/new".to_string());
                                fade.mask = Some((wiper, to));
                            }
                            Err(e) => warn!("Could not load /curtain.msk: {e:?}"),
                        }
                        self.fade = Some(fade);
                    }
                    Err(e) => error!("Could not open the scenario select: {e:?}"),
                }
                return;
            }
            Next::Bridge(scenario) => {
                let old = snapshot();
                self.fade = None;
                self.scene = Scene::Bridge(Bridge::new(old, scenario));
                self.idle_bgm = bgm.take();
                // the bridge keeps the save data in memory for the story that follows
                self.bridge_global = Some(global);
                return;
            }
            Next::TitleAt(cursor) => {
                let old = snapshot();
                match Title::new(
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    cursor,
                    bgm.take().unwrap(),
                ) {
                    Ok(title) => {
                        self.scene = Scene::Title(title);
                        self.fade = Some(SceneFade::new(Some(old), 60.0));
                    }
                    Err(e) => error!("Could not open the title: {e:?}"),
                }
                return;
            }
            _ => {}
        }
        match next {
            Next::Story(entry, crossfade) => {
                // the scene loader's mode 0 crossfades from the old screen over 30 frames (`0x7100062d50`, 0x1e)
                self.fade = crossfade.then(|| SceneFade::new(Some(snapshot()), 30.0));
                self.scene = Scene::Adv(Adv::new(
                    self.audio_manager.clone(),
                    self.asset_server.clone(),
                    self.adv_assets.clone(),
                    global,
                    entry,
                    None,
                    bgm.take().unwrap(),
                ));
            }
            Next::Title => {
                let old = snapshot();
                match Title::new(
                    &self.asset_server,
                    self.audio_manager.clone(),
                    MenuAssets::new(&self.adv_assets),
                    global,
                    -1,
                    bgm.take().unwrap(),
                ) {
                    Ok(title) => {
                        self.scene = Scene::Title(title);
                        self.fade = Some(SceneFade::new(Some(old), 90.0));
                    }
                    Err(e) => error!("Could not open the title: {e:?}"),
                }
            }
            Next::ScenarioSelect | Next::Bridge(_) | Next::TitleAt(_) | Next::MusicBox
            | Next::MovieBox
            | Next::PictureBox
            | Next::CharacterBox => unreachable!(),
        }
    }
}

/// What the app shows
enum Scene {
    /// The opening movie
    TitleDemo(TitleDemo),
    Title(Title),
    ScenarioSelect(ScenarioSelect),
    MusicBox(MusicBox),
    MovieBox(MovieBox),
    PictureBox(PictureBox),
    CharacterBox(CharacterBox),
    Bridge(Bridge),
    Adv(Adv),
}

impl Scene {
    /// The scene's music player, for the next one; the scenes without music hand back the app's (`idle`)
    fn take_bgm_from(
        &mut self,
        idle: &mut Option<BgmPlayer>,
        audio: &Arc<AudioManager>,
    ) -> BgmPlayer {
        let fresh = || BgmPlayer::new(audio.clone());
        match self {
            Scene::Title(title) => std::mem::replace(&mut title.bgm, fresh()),
            Scene::ScenarioSelect(select) => std::mem::replace(&mut select.bgm, fresh()),
            Scene::MusicBox(music) => std::mem::replace(&mut music.bgm, fresh()),
            Scene::PictureBox(pictures) => std::mem::replace(&mut pictures.bgm, fresh()),
            Scene::CharacterBox(characters) => std::mem::replace(&mut characters.bgm, fresh()),
            Scene::Adv(adv) => adv.take_bgm(),
            Scene::TitleDemo(_) | Scene::MovieBox(_) | Scene::Bridge(_) => {
                idle.take().unwrap_or_else(fresh)
            }
        }
    }

    fn render(&self, pass: &mut RenderPass) {
        match self {
            Scene::TitleDemo(demo) => demo.render(pass),
            Scene::Title(title) => title.render(pass),
            Scene::ScenarioSelect(select) => select.render(pass),
            Scene::MusicBox(music) => music.render(pass),
            Scene::MovieBox(movies) => movies.render(pass),
            Scene::PictureBox(pictures) => pictures.render(pass),
            Scene::CharacterBox(characters) => characters.render(pass),
            Scene::Bridge(bridge) => bridge.render(pass),
            Scene::Adv(adv) => adv.render(pass),
        }
    }
}

/// Where a scene asked to go
enum Next {
    /// A new adventure screen; crossfading from the old screen or not
    Story(GameDataEntry, bool),
    /// `CreateTitle(-1)`: as after the boot
    Title,
    /// `CreateTitle(cursor)`: back to the title with the cursor on that item
    TitleAt(i32),
    ScenarioSelect,
    Bridge(u8),
    MusicBox,
    MovieBox,
    PictureBox,
    CharacterBox,
}

/// The old screen (or black) over the new one, fading out (the transition of `Title::First`, program 8)
struct SceneFade {
    from: Option<RenderTexture>,
    elapsed: f32,
    duration: f32,
    /// A mask wipe instead of the crossfade (program 0x20), with the new screen rendered each frame
    mask: Option<(MaskWiperImpl, RenderTexture)>,
    /// For the debug log of its timing at the frame rate in use
    started: std::time::Instant,
    updates: u32,
}

impl SceneFade {
    fn new(from: Option<RenderTexture>, duration: f32) -> Self {
        Self {
            from,
            elapsed: 0.0,
            duration,
            mask: None,
            started: std::time::Instant::now(),
            updates: 0,
        }
    }

    fn render(&self, pass: &mut RenderPass) {
        let progress = (self.elapsed / self.duration).clamp(0.0, 1.0);
        if let (Some((wiper, to)), Some(from)) = (&self.mask, &self.from) {
            wiper.render(
                pass,
                RenderRequestBuilder::new(),
                to.as_texture_source(),
                from.as_texture_source(),
                progress,
            );
            return;
        }
        let alpha = 1.0 - progress;
        let full = [0.0, 0.0, 1920.0, 1080.0];
        match &self.from {
            Some(texture) => ui::draw_texture(
                pass,
                texture.as_texture_source(),
                ui::projection(),
                full,
                [0.0, 0.0, 1.0, 1.0],
                FloatColor4::WHITE.with_alpha(alpha),
                ui::builder(),
            ),
            None => ui::fill(pass, ui::projection(), full, FloatColor4::BLACK.with_alpha(alpha)),
        }
    }
}

/// The original draws a random seed for each new game (`0x71000fc2c0`); debug runs that seek lines keep a fixed one
/// so they repeat
fn new_game_seed_for(cli: &Cli) -> u32 {
    if cli.fast_forward_to.is_some() || !cli.capture_messages.is_empty() {
        return 42;
    }
    new_game_seed()
}

/// A random seed for a new game (`0x71000fc2c0`)
pub fn new_game_seed() -> u32 {
    use std::hash::BuildHasher;
    std::collections::hash_map::RandomState::new().hash_one(0u8) as u32
}
