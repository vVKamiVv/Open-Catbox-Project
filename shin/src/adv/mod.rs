pub mod assets;
mod command;
pub mod hud;
pub mod menu;
mod restore;
pub(crate) mod unlocks;
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

use menu::{
    AdvRequest, Task, TaskCx, backlog::Backlog, save_notice::SaveNotice, dialog::Dialog, hidden_window::HiddenWindow, jump::JumpDialog,
    system_menu::SystemMenu,
};
use shin_core::{primitives::color::FloatColor4, time::{Tween, Tweener}};
use shin_render::render_texture::RenderTexture;

use crate::{
    adv::assets::{AdvAssets, AdvFonts},
    app::AppAction,
    asset::system::AssetServer,
    audio::{BgmPlayer, SePlayer, VoicePlayer, play_sys_se},
    capture::CaptureTargets,
    layer::{
        AnyLayer, AnyLayerMut, DrawableLayer as _, Layer as _, LayerGroup, PageLayer, RootLayerGroup,
        ScreenLayer,
        message_layer::{MessageLayer, MessageboxTextures},
        apply_backdrop_blur, render_layer_without_bg,
        render_params::TransformParams,
        select_layer::{SelectInput, SelectLayer},
        user::minigame::{self, Directions, MinigameInput, PointerTracker},
        user::UserLayer,
    },
    ui::gesture::{Gesture, GestureRecognizer},
    render::{
        PreRenderContext,
        overlay::{OverlayCollector, OverlayVisitable},
    },
    ui::{self, MenuInput},
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
    /// The story's touch input, with long presses (the ADV listener `0x71000154f0`..`0x7100015dd0`)
    gestures: GestureRecognizer,
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
    /// The task pool (`ADV+0x158`): the windows and menus over the scene, the last one on top
    tasks: Vec<Box<dyn Task>>,
    /// The interface's hide counter (`ADV+0x478`) and the pause counter (`ADV+0x474`), which stops the script, voices
    /// and sounds
    hide_count: u32,
    pause_count: u32,
    /// "Return to title" was chosen and the save is written; the app takes it from here
    exit_to_title: bool,
    /// A bookmark was chosen in the load screen; the app replaces this screen with one that loads it
    /// with whether the new screen crossfades from this one (load mode 0) or cuts (mode 1, rollback)
    load_request: Option<(GameDataEntry, bool)>,
    /// Where the jump dialog on screen goes if confirmed
    pending_jump: Option<GameDataEntry>,
    /// Frames until EXIT leaves for the title
    exit_wait: Option<f32>,
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
        bgm_player: BgmPlayer,
    ) -> Self {
        let scenario = assets.scenario.clone();
        let play_frames = global.play_seconds as f64 * 60.0;
        let (scripter, vm_state, load) = replay(&scenario, global, entry, entry_point);
        let adv_state = AdvState::new(audio_manager, asset_server, assets, bgm_player);

        Self {
            scenario,
            scripter,
            vm_state,
            adv_state,
            current_command: None,
            select_pointer: PointerTracker::default(),
            gestures: GestureRecognizer::new(true),
            fast_forward_to_bp: None,
            script_exited: false,
            capture: None,
            load: load.map(LoadState::Replayed),
            play_frames,
            write_requested: false,
            toggles_held: (false, false),
            tasks: Vec::new(),
            hide_count: 0,
            pause_count: 0,
            exit_to_title: false,
            load_request: None,
            pending_jump: None,
            exit_wait: None,
        }
    }

    /// The current state as a slot entry whose save position is the current line (where a look-ahead starts)
    fn current_entry(&self) -> GameDataEntry {
        self.vm_state.save.entry.clone()
    }

    /// Rebuilds the screen at `save_position` of the current playthrough (`0x710000f220`), crossfading or not
    fn rebuild_at(&mut self, save_position: u32, crossfade: bool) {
        let mut entry = self.current_entry();
        entry.save_position = save_position;
        self.load_request = Some((entry, crossfade));
    }

    /// + (`ADV::JumpNext` `0x7100012c80`): "Skip this chapter?" when the look-ahead finds a stop, else "error"
    fn open_jump_next(&mut self, context: &mut UpdateContext) {
        let stop_unread = self.vm_state.global.settings.v11_disallowskipunread;
        let target = look_ahead(
            &self.scenario,
            self.vm_state.global.clone(),
            self.current_entry(),
            LookAhead::NextStop { stop_unread },
        );
        let Some(target) = target else {
            play_sys_se(&self.adv_state.audio_manager, context.asset_server, "error", Default::default());
            return;
        };
        // the English patch translates only the first text (`0x71001619f6`); with setting v11 on the original
        // shows its Japanese one (`0x710015eedb`)
        let text = if stop_unread {
            "次のチャプター、未読メッセージ、選択肢まで@rスキップします。よろしいですか？"
        } else {
            "Skip this chapter?"
        };
        self.pending_jump = Some(target);
        let dialog = JumpDialog::new(Dialog::yes_no(
            context.pre_render,
            &self.adv_state.menu_assets,
            text,
            1,
        ));
        self.tasks.push(Box::new(dialog));
    }

    /// - (`ADV::JumpPrev` `0x7100012f80`): "Return to the last chapter?"
    fn open_jump_prev(&mut self, context: &mut UpdateContext) {
        let mut target = self.current_entry();
        target.save_position = self.vm_state.save.previous_chapter();
        self.pending_jump = Some(target);
        let dialog = JumpDialog::new(Dialog::yes_no(
            context.pre_render,
            &self.adv_state.menu_assets,
            "Return to the last chapter?",
            1,
        ));
        self.tasks.push(Box::new(dialog));
    }

    /// B inside an event (`0x710000f600`): the look-ahead to the event's end, and the screen rebuilt there
    fn skip_event(&mut self, context: &mut UpdateContext) {
        self.skip_event_with(context, true);
    }

    /// The event skip; `error_sound` plays `error` when there is no event end to skip to (B does, a long press is
    /// silent: `0x7100015c00`)
    fn skip_event_with(&mut self, context: &mut UpdateContext, error_sound: bool) {
        let target = look_ahead(
            &self.scenario,
            self.vm_state.global.clone(),
            self.current_entry(),
            LookAhead::EventEnd,
        );
        let audio = self.adv_state.audio_manager.clone();
        match target {
            Some(target) => {
                info!("Skipping the event to save point {}", target.save_position);
                play_sys_se(&audio, context.asset_server, "decide", Default::default());
                self.load_request = Some((target, false));
            }
            None if error_sound => play_sys_se(&audio, context.asset_server, "error", Default::default()),
            None => {}
        }
    }

    /// Up (`0x71000137f0`): back to the previous line's save point, once two lines have played
    fn roll_back(&mut self, context: &mut UpdateContext) {
        let audio = self.adv_state.audio_manager.clone();
        let save = &self.vm_state.save;
        if save.lines < 2 {
            play_sys_se(&audio, context.asset_server, "error", Default::default());
            return;
        }
        let target = save.line_points.0;
        play_sys_se(&audio, context.asset_server, "decide", Default::default());
        info!("Rolling back to save point {target}");
        // `0x71000137f0` loads with mode 1: no crossfade
        self.rebuild_at(target, false);
    }

    /// The bookmark the player chose to load (once)
    pub fn take_load_request(&mut self) -> Option<(GameDataEntry, bool)> {
        self.load_request.take()
    }

    /// Stops this screen's sounds before it is replaced (the original destroys its players with it)
    /// The sounds and voices stop as the adventure screen goes; the music goes on into the next scene, as the
    /// original's shared player does (`ExitGame $_6` `0x71000187c0` only lifts its duck)
    pub fn shutdown(&mut self) {
        let quick = Tween::linear(Ticks::ZERO);
        self.adv_state.bgm_player.set_gain(self.adv_state.bgm_player.setting_volume(), 1.0);
        self.adv_state.se_player.stop_all(quick);
        self.adv_state.message_layer_mut().stop_voices();
    }

    /// The music player, for the next scene
    pub fn take_bgm(&mut self) -> BgmPlayer {
        let fresh = BgmPlayer::new(self.adv_state.audio_manager.clone());
        std::mem::replace(&mut self.adv_state.bgm_player, fresh)
    }

    /// Whether the player chose "Return to title" (once)
    pub fn take_exit_to_title(&mut self) -> bool {
        std::mem::take(&mut self.exit_to_title)
    }

    /// `0x71000145f0`: the message window and the banners hide while the counter is above 0
    fn hide_interface(&mut self, hide: bool) {
        if hide {
            self.hide_count += 1;
        } else {
            self.hide_count = self.hide_count.saturating_sub(1);
        }
        let hidden = self.hide_count > 0;
        self.adv_state.message_layer_mut().set_hidden(hidden);
        self.adv_state.hud.set_hidden(hidden);
    }

    /// The pause counter (`ADV+0x474`): voices and sounds hold while it is above 0, and the script waits
    fn pause(&mut self, pause: bool) {
        let was_paused = self.pause_count > 0;
        if pause {
            self.pause_count += 1;
        } else {
            self.pause_count = self.pause_count.saturating_sub(1);
        }
        let paused = self.pause_count > 0;
        if paused != was_paused {
            self.adv_state.message_layer_mut().set_voices_paused(paused);
            self.adv_state.se_player.set_paused(paused);
        }
    }

    /// Y (`0x7100013320`): the system menu over the blurred scene, which pauses and hides everything else
    fn open_system_menu(&mut self, context: &mut UpdateContext) {
        play_sys_se(&self.adv_state.audio_manager, context.asset_server, "page", Default::default());
        self.adv_state.skip_auto = 0;
        // `0x7100013a70`
        self.adv_state.capture_backdrop(context.pre_render);
        minigame::tween_now(&mut self.adv_state.backdrop_alpha, 1.0, 0.1);
        self.hide_interface(true);
        self.pause(true);
        let menu = SystemMenu::new(
            context.pre_render,
            &self.adv_state.menu_assets,
            &self.vm_state.save_info.info[1],
            self.vm_state.chars,
            self.vm_state.tips.clone(),
            self.vm_state.global.play_seconds,
        );
        self.tasks.push(Box::new(menu));
    }

    /// X (`ADV::ShowMessageBrowser` `0x7100013400`): the backlog of the current chapter, with "page", or "error"
    /// when it has no line
    fn open_backlog(&mut self, context: &mut UpdateContext) {
        let log = build_log(&self.scenario, self.vm_state.global.clone(), self.current_entry());
        let audio = self.adv_state.audio_manager.clone();
        if log.entries.is_empty() {
            play_sys_se(&audio, context.asset_server, "error", Default::default());
            return;
        }
        let task = {
            let mut requests = Vec::new();
            let cx = TaskCx {
                frames: 0.0,
                focused: false,
                input: MenuInput::released(Default::default()),
                pre_render: &mut *context.pre_render,
                asset_server: context.asset_server,
                audio: &audio,
                assets: context.asset_server,
                global: &mut self.vm_state.global,
                requests: &mut requests,
            };
            Backlog::new(&cx, &self.adv_state.menu_assets, log)
        };
        let Some(backlog) = task else {
            play_sys_se(&audio, context.asset_server, "error", Default::default());
            return;
        };
        play_sys_se(&audio, context.asset_server, "page", Default::default());
        self.adv_state.skip_auto = 0;
        // `0x7100013a70`, as for the system menu
        self.adv_state.capture_backdrop(context.pre_render);
        minigame::tween_now(&mut self.adv_state.backdrop_alpha, 1.0, 0.1);
        self.hide_interface(true);
        self.pause(true);
        self.tasks.push(Box::new(backlog));
    }

    /// B (`0x7100013720`): hides the message window and the banners until the next press
    fn open_hidden_window(&mut self) {
        self.adv_state.skip_auto = 0;
        self.hide_interface(true);
        self.tasks.push(Box::new(HiddenWindow::default()));
    }

    /// Runs the task pool; true when a task has the input this frame
    fn update_tasks(
        &mut self,
        context: &mut UpdateContext,
        input_state: &EnumMap<AppAction, ActionState>,
        pointer: Pointer,
    ) -> bool {
        if self.tasks.is_empty() {
            return false;
        }
        let mut requests = Vec::new();
        let top = self.tasks.len() - 1;
        let input = MenuInput::new(input_state, pointer);
        let audio = self.adv_state.audio_manager.clone();
        let frames = context.delta_ticks.as_f32();
        let pre_render = &mut *context.pre_render;
        let asset_server = context.asset_server;
        let global = &mut self.vm_state.global;
        let mut index = 0;
        self.tasks.retain_mut(|task| {
            let focused = index == top;
            index += 1;
            task.update(&mut TaskCx {
                frames,
                focused,
                input: if focused { input } else { MenuInput::released(pointer) },
                pre_render: &mut *pre_render,
                asset_server,
                audio: &audio,
                assets: asset_server,
                global,
                requests: &mut requests,
            })
        });

        for request in requests {
            match request {
                AdvRequest::Unhide => self.hide_interface(false),
                AdvRequest::SystemMenuClosed => {
                    // `0x7100014570`
                    self.pause(false);
                    self.hide_interface(false);
                    minigame::tween_now(&mut self.adv_state.backdrop_alpha, 0.0, 0.1);
                }
                AdvRequest::WriteSave => self.write_requested = true,
                AdvRequest::JumpTo(save_position) => {
                    info!("Jumping from the backlog to save point {save_position}");
                    self.rebuild_at(save_position, true);
                }
                AdvRequest::Jump => {
                    // `$_4` / `$_2`: only with the system still enabled
                    if self.vm_state.save.system_enabled {
                        if let Some(target) = self.pending_jump.take() {
                            info!("Jumping to save point {}", target.save_position);
                            self.load_request = Some((target, true));
                        }
                    }
                }
                // the save menu's notice writes the file
                AdvRequest::SaveToSlot(slot) => self.save_to_slot(slot),
                AdvRequest::LoadSlot(slot) => {
                    let global = &mut self.vm_state.global;
                    if let Some(data) = &global.manual_save_slots[slot] {
                        // the position is remembered in memory only (`0x71000db120`)
                        global.save_menu_position = slot as u8;
                        self.load_request = Some((data.entry.clone(), true));
                    }
                }
                AdvRequest::ExitToTitle => {
                    info!("Returning to the title");
                    let notice = SaveNotice::new(
                        context.pre_render,
                        &self.adv_state.menu_assets,
                        Some(AdvRequest::LeaveForTitle),
                    );
                    self.tasks.push(Box::new(notice));
                }
                AdvRequest::LeaveForTitle => self.exit_to_title = true,
            }
        }
        true
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
        self.tasks.is_empty()
            && save.system_enabled
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
        for task in &self.tasks {
            task.render(pass);
        }
    }

    pub fn handle_input(
        &mut self,
        context: &mut UpdateContext,
        state: EnumMap<AppAction, ActionState>,
        pointer: Pointer,
        gestures: &[Gesture],
    ) {
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
            self.select_pointer
                .dispatch(pointer, context.delta_ticks.as_f32(), select_layer);
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

        // a long press inside an event skips it, like B (`0x7100015c00`); a hold elsewhere does nothing
        let save = &self.vm_state.save;
        if gestures.contains(&Gesture::LongPress)
            && save.system_enabled
            && save.event.is_some()
            && self.load_request.is_none()
        {
            self.skip_event_with(context, false);
            return;
        }
        let tapped = gestures.iter().any(|g| matches!(g, Gesture::Tap(_)));

        // with the system enabled and auto and skip off (`0x7100011f08`): inside an event B skips it; outside, + and -
        // jump between chapters, Y opens the system menu and B hides the window
        if save.system_enabled && self.adv_state.skip_auto == 0 && self.load_request.is_none() {
            if save.event.is_some() {
                if state[AppAction::Cancel].is_clicked {
                    self.skip_event(context);
                    return;
                }
            } else {
                if state[AppAction::Plus].is_clicked {
                    self.open_jump_next(context);
                    return;
                }
                if state[AppAction::Minus].is_clicked {
                    self.open_jump_prev(context);
                    return;
                }
                if state[AppAction::ButtonY].is_clicked {
                    self.open_system_menu(context);
                    return;
                }
                if state[AppAction::ButtonX].is_clicked {
                    self.open_backlog(context);
                    return;
                }
                if state[AppAction::Cancel].is_clicked {
                    self.open_hidden_window();
                    return;
                }
            }
        }
        if state[AppAction::AnyUp].is_clicked_or_repeated && self.load_request.is_none() {
            self.roll_back(context);
            return;
        }

        // inside an event (system enabled), confirm shows "Press B to Skip." (`0x7100011f2c`), a tap the touch
        // screen's hint (`0x71000157d0`)
        let save = &self.vm_state.save;
        if save.event.is_some() && save.system_enabled && (state[AppAction::Enter].is_clicked || tapped) {
            self.adv_state.hud.show_event_hint(!state[AppAction::Enter].is_clicked);
            return;
        }

        // a tap (a click released without a drag or a long press) advances the text, as in the original
        if state[AppAction::Enter].is_clicked || tapped {
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
        self.adv_state.frame_start = std::time::Instant::now();

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

        // a task in the pool takes the input; the ADV's buttons only work without one (`0x7100011ea0`)
        let task_has_input = self.update_tasks(context, &input_state, pointer);
        let gestures = self.gestures.update(pointer, context.delta_ticks.as_f32());
        if task_has_input {
            // a touch that began over a menu stays the menu's
            self.gestures.cancel();
        } else if !self.handle_mode_buttons(&input_state, context.asset_server) {
            self.handle_input(context, input_state, pointer, &gestures);
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
                            self.script_exited = true;
                            self.exit_wait = Some(30.0);
                        }
                    },
                }
            }
        }

        // while the system menu is up the script waits (`ADV+0x474`)
        while !self.script_exited && self.pause_count == 0 {
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
                    info!("Script reached EXIT at {:?}", self.scripter.position());
                    self.script_exited = true;
                    self.exit_wait = Some(30.0);
                }
            }
        }

        self.adv_state.update(context);

        // EXIT: after 30 frames and once the box has closed, the save is written and the title opens
        // (`0x7100004b10`, `0x71000147e0`)
        if let Some(wait) = &mut self.exit_wait {
            *wait -= context.delta_ticks.as_f32();
            if *wait <= 0.0 && self.adv_state.message_layer().is_closed() && self.tasks.is_empty() {
                self.exit_wait = None;
                info!("The piece is over; returning to the title");
                let notice = SaveNotice::new(
                    context.pre_render,
                    &self.adv_state.menu_assets,
                    Some(AdvRequest::LeaveForTitle),
                );
                self.tasks.push(Box::new(notice));
            }
        }

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

