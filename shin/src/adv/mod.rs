pub mod assets;
mod command;
mod vm_state;

use std::{borrow::Cow, sync::Arc};

pub use command::{CommandStartResult, ExecutingCommand, StartableCommand, UpdatableCommand};
use egui::Window;
use enum_map::{Enum, EnumMap, enum_map};
use glam::Mat4;
use itertools::Itertools;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::{Scenario, instruction_elements::CodeAddress},
    primitives::color::UnormColor,
    vm::{
        Scripter,
        breakpoint::BreakpointObserver,
        command::{
            CommandResult, RuntimeCommand,
            types::{LayerId, PLANES_COUNT, PlaneId, VLayerId, VLayerIdRepr},
        },
    },
};
use shin_input::{Action, ActionState, Pointer, inputs::MouseButton};
use shin_render::{
    render_pass::RenderPass,
    shaders::types::{RenderClone as _, RenderCloneCtx},
};
use smallvec::{SmallVec, smallvec};
use tracing::{debug, info, warn};
use vm_state::layers::ITER_VLAYER_SMALL_VECTOR_SIZE;
pub use vm_state::{VmState, layers::LayerSelection};
use winit::keyboard::KeyCode;

use crate::{
    adv::assets::{AdvAssets, AdvFonts},
    app::AppAction,
    audio::{BgmPlayer, SePlayer, VoicePlayer, play_sys_se},
    capture::CaptureTargets,
    layer::{
        AnyLayer, AnyLayerMut, Layer as _, LayerGroup, PageLayer, RootLayerGroup, ScreenLayer,
        message_layer::{MessageLayer, MessageboxTextures},
        render_layer_without_bg,
        render_params::TransformParams,
        select_layer::{SelectInput, SelectLayer},
        user::minigame::{Directions, MinigameInput, PointerTracker},
        user::UserLayer,
    },
    render::{
        PreRenderContext,
        overlay::{OverlayCollector, OverlayVisitable},
    },
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

/// Actions available in all ADV contexts
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Enum)]
pub enum AdvMessageAction {
    Advance,
    HoldFastForward,
    Backlog,
    Rollback,
}

// impl Action for AdvMessageAction {
//     fn default_action_map() -> ActionMap<Self> {
//         fn map(v: AdvMessageAction) -> InputSet {
//             match v {
//                 AdvMessageAction::Advance => [
//                     MouseButton::Left.into(),
//                     KeyCode::Enter.into(),
//                     KeyCode::Space.into(),
//                 ]
//                 .into_iter()
//                 .collect(),
//                 AdvMessageAction::HoldFastForward => {
//                     [KeyCode::ControlLeft.into()].into_iter().collect()
//                 }
//                 AdvMessageAction::Backlog => [].into_iter().collect(),
//                 AdvMessageAction::Rollback => [].into_iter().collect(),
//             }
//         }
//
//         ActionMap::new(enum_map! { v => map(v) })
//     }
// }

pub struct Adv {
    scenario: Arc<Scenario>,
    scripter: Scripter,
    vm_state: VmState,
    adv_state: AdvState,
    // action_state: ActionState<AdvMessageAction>,
    current_command: Option<ExecutingCommand>,
    /// The mouse over the choice menu
    select_pointer: PointerTracker,
    fast_forward_to_bp: Option<BreakpointObserver>,
    /// Set by `EXIT`; the script then stops running while the screen keeps updating
    script_exited: bool,
    /// `--capture-messages`: fast-forwards to these lines and holds each for saving
    capture: Option<CaptureTargets>,
}

impl Adv {
    pub fn new(audio_manager: Arc<AudioManager>, assets: AdvAssets, scripter: Scripter) -> Self {
        let scenario = assets.scenario.clone();
        let vm_state = VmState::new();
        let adv_state = AdvState::new(audio_manager, assets);

        Self {
            scenario,
            scripter,
            vm_state,
            adv_state,
            current_command: None,
            select_pointer: PointerTracker::default(),
            fast_forward_to_bp: None,
            script_exited: false,
            capture: None,
        }
    }

    pub fn fast_forward_to(&mut self, addr: CodeAddress) {
        assert!(self.fast_forward_to_bp.is_none());
        self.fast_forward_to_bp = Some(self.scripter.add_breakpoint(addr).into());
    }

    pub fn capture_messages(&mut self, targets: CaptureTargets) {
        self.capture = Some(targets);
    }

