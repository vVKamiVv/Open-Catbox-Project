use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use enum_map::{Enum, EnumMap};
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::instruction_elements::CodeAddress, primitives::update::FrameId, time::Ticks,
    vm::Scripter,
};
use shin_input::{Action, ActionState, Pointer, RawInputState, inputs::MouseButton};
use shin_render::render_pass::RenderPass;
use shin_window::{AppContext, RenderContext, ShinApp};
use tracing::{debug, error, info};
use winit::{dpi::PhysicalSize, keyboard::KeyCode};

use crate::{
    adv::{Adv, assets::AdvAssets},
    asset::system::{AssetLoadContext, AssetServer, cache::AssetCache, locate_assets},
    capture::{self, CaptureTargets, PendingCapture},
    cli::Cli,
    layer::user::EffectTextures,
    render::PreRenderContext,
    update::UpdateContext,
};

#[derive(Debug, Enum)]
pub enum AppAction {
    ToggleFullscreen,
    Act,
    Enter,
    Cancel,
    AnyDown,
    AnyUp,
    AnyLeft,
    AnyRight,
    PageUp,
    PageDown,
    HoldSkip,
}

impl Action for AppAction {
    fn lower(raw_input_state: &RawInputState) -> EnumMap<Self, bool> {
        EnumMap::from_fn(|action| match action {
            AppAction::ToggleFullscreen => raw_input_state.keyboard.contains(&KeyCode::F11),
            AppAction::Act => {
                raw_input_state.keyboard.contains(&KeyCode::Space)
                    || raw_input_state.mouse.buttons[MouseButton::Left]
            }
            AppAction::Enter => {
                raw_input_state.keyboard.contains(&KeyCode::Space)
                    | raw_input_state.keyboard.contains(&KeyCode::Enter)
            }
            AppAction::Cancel => raw_input_state.keyboard.contains(&KeyCode::Backspace),
            AppAction::AnyDown => raw_input_state.keyboard.contains(&KeyCode::ArrowDown),
            AppAction::AnyUp => raw_input_state.keyboard.contains(&KeyCode::ArrowUp),
            AppAction::AnyLeft => raw_input_state.keyboard.contains(&KeyCode::ArrowLeft),
            AppAction::AnyRight => raw_input_state.keyboard.contains(&KeyCode::ArrowRight),
            AppAction::PageUp => raw_input_state.keyboard.contains(&KeyCode::PageUp),
            AppAction::PageDown => raw_input_state.keyboard.contains(&KeyCode::PageDown),
            AppAction::HoldSkip => raw_input_state.keyboard.contains(&KeyCode::ControlLeft),
        })
    }
}

pub struct App {
    frame_id: FrameId,
    #[expect(unused)] // for future stuff
    audio_manager: Arc<AudioManager>,
    asset_server: Arc<AssetServer>,
    effect_textures: Arc<EffectTextures>,
    adv: Adv,
    capture_dir: PathBuf,
    /// A captured frame queued for readback; saved on the next update, after the GPU has run the copy
    pending_capture: Option<PendingCapture>,
}

impl ShinApp for App {
    type Parameters = Cli;
    type EventType = ();
    type ActionType = AppAction;

    fn init(context: AppContext<Self>, cli: Self::Parameters) -> anyhow::Result<Self> {
        let audio_manager = Arc::new(AudioManager::new());

        crate::asset::executable::load(
            &[
                cli.exefs.clone(),
                std::env::var_os("SHIN_EXEFS").map(PathBuf::from),
                cli.assets_dir.as_ref().map(|dir| dir.join("exefs")),
                std::env::var_os("SHIN_ASSETS").map(|dir| PathBuf::from(dir).join("exefs")),
                std::env::current_dir().ok().map(|dir| dir.join("assets").join("exefs")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        );
        let asset_io = locate_assets(cli.assets_dir.as_deref()).context("Failed to locate assets. Consult the README for instructions on how to set up the game.")?;

        debug!("Asset IO: {:#?}", asset_io);

        let asset_server = Arc::new(AssetServer::new(asset_io.into(), AssetLoadContext {
            wgpu_device: context.wgpu.device.clone(),
            wgpu_queue: context.wgpu.queue.clone(),
            bustup_cache: AssetCache::new(),
        }));

        // TODO: do not block the game loop (?)
        let adv_assets = shin_tasks::block_on(AdvAssets::load(&asset_server)).unwrap();

        let mut scripter = Scripter::new(&adv_assets.scenario, 0, 42);

        if let Some(addr) = cli.unsafe_entry_point {
            debug!("Starting execution from 0x{:x}", addr);
            scripter.unsafe_set_position(CodeAddress(addr));
        }

        let effect_textures = adv_assets.effect_textures.clone();
        let mut adv = Adv::new(audio_manager.clone(), adv_assets, scripter);

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
            adv,
            capture_dir: cli.capture_dir,
            pending_capture: None,
        })
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
        if input[AppAction::ToggleFullscreen].is_clicked {
            context.winit.toggle_fullscreen();
        }

        if let Some(pending) = self.pending_capture.take() {
            match pending.save(&context.wgpu.device, &self.capture_dir) {
                Ok(path) => info!("Captured {}", path.display()),
                Err(e) => error!("Saving a capture failed: {e:?}"),
            }
            if !self.adv.capture_saved() {
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

        let mut pre_render_context = PreRenderContext {
            device: &context.wgpu.device,
            queue: &context.wgpu.queue,
            resize_source: &context.winit.resize_source,
            sampler_store: &context.render.sampler_store,
            depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),
            effect_textures: &self.effect_textures,

            pipeline_storage: &mut context.render.pipelines,
            dynamic_buffer: &mut context.render.dynamic_buffer,
            encoder: command_encoder,
        };

        let mut update_context = UpdateContext {
            frame_id: self.frame_id,
            delta_ticks: Ticks::from_duration(elapsed_time),
            asset_server: &self.asset_server,
            pre_render: &mut pre_render_context,
        };

        self.adv.update(&mut update_context, input, pointer);


        if self.pending_capture.is_none() {
            if let Some(message_id) = self.adv.capture_due() {
                self.pending_capture = Some(capture::render(
                    &mut pre_render_context,
                    &self.adv,
                    message_id,
                ));
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

    #[tracing::instrument(skip_all)]
    fn render(&mut self, _context: RenderContext, pass: &mut RenderPass) {
        self.adv.render(pass);

        // render_layer(pass, &transform, &self.adv, FloatColor4::BLACK, 0);
    }
}