/// What the save and load screens show of a bookmark: its SAVEINFO texts (levels 0 and 1) and its scene
pub struct Bookmark {
    pub info: [String; 2],
    pub layers: vm_state::layers::LayersState,
}

/// Replays a bookmark to its save point as a load does, for the save and load screens (`ThumbMaker::StateBuilder`)
pub(crate) fn replay_bookmark(scenario: &Scenario, global: SaveData, entry: GameDataEntry) -> Bookmark {
    let (_, vm_state, _) = replay(scenario, global, entry, None);
    let [first, second, ..] = vm_state.save_info.info;
    Bookmark {
        info: [first, second],
        layers: vm_state.layers,
    }
}

/// What the backlog lists, in order (`MessageBrowser::LogBuilder`, ctor `0x71000a3280`)
pub struct Log {
    pub entries: Vec<LogEntry>,
    /// The line on screen in the game (`+0xa4`)
    pub current: usize,
}

pub struct LogEntry {
    pub kind: LogKind,
    /// The text, with its markup
    pub text: String,
    /// The save point a jump to this entry rebuilds at
    pub save_position: u32,
    /// The chapter list's label of a marker: the choice's prompt, the chapter's second title or the minigame's name
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Message { voiced: bool },
    Choice,
    Chapter,
    Quiz,
}