    /// The captured line whose frame should be saved now, if any
    pub fn capture_due(&self) -> Option<u32> {
        self.capture.as_ref().and_then(CaptureTargets::due)
    }

    /// Moves on to the next line to capture; false when the saved one was the last
    pub fn capture_saved(&mut self) -> bool {
        let more = self
            .capture
            .as_mut()
            .is_some_and(CaptureTargets::finish_due);
        if more {
            self.adv_state.message_layer_mut().try_advance();
        }
        more
    }

    fn is_fast_forwarding(&self, button_held: bool) -> bool {
        button_held
            || self.fast_forward_to_bp.is_some()
            || self
                .capture
                .as_ref()
                .is_some_and(CaptureTargets::is_seeking)
    }

    // TODO: impl Scene for Adv
    #[tracing::instrument(skip_all)]
    pub fn render(&self, pass: &mut RenderPass) {
        self.adv_state.render(pass);
    }

    pub fn handle_input(
        &mut self,
        state: EnumMap<AppAction, ActionState>,
        pointer: Pointer,
        is_focused: bool,
    ) {
        if !is_focused {
            return;
        }

        if let Some(select_layer) = &mut self.adv_state.select_layer {
            for (action, input) in [
                (AppAction::AnyUp, SelectInput::Up),
                (AppAction::AnyDown, SelectInput::Down),
                (AppAction::Enter, SelectInput::Confirm),
            ] {
                if state[action].is_clicked {
                    select_layer.handle_input(input);
                }
            }
            // a click goes through the pointer handlers
            self.select_pointer.dispatch(pointer, select_layer);
            return;
        }
        if let Some(ExecutingCommand::QUIZ(quiz)) = &mut self.current_command {
            let directions = |pick: fn(&ActionState) -> bool| Directions {
                up: pick(&state[AppAction::AnyUp]),
                down: pick(&state[AppAction::AnyDown]),
                left: pick(&state[AppAction::AnyLeft]),
                right: pick(&state[AppAction::AnyRight]),
            };
            let input = MinigameInput {
                held: directions(|s| s.is_held),
                pressed: directions(|s| s.is_clicked_or_repeated),
                // a click goes through the pointer handlers
                confirm: state[AppAction::Enter].is_clicked,
                cancel: state[AppAction::Cancel].is_clicked,
                text_scroll: state[AppAction::PageDown].is_held as i32 as f32
                    - state[AppAction::PageUp].is_held as i32 as f32,
                // the QUIZ command fills in the frame time
                frames: 0.0,
            };
            quiz.handle_input(input, pointer);
            return;
        }

        // a click advances the text too, as a touch does in the original
        if state[AppAction::Enter].is_clicked || state[AppAction::Act].is_clicked {
            self.adv_state.message_layer_mut().try_advance();
            if let Some(ExecutingCommand::WAIT(wait)) = &mut self.current_command {
                wait.interrupt();
            }
        }
    }

