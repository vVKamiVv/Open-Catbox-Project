pub mod assets;
mod command;
pub mod hud;
mod restore;
mod unlocks;
mod vm_state;

use std::{borrow::Cow, sync::Arc};

pub use command::{CommandStartResult, ExecutingCommand, StartableCommand, UpdatableCommand};
use command::ReplayStep;
use egui::Window;
use enum_map::{Enum, EnumMap, enum_map};
use glam::Mat4;
use itertools::Itertools;
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::{GameData, GameDataEntry, SaveData, Settings},
        scenario::{Scenario, instruction_elements::CodeAddress},
    },
    primitives::color::UnormColor,
    time::Ticks,
    vm::{
        Scripter,
        breakpoint::BreakpointObserver,
        command::{
            CommandResult, RuntimeCommand,
            types::{AudioWaitStatus, LayerId, PLANES_COUNT, PlaneId, VLayerId, VLayerIdRepr},
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
    asset::system::AssetServer,
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
    /// Set by a load until the scene has been rebuilt from the replay
    load: Option<LoadState>,
    /// The play time in 60 Hz frames (`*0x71002267d0`), counted into the save's play seconds
    play_frames: f64,
    /// The save file should be written (the original writes it when leaving the adventure screen)
    write_requested: bool,
    /// L and R as last frame, as the original toggles auto and skip on release
    toggles_held: (bool, bool),
}

/// Where a load's replay left the script
enum LoadEnd {
    /// Before this command, which starts live once the scene is rebuilt
    Stopped(RuntimeCommand),
    /// After a replayed choice; the VM goes on with this result
    Resume(CommandResult),
}

enum LoadState {
    Replayed(LoadEnd),
    Restoring(restore::SceneRestore, LoadEnd),
}

impl Adv {
    /// Starts `entry`: a new game when its save position is 0, else a load, which replays the script to the save
    /// point first (`0x7100019930`, the replay job `0x710001b340`)
    pub fn new(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<AssetServer>,
        assets: AdvAssets,
        global: SaveData,
        entry: GameDataEntry,
        entry_point: Option<CodeAddress>,
    ) -> Self {
        let scenario = assets.scenario.clone();
        let play_frames = global.play_seconds as f64 * 60.0;
        let (scripter, vm_state, load) = replay(&scenario, global, entry, entry_point);
        let adv_state = AdvState::new(audio_manager, asset_server, assets);

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
            load: load.map(LoadState::Replayed),
            play_frames,
            write_requested: false,
            toggles_held: (false, false),
        }
    }

    /// The save file's contents
    pub fn save_data(&self) -> &SaveData {
        &self.vm_state.global
    }

    /// Whether the save file should be written now; clears the request
    pub fn take_write_request(&mut self) -> bool {
        std::mem::take(&mut self.write_requested)
    }

    /// A manual save (`0x71000da920`): the last save point goes to `slot` (0-based), which the save menu then
    /// opens at; the caller writes the file
    pub fn save_to_slot(&mut self, slot: usize) {
        let global = &mut self.vm_state.global;
        global.manual_save_slots[slot] = Some(GameData {
            date_time: chrono::Local::now().naive_local(),
            entry: self.vm_state.save.entry.clone(),
        });
        global.save_menu_position = slot as u8;
        info!(
            "Saved to slot {} at save point {}",
            slot + 1,
            self.vm_state.save.entry.save_position
        );
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

    /// `isSkipping` (`0x7100012430`): the skip mode is on (or the PC's Ctrl is held), the system is enabled, no event
    /// plays, nothing waits for the player, and the line was read before unless setting v11 lets unread lines pass;
    /// debug fast-forwarding skips regardless
    fn is_fast_forwarding(&self, button_held: bool) -> bool {
        if self.fast_forward_to_bp.is_some()
            || self
                .capture
                .as_ref()
                .is_some_and(CaptureTargets::is_seeking)
        {
            return true;
        }
        let skip_on = self.adv_state.skip_auto & SKIP != 0 || button_held;
        let save = &self.vm_state.save;
        skip_on
            && self.modes_allowed()
            && (!self.vm_state.global.settings.v11_disallowskipunread || save.line_read)
    }

    /// `isAuto` (`0x71000122f0`): the same gates as skipping, without the read test
    fn is_auto(&self) -> bool {
        self.adv_state.skip_auto & AUTO != 0 && self.modes_allowed()
    }

    /// What both modes need: the system enabled, no event, no choice or minigame waiting for the player
    fn modes_allowed(&self) -> bool {
        let save = &self.vm_state.save;
        save.system_enabled
            && save.event.is_none()
            && self.adv_state.select_layer.is_none()
            && !matches!(self.current_command, Some(ExecutingCommand::QUIZ(_)))
    }

    /// The ADV's mode buttons (`0x7100011f08`): L toggles auto and R toggles skip when released (sounds `son` on,
    /// `cancel` off); while either mode is on, X, Y or cancel turn both off. Returns whether the press was used
    fn handle_mode_buttons(
        &mut self,
        state: &EnumMap<AppAction, ActionState>,
        assets: &AssetServer,
    ) -> bool {
        let held = (
            state[AppAction::AutoToggle].is_held,
            state[AppAction::SkipToggle].is_held,
        );
        let released = (
            self.toggles_held.0 && !held.0,
            self.toggles_held.1 && !held.1,
        );
        self.toggles_held = held;

        let audio = self.adv_state.audio_manager.clone();
        let sound = |on: bool| {
            play_sys_se(&audio, assets, if on { "son" } else { "cancel" }, Default::default())
        };
        if self.vm_state.save.system_enabled {
            for (was_released, bit) in [(released.0, AUTO), (released.1, SKIP)] {
                if was_released {
                    self.adv_state.skip_auto ^= bit;
                    sound(self.adv_state.skip_auto & bit != 0);
                    return true;
                }
            }
        }
        if self.adv_state.skip_auto != 0
            && (state[AppAction::ButtonX].is_clicked
                || state[AppAction::ButtonY].is_clicked
                || state[AppAction::Cancel].is_clicked)
        {
            self.adv_state.skip_auto = 0;
            sound(false);
            return true;
        }
        false
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

        // inside an event (system enabled), confirm shows "Press B to Skip." (`0x7100011f2c`)
        let save = &self.vm_state.save;
        if save.event.is_some() && save.system_enabled && state[AppAction::Enter].is_clicked {
            self.adv_state.hud.show_event_hint();
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
        if !self.handle_mode_buttons(&input_state, context.asset_server) {
            self.handle_input(input_state, pointer, true);
        }
        self.adv_state.skipping = self.is_fast_forwarding(fast_forward_button_held);
        if self.is_auto() {
            self.adv_state.message_layer_mut().request_autoplay();
        }

        if self.is_fast_forwarding(fast_forward_button_held) {
            self.adv_state.root_layer_group_mut().fast_forward();
            if let Some(back_layer_group) = &mut self.adv_state.back_layer_group {
                back_layer_group.fast_forward();
            }
        }

        // the subsystems read the settings from the save data, as the original does each time it needs one
        let settings = self.vm_state.global.settings;
        self.adv_state
            .apply_settings(&settings, context.delta_ticks);

        // `0x7100026d50`: the play time counts every frame
        self.play_frames += context.delta_ticks.as_f32() as f64;
        self.vm_state.global.play_seconds = (self.play_frames / 60.0).min(u32::MAX as f64) as u32;

        let mut result = CommandResult::None;
        match self.load.take() {
            None => {}
            Some(LoadState::Replayed(end)) => {
                // the rebuilt layers unlock their pictures and movies again, as LAYERLOAD does
                for (_, bank) in &self.vm_state.layers.layerbanks {
                    if let Some(layer_type) = bank.layer_type {
                        unlocks::layer_loaded(
                            &mut self.vm_state.global.save_vectors,
                            &self.scenario,
                            layer_type,
                            bank.params,
                        );
                    }
                }
                let restore = restore::SceneRestore::start(
                    context,
                    &self.scenario,
                    &self.vm_state,
                    &mut self.adv_state,
                );
                self.load = Some(LoadState::Restoring(restore, end));
                self.adv_state.update(context);
                return;
            }
            Some(LoadState::Restoring(mut restore, end)) => {
                if !restore.poll(&mut self.adv_state) {
                    self.load = Some(LoadState::Restoring(restore, end));
                    self.adv_state.update(context);
                    return;
                }
                info!("Loaded; playing from {:?}", self.scripter.position());
                match end {
                    LoadEnd::Resume(resume) => result = resume,
                    LoadEnd::Stopped(command) => match command::apply_command_state_and_start(
                        command,
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
                            self.write_requested = true;
                            self.script_exited = true;
                        }
                    },
                }
            }
        }

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
                    &mut self.vm_state,
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
                    self.write_requested = true;
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

        let settings = &self.vm_state.global.settings;
        let modes = hud::HudModes {
            skip_on: self.adv_state.skip_auto & SKIP != 0,
            auto_on: self.adv_state.skip_auto & AUTO != 0,
            skipping: self.adv_state.skipping,
            auto: self.is_auto(),
            skip_read_only: settings.v11_disallowskipunread,
            show_scene_title: settings.v14_showscenetitle,
            show_song_title: settings.v15_showsongtitle,
        };
        let audio = self.adv_state.audio_manager.clone();
        self.adv_state
            .hud
            .update(context.delta_ticks.as_f32(), modes, &audio, context.asset_server);

        // `0x7100014f50`: a finished line is marked read (by the script's id - 1)
        if let Some(id) = self.adv_state.message_layer_mut().take_finished() {
            if let Some(index) = id.0.checked_sub(1) {
                self.vm_state
                    .global
                    .save_vectors
                    .seen_messages
                    .add(index as usize);
            }
        }

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

/// Replays the script of `entry` to its save point (`0x710001b340`); for a new game just sets the VM up
fn replay(
    scenario: &Scenario,
    mut global: SaveData,
    entry: GameDataEntry,
    entry_point: Option<CodeAddress>,
) -> (Scripter, VmState, Option<LoadEnd>) {
    let mut target = entry.save_position;
    let mut sset_threshold = target;
    loop {
        let mut scripter = Scripter::new(scenario, entry.scenario_id, entry.random_seed);
        if let Some(address) = entry_point {
            debug!("Starting execution from {:?}", address);
            scripter.unsafe_set_position(address);
        }
        let mut vm_state = VmState::new(
            global,
            GameDataEntry {
                save_position: target,
                ..entry.clone()
            },
        );
        vm_state.save.sset_threshold = sset_threshold;
        if !vm_state.save.is_replaying() {
            return (scripter, vm_state, None);
        }

        let started = std::time::Instant::now();
        let mut result = CommandResult::None;
        let end = loop {
            let command = scripter.run(result).expect("scripter run failed");
            match command::replay_command(command, &mut vm_state) {
                ReplayStep::Continue(r) => result = r,
                ReplayStep::Stop(command) => break LoadEnd::Stopped(command),
                ReplayStep::Done(r) => break LoadEnd::Resume(r),
            }
        };
        let save = &vm_state.save;
        info!(
            "Replayed to save point {} of {} at {:?} in {:?}",
            save.counter,
            target,
            scripter.position(),
            started.elapsed()
        );

        // a save made inside an event, or a replay that ran into the end, resumes at the last save point before it
        // (`0x710000f210`)
        let (in_event, exited) = match &end {
            LoadEnd::Stopped(RuntimeCommand::EVBEGIN(_)) => (true, save.exit_seen),
            LoadEnd::Stopped(RuntimeCommand::EVEND(_)) => (false, save.exit_seen),
            LoadEnd::Stopped(RuntimeCommand::EXIT(exit)) => {
                (save.event.is_some(), save.exit_seen || exit.arg1 == 0)
            }
            _ => (save.event.is_some(), save.exit_seen),
        };
        let fallback = save.last_save_point;
        if !(in_event || exited) || fallback >= target {
            return (scripter, vm_state, Some(end));
        }
        warn!(
            "The save point is inside an event or past the end; loading the save point before it ({fallback})"
        );
        sset_threshold = save.counter;
        target = fallback;
        global = vm_state.global;
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

/// The bits of [`AdvState::skip_auto`]
const AUTO: u8 = 1;
const SKIP: u8 = 2;

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
    /// The voice-focus factor for music and looping sounds (1, or down to 0.5 while a voice plays)
    voice_duck: f32,
    /// The player's modes (`ADV+0x47c`): bit 0 auto, bit 1 skip
    pub skip_auto: u8,
    /// Whether the script is being skipped this frame (`isSkipping` `0x7100012430`), for the commands as they start
    pub skipping: bool,
    /// What the screen draws over its layers: banners, icons, notifications
    pub hud: hud::AdvHud,
}

impl AdvState {
    /// Hands the player's settings to the message layer and the sound players, and moves the voice focus: with
    /// setting v4, music and looping sounds go down to half while a voice plays, by 0.04 per frame
    /// (`0x7100010fb0`)
    pub fn apply_settings(&mut self, settings: &Settings, delta: Ticks) {
        self.root_layer_group.message_layer_mut().set_settings(*settings);

        let voice_playing = self
            .root_layer_group
            .message_layer()
            .voice_wait_status()
            .contains(AudioWaitStatus::PLAYING);
        let target = if settings.v4_voicefocus && voice_playing { 0.5 } else { 1.0 };
        let step = 0.04 * delta.as_f32();
        self.voice_duck = if self.voice_duck < target {
            (self.voice_duck + step).min(target)
        } else {
            (self.voice_duck - step).max(target)
        };

        self.bgm_player.set_gain(settings.v0_bgmvol, self.voice_duck);
        self.se_player.set_gain(settings.v1_sfxvol, self.voice_duck);
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        crate::audio::sys_se::set_movie_setting_volumes(
            settings.v0_bgmvol,
            settings.v1_sfxvol,
            settings.v2_voicevol,
        );
    }

    pub fn new(
        audio_manager: Arc<AudioManager>,
        asset_server: Arc<AssetServer>,
        assets: AdvAssets,
    ) -> Self {
        Self {
            root_layer_group: RootLayerGroup::new(
                assets.fonts.clone(),
                assets.messagebox_textures.clone(),
                VoicePlayer::new(audio_manager.clone(), asset_server),
            ),
            voice_duck: 1.0,
            skip_auto: 0,
            skipping: false,
            hud: hud::AdvHud::new(assets.hud_textures.clone(), assets.fonts.system_font.clone()),
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
        self.hud.render(pass);
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