impl LogKind {
    /// A marker that L and R move between
    pub fn is_marker(self) -> bool {
        !matches!(self, LogKind::Message { .. })
    }
}

/// The backlog of `entry`'s chapter (`0x71000a2780`): its script replayed from the start through the current line
/// and on through text already read, stopping at the first unread line (with setting v11), a choice or minigame not
/// played yet, or the end
pub(crate) fn build_log(scenario: &Scenario, global: SaveData, entry: GameDataEntry) -> Log {
    let target = entry.save_position;
    let stop_unread = global.settings.v11_disallowskipunread;
    let mut scripter = Scripter::new(scenario, entry.scenario_id, entry.random_seed);
    let mut vm_state = VmState::new(global, entry);
    // the stops are this builder's own; replay_command passes every save point
    vm_state.save.target = u32::MAX;
    let mut log = Log {
        entries: Vec::new(),
        current: 0,
    };
    let mut result = CommandResult::None;
    loop {
        let Ok(command) = scripter.run(result) else {
            break;
        };
        let past = vm_state.save.counter >= target;
        result = match command {
            RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_) => break,
            RuntimeCommand::SGET(sget) => {
                // the route before the save point, the persistent data after it (`0x71000ab130`)
                let save = &mut vm_state.save;
                let value = if past {
                    save.sget(&vm_state.global.persist_data, sget.slot_number)
                } else {
                    save.replay_sget()
                };
                sget.token.finish(value)
            }
            RuntimeCommand::MSGSET(msgset) => {
                let id = msgset.msg_id.0 as i64 - 1;
                let read = id < 0
                    || vm_state
                        .global
                        .save_vectors
                        .seen_messages
                        .contains(id as usize);
                let text = msgset.text.clone();
                let voiced = shin_core::layout::MessageTextParser::new(&text)
                    .any(|c| matches!(c, shin_core::layout::ParsedCommand::Voice(_)));
                let step = command::replay_command(RuntimeCommand::MSGSET(msgset), &mut vm_state);
                // `0x71000ab210` compares the counter after this line's step: the line on screen is the target
                // itself, so the backlog's current line is the one before it
                let counter = vm_state.save.counter;
                if counter >= target && stop_unread && !read {
                    break;
                }
                if counter < target {
                    log.current = log.entries.len();
                }
                log.entries.push(LogEntry {
                    kind: LogKind::Message { voiced },
                    text,
                    save_position: vm_state.save.counter,
                    label: String::new(),
                });
                match step {
                    ReplayStep::Continue(r) => r,
                    _ => break,
                }
            }
            RuntimeCommand::SELECT(select) => {
                // shortcut: past the save point the original previews the last choice made there; the port stops
                if past {
                    break;
                }
                let title = select.choice_title.clone();
                let variants = select.variants.clone();
                let step = command::replay_command(RuntimeCommand::SELECT(select), &mut vm_state);
                let ReplayStep::Continue(r) = step else { break };
                if let CommandResult::WriteMemory(_, pick) = r {
                    // a leading "*...." in the option is stripped
                    let option = variants.get(pick as usize).map_or("", |v| v.as_str());
                    let option = option
                        .strip_prefix('*')
                        .and_then(|rest| rest.split_once('.').map(|(_, text)| text))
                        .unwrap_or(option);
                    log.entries.push(LogEntry {
                        kind: LogKind::Choice,
                        text: format!("@r《{title}》@r{option}"),
                        save_position: vm_state.save.counter,
                        label: title.to_string(),
                    });
                }
                r
            }
            RuntimeCommand::QUIZ(quiz) => {
                if past {
                    break;
                }
                let step = command::replay_command(RuntimeCommand::QUIZ(quiz), &mut vm_state);
                let ReplayStep::Continue(r) = step else { break };
                // shortcut: the original names the minigame and its result; the port labels every one a quiz
                log.entries.push(LogEntry {
                    kind: LogKind::Quiz,
                    text: "@r《クイズ》".to_string(),
                    save_position: vm_state.save.counter,
                    // "クイズ" (`0x710015dd7a`)
                    label: crate::asset::executable::text_at(0x710015dd7a)
                        .unwrap_or_else(|| "クイズ".to_string()),
                });
                r
            }
            RuntimeCommand::AUTOSAVE(autosave) => {
                let step = command::replay_command(RuntimeCommand::AUTOSAVE(autosave), &mut vm_state);
                let [first, second, ..] = &vm_state.save_info.info;
                log.entries.push(LogEntry {
                    kind: LogKind::Chapter,
                    text: format!("@r{first}@r{second}"),
                    save_position: vm_state.save.counter,
                    label: second.to_string(),
                });
                match step {
                    ReplayStep::Continue(r) => r,
                    _ => break,
                }
            }
            command => match command::replay_command(command, &mut vm_state) {
                ReplayStep::Continue(r) => r,
                _ => break,
            },
        };
    }
    log.current = log.current.min(log.entries.len().saturating_sub(1));
    log
}