    // TODO: impl Scene for Adv
    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        input_state: EnumMap<AppAction, ActionState>,
        pointer: Pointer,
    ) {
        // self.action_state.update(context.raw_input_state);

        let fast_forward_button_held = input_state[AppAction::HoldSkip].is_held;
        // self
        //     .action_state
        //     .is_pressed(AdvMessageAction::HoldFastForward);

        // if self.action_state.is_just_pressed(AdvMessageAction::Advance) {
        //     self.adv_state
        //         .root_layer_group
        //         .message_layer_mut()
        //         .advance();
        // }

        // TODO: tasks from task pool can steal focus
        self.handle_input(input_state, pointer, true);

        if self.is_fast_forwarding(fast_forward_button_held) {
            self.adv_state.root_layer_group_mut().fast_forward();
            if let Some(back_layer_group) = &mut self.adv_state.back_layer_group {
                back_layer_group.fast_forward();
            }
        }

        let mut result = CommandResult::None;
        while !self.script_exited {
            // check the fast-forward breakpoint; delete if hit
            if self
                .fast_forward_to_bp
                .as_mut()
                .is_some_and(|bp| bp.update())
            {
                debug!("Fast-forward breakpoint hit, stopping the FF");
                self.fast_forward_to_bp = None;
            }

            let is_fast_forwarding = self.is_fast_forwarding(fast_forward_button_held);
            // only debug fast-forwarding picks choices itself; it can end mid-frame at its breakpoint
            self.adv_state.auto_choose = self.fast_forward_to_bp.is_some()
                || self.capture.as_ref().is_some_and(CaptureTargets::is_seeking);

            // TODO: maybe yield if spent too much time in this loop?
            let runtime_command = if let Some(command) = &mut self.current_command {
                match command.update(
                    context,
                    &self.scenario,
                    &self.vm_state,
                    &mut self.adv_state,
                    is_fast_forwarding,
                ) {
                    None => break,
                    Some(result) => {
                        self.current_command = None;
                        self.scripter.run(result).expect("scripter run failed")
                    }
                }
            } else {
                self.scripter.run(result).expect("scripter run failed")
            };

            if let (Some(capture), RuntimeCommand::MSGSET(msgset)) =
                (&mut self.capture, &runtime_command)
            {
                capture.on_line(msgset.msg_id.0);
            }

            match command::apply_command_state_and_start(
                runtime_command,
                context,
                &self.scenario,
                &mut self.vm_state,
                &mut self.adv_state,
            ) {
                CommandStartResult::Continue(r) => result = r,
                CommandStartResult::Yield(executing_command) => {
                    self.current_command = Some(executing_command);
                }
                CommandStartResult::Exit => {
                    // the original returns to the menus here, which the port does not have yet
                    info!(
                        "Script reached EXIT at {:?}; no menus to return to, so the script stops here",
                        self.scripter.position()
                    );
                    self.script_exited = true;
                }
            }
        }

        self.adv_state.update(context);

        if let Some(capture) = &mut self.capture {
            // a question line before a choice does not wait for a click: the menu waits instead
            let waiting = self.adv_state.message_layer().is_waiting_for_click()
                || self
                    .adv_state
                    .select_layer
                    .as_ref()
                    .is_some_and(SelectLayer::is_interactive);
            capture.update(context.delta_ticks, waiting);
        }
    }
}

// impl Renderable for Adv {
//     fn render<'enc>(
//         &'enc self,
//         resources: &'enc GpuCommonResources,
//         render_pass: &mut wgpu::RenderPass<'enc>,
//         transform: Mat4,
//         projection: Mat4,
//     ) {
//         self.adv_state
//             .render(resources, render_pass, transform, projection);
//     }
//
//     fn resize(&mut self, resources: &GpuCommonResources) {
//         self.adv_state.resize(resources);
//     }
// }

// impl OverlayVisitable for Adv {
//     fn visit_overlay(&self, collector: &mut OverlayCollector) {
//         collector.subgroup(
//             "ADV",
//             |collector| {
//                 collector.overlay(
//                     "Command",
//                     |_ctx, top_left| {
//                         let command = if let Some(command) = &self.current_command {
//                             Cow::Owned(format!("{:?}", command))
//                         } else {
//                             Cow::Borrowed("None")
//                         };
//                         top_left.label(format!(
//                             "Command: {:08x} {}",
//                             self.scripter.position().0,
//                             command
//                         ));
//                     },
//                     true,
//                 );
//                 self.adv_state
//                     .root_layer_group
//                     .message_layer()
//                     .visit_overlay(collector);
//                 collector.overlay(
//                     "User Layers",
//                     |ctx, _top_left| {
//                         let page_layer =
//                             self.adv_state.root_layer_group.screen_layer().page_layer();
//                         Window::new("User Layers").show(ctx, |ui| {
//                             for plane in 0..PLANES_COUNT {
//                                 let layer_group = page_layer.plane(plane as u32);
//                                 let layer_ids =
//                                     layer_group.get_layer_ids().sorted().collect::<Vec<_>>();
//                                 if !layer_ids.is_empty() {
//                                     ui.monospace(format!("Plane {}:", plane));
//                                     for layer_id in layer_ids {
//                                         let layer = layer_group.get_layer(layer_id).unwrap();
//                                         ui.monospace(format!(
//                                             "  {:>2}: {:?}",
//                                             layer_id.raw(),
//                                             layer
//                                         ));
//                                     }
//                                 }
//                             }
//                         });
//                     },
//                     false,
//                 );
//             },
//             true,
//         );
//     }
// }

/// Contains all the state of the ADV system NOT pertaining to the VM
///
/// This is the object the VM manipulates
pub struct AdvState {
    pub root_layer_group: RootLayerGroup,
    pub back_layer_group: Option<RootLayerGroup>,
    pub audio_manager: Arc<AudioManager>,
    pub bgm_player: BgmPlayer,
    pub se_player: SePlayer,
    pub allow_running_animations: bool,
    pub fonts: AdvFonts,
    pub messagebox_textures: Arc<MessageboxTextures>,
    /// The choice menu while a SELECT runs
    pub select_layer: Option<SelectLayer>,
    /// Set while the script is fast-forwarded for debugging: SELECT then picks the first option itself
    pub auto_choose: bool,
}

impl AdvState {
    pub fn new(audio_manager: Arc<AudioManager>, assets: AdvAssets) -> Self {
        Self {
            root_layer_group: RootLayerGroup::new(
                assets.fonts.clone(),
                assets.messagebox_textures.clone(),
                VoicePlayer::new(audio_manager.clone()),
            ),
            back_layer_group: None,
            audio_manager: audio_manager.clone(),
            bgm_player: BgmPlayer::new(audio_manager.clone()),
            se_player: SePlayer::new(audio_manager),
            allow_running_animations: true,
            fonts: assets.fonts.clone(),
            messagebox_textures: assets.messagebox_textures.clone(),
            select_layer: None,
            auto_choose: false,
        }
    }

    pub fn root_layer_group(&self) -> &RootLayerGroup {
        &self.root_layer_group
    }
    pub fn root_layer_group_mut(&mut self) -> &mut RootLayerGroup {
        &mut self.root_layer_group
    }

    pub fn message_layer(&self) -> &MessageLayer {
        self.root_layer_group.message_layer()
    }
    pub fn message_layer_mut(&mut self) -> &mut MessageLayer {
        self.root_layer_group.message_layer_mut()
    }

    pub fn screen_layer(&self) -> &ScreenLayer {
        self.root_layer_group.screen_layer()
    }
    pub fn screen_layer_mut(&mut self) -> &mut ScreenLayer {
        self.root_layer_group.screen_layer_mut()
    }

    pub fn page_layer(&self) -> &PageLayer {
        self.root_layer_group.screen_layer().page_layer()
    }
    pub fn page_layer_mut(&mut self) -> &mut PageLayer {
        self.root_layer_group.screen_layer_mut().page_layer_mut()
    }

    pub fn plane_layer_group(&self, plane: PlaneId) -> &LayerGroup {
        self.root_layer_group
            .screen_layer()
            .page_layer()
            .get_plane(plane)
    }

    pub fn plane_layer_group_mut(&mut self, plane: PlaneId) -> &mut LayerGroup {
        self.root_layer_group
            .screen_layer_mut()
            .page_layer_mut()
            .get_plane_mut(plane)
    }

    pub fn create_back_layer_group_if_needed(&mut self, ctx: &mut RenderCloneCtx) {
        // This currently doesn't work because cloning MessageLayer is weird
        // the game just increases the refcount but in rust we would have to wrap it in an `Arc`
        // I am also not yet convinced that "back_layer_group" is not some legacy transition mechanism not actually used by umineko
        // so, do not do anything for now

        // if self.back_layer_group.is_some() {
        //     return;
        // }
        //
        // self.back_layer_group = Some(self.root_layer_group.render_clone(ctx));
    }

    #[tracing::instrument(skip_all)]
    pub fn render(&self, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        render_layer_without_bg(pass, &TransformParams::default(), &self.root_layer_group, 0);
        if let Some(select_layer) = &self.select_layer {
            select_layer.render(pass, &self.messagebox_textures);
        }
    }
}

impl Updatable for AdvState {
    fn update(&mut self, context: &mut UpdateContext) {
        let adv_update_context = AdvUpdateContext {
            frame_id: context.frame_id,
            delta_ticks: context.delta_ticks,
            asset_server: context.asset_server,
            device: context.pre_render.device,
            queue: context.pre_render.queue,
            are_animations_allowed: self.allow_running_animations,
        };

        // this seems like a pre-PAGEBACK feature to stop incomplete transitions from rendering
        // why did they leave it in :/
        if self.allow_running_animations {
            self.back_layer_group = None;
        }

        self.root_layer_group.update(&adv_update_context);

        let transform = TransformParams::default();

        self.root_layer_group
            .pre_render(context.pre_render, &transform);

        if let Some(select_layer) = &mut self.select_layer {
            select_layer.update(context.delta_ticks);
            for sound in select_layer.take_sounds() {
                play_sys_se(
                    &self.audio_manager,
                    context.asset_server,
                    sound,
                    shin_core::vm::command::types::Volume::default(),
                );
            }
        }
    }
}