/// What a look-ahead advances to
enum LookAhead {
    /// `0x710000f4f0`: the next chapter (AUTOSAVE), choice or minigame, or with `stop_unread` an unread line
    NextStop { stop_unread: bool },
    /// `0x710000f600`: the end of the current event
    EventEnd,
}

/// Replays `entry` to its save point (the current line), then keeps going over the commands a jump passes (their
/// slot 3, "skippable") to the first that stops it; the slot entry that rebuilds there, or `None` when the script
/// ends first
fn look_ahead(
    scenario: &Scenario,
    global: SaveData,
    entry: GameDataEntry,
    kind: LookAhead,
) -> Option<GameDataEntry> {
    let (mut scripter, mut vm_state, end) = replay(scenario, global, entry, None);
    vm_state.save.target = u32::MAX;
    let mut result = match end? {
        LoadEnd::Resume(result) => result,
        // the current line itself is passed
        LoadEnd::Stopped(command) => match command::replay_command(command, &mut vm_state) {
            ReplayStep::Continue(result) => result,
            _ => return None,
        },
    };
    loop {
        let command = scripter.run(result).ok()?;
        let save = &vm_state.save;
        // the counter value a replay stops at for this command
        let stop = match (&command, &kind) {
            (RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_), _) => return None,
            (RuntimeCommand::EVEND(_), LookAhead::EventEnd) => Some(save.counter + 1),
            (RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_), _) => {
                // a choice under its question line rebuilds at that line
                Some(save.counter + !save.message_current as u32)
            }
            (RuntimeCommand::AUTOSAVE(_), LookAhead::NextStop { .. }) => Some(save.counter + 1),
            (RuntimeCommand::MSGSET(msgset), LookAhead::NextStop { stop_unread: true }) => {
                let id = msgset.msg_id.0 as i64 - 1;
                let read = id < 0
                    || vm_state
                        .global
                        .save_vectors
                        .seen_messages
                        .contains(id as usize);
                (save.event.is_none() && !read).then_some(save.counter + 1)
            }
            _ => None,
        };
        if let Some(target) = stop {
            let mut entry = vm_state.save.entry.clone();
            entry.save_position = target;
            return Some(entry);
        }
        result = match command {
            // played live, so the rebuild's replay reads the same values from the route
            RuntimeCommand::SGET(sget) => {
                let value = vm_state
                    .save
                    .sget(&vm_state.global.persist_data, sget.slot_number);
                sget.token.finish(value)
            }
            command => match command::replay_command(command, &mut vm_state) {
                ReplayStep::Continue(result) => result,
                _ => return None,
            },
        };
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

impl OverlayVisitable for Adv {
    fn visit_overlay(&self, collector: &mut OverlayCollector) {
        collector.overlay(
            "ADV",
            |_ctx, top_left| {
                let command = match &self.current_command {
                    // a command's fields can hold a whole line of text
                    Some(command) => Cow::Owned(format!("{command:?}").chars().take(120).collect()),
                    None => Cow::Borrowed("None"),
                };
                let save = &self.vm_state.save;
                let skip_auto = self.adv_state.skip_auto;
                top_left.label(format!(
                    "Script {:08x} {command}
Message {}  save counter {}  event {:?}
auto {}  skip {}  skipping {}",
                    self.scripter.position().0,
                    self.adv_state.root_layer_group.message_layer().message_id().0,
                    save.counter,
                    save.event,
                    skip_auto & AUTO != 0,
                    skip_auto & SKIP != 0,
                    self.adv_state.skipping,
                ));
            },
            false,
        );
        collector.overlay(
            "Layers",
            |ctx, _top_left| {
                let plane = self.vm_state.layers.current_plane;
                let group = self
                    .adv_state
                    .root_layer_group
                    .screen_layer()
                    .page_layer()
                    .get_plane(plane);
                Window::new("Layers").default_pos([8.0, 160.0]).show(ctx, |ui| {
                    ui.monospace(format!("Plane {}", plane.raw()));
                    let banks = group
                        .get_used_layerbank_ids()
                        .into_iter()
                        .map(|bank| (&self.vm_state.layers.layerbanks[bank], bank))
                        .sorted_by_key(|(state, _)| state.layer_id.raw());
                    for (state, bank) in banks {
                        ui.monospace(format!(
                            "layer {:>3} {:?} (bank {})",
                            state.layer_id.raw(),
                            state.layer_type,
                            bank.raw()
                        ));
                        let Some(layer) = group.get_layer(bank) else { continue };
                        for (property, tweener) in layer.properties().changed() {
                            let running = if tweener.is_idle() { "" } else { " tweening" };
                            ui.monospace(format!(
                                "    {property:?}: {} -> {}{running}",
                                tweener.value(),
                                tweener.target_value()
                            ));
                        }
                    }
                });
            },
            false,
        );
    }
}

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
    /// When this frame's update began (the frame clock `0x71000fc290`), for LAYERLOAD's short wait
    pub frame_start: std::time::Instant,
    /// What the screen draws over its layers: banners, icons, notifications
    pub hud: hud::AdvHud,
    pub menu_assets: menu::MenuAssets,
    /// The blurred capture of the scene behind the system menu (`ADV+0x190`) and its alpha (`ADV+0x198`)
    backdrop: Option<RenderTexture>,
    pub backdrop_alpha: Tweener,
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
        bgm_player: BgmPlayer,
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
            frame_start: std::time::Instant::now(),
            hud: hud::AdvHud::new(assets.hud_textures.clone(), assets.fonts.system_font.clone()),
            menu_assets: menu::MenuAssets::new(&assets),
            backdrop: None,
            backdrop_alpha: Tweener::new(0.0),
            back_layer_group: None,
            audio_manager: audio_manager.clone(),
            bgm_player,
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

    /// `0x7100013af0`: the scene without the message window and the overlay, rendered and blurred, for the system
    /// menu's backdrop
    pub fn capture_backdrop(&mut self, ctx: &mut PreRenderContext) {
        let mut texture = ctx.new_render_texture("backdrop".to_string());
        let mut scratch = ctx.new_render_texture("backdrop/scratch".to_string());
        let depth_stencil = ctx.depth_stencil;
        {
            let mut pass = ctx.begin_pass(texture.as_texture_target(), Some(depth_stencil), "backdrop");
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            render_layer_without_bg(&mut pass, &TransformParams::default(), self.screen_layer(), 0);
        }
        apply_backdrop_blur(ctx, &mut texture, &mut scratch);
        self.backdrop = Some(texture);
    }

    #[tracing::instrument(skip_all)]
    pub fn render(&self, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        let backdrop_alpha = self.backdrop_alpha.value();
        let backdrop = self.backdrop.as_ref().filter(|_| backdrop_alpha > 0.0);
        // with the backdrop fully up the scene is not drawn (ADV render)
        if backdrop.is_none() || backdrop_alpha < 1.0 {
            render_layer_without_bg(pass, &TransformParams::default(), &self.root_layer_group, 0);
            if let Some(select_layer) = &self.select_layer {
                select_layer.render(pass, &self.messagebox_textures);
            }
        }
        if let Some(backdrop) = backdrop {
            draw_backdrop(pass, backdrop, backdrop_alpha);
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

        ui::advance(&mut self.backdrop_alpha, context.delta_ticks.as_f32());
        if self.backdrop_alpha.is_idle() && self.backdrop_alpha.value() <= 0.0 {
            self.backdrop = None;
        }

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

/// The backdrop over the whole screen at half brightness, faded by `alpha`: colour (0.5a, 0.5a, 0.5a, a) with blend 5
/// (premultiplied) while it fades, opaque once it is up (ADV render `0x71000126b4`)
fn draw_backdrop(pass: &mut RenderPass, backdrop: &RenderTexture, alpha: f32) {
    use shin_render::{
        DrawPrimitive, RenderProgramWithArguments,
        shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
    };
    let grey = 0.5 * alpha;
    let color = FloatColor4::from_rgba(grey, grey, grey, alpha).into_unorm();
    let blend = if alpha < 1.0 {
        shin_render::ColorBlendType::LayerPremultiplied1
    } else {
        shin_render::ColorBlendType::Opaque
    };
    let vertex = |x: f32, y: f32| PosColTexVertex {
        position: glam::vec3(x * 1920.0, y * 1080.0, 0.0),
        color,
        texture_position: glam::vec2(x, y),
    };
    let vertices = [vertex(0.0, 0.0), vertex(1.0, 0.0), vertex(0.0, 1.0), vertex(1.0, 1.0)];
    pass.run(shin_render::RenderRequestBuilder::new().color_blend_type(blend).build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: backdrop.as_texture_source(),
            transform: ui::projection(),
        },
        DrawPrimitive::TrianglesStrip,
    ));
}
