use std::fmt::{Debug, Formatter};

use shin_core::vm::command::types::{LayerId, Volume};
use tracing::{debug, info};

use super::prelude::*;
use crate::{audio::play_sys_se, layer::user::minigame::MinigameInput};

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::QUIZ`, constructor `0x710000dcc0`, execute
/// `0x710000e020`): waits on the minigame loaded at layer `arg` of the current plane (the quiz layer type: the quiz,
/// the cake, the culprit game or the last choice) until it has an answer, then writes the answer to `dest`. While it
/// waits, the minigame's input handler (its `*OverlayLayer`) takes the controller every frame.
///
/// The original also records the answer for save data: in the save's command history (replayed instead of asking
/// again when a save made before the quiz is loaded) and, with a completed flag, in the layerbank so a restored
/// quiz layer shows the answer picked.
pub struct QUIZ {
    token: Option<command::token::QUIZ>,
    layer_id: LayerId,
    /// This frame's controller state, handed to the minigame on the next update
    pending_input: Option<MinigameInput>,
    /// Debug fast-forwarding has already announced it is picking for the player
    auto_pick_logged: bool,
}

impl QUIZ {
    pub fn handle_input(&mut self, input: MinigameInput) {
        self.pending_input = Some(input);
    }
}

impl StartableCommand for command::runtime::QUIZ {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // shortcut: there is no save data yet, so answers are neither replayed from nor recorded in it
        Yield(
            QUIZ {
                token: Some(self.token),
                layer_id: LayerId::new(self.arg as u16),
                pending_input: None,
                auto_pick_logged: false,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for QUIZ {
    fn update(
        &mut self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let plane = vm_state.layers.current_plane;
        let auto_choose = adv_state.auto_choose;
        let minigame = vm_state
            .layers
            .layerbank_allocator
            .get_layerbank_id(plane, self.layer_id)
            .and_then(|layerbank| {
                adv_state
                    .plane_layer_group_mut(plane)
                    .get_layer_mut(layerbank)
            })
            .and_then(|layer| layer.as_minigame_mut());
        let Some(quiz) = minigame else {
            // the original finishes with 0 when there is no minigame there
            warn!(
                "QUIZ: no minigame at {:?} on plane {:?}",
                self.layer_id, plane
            );
            return Some(self.token.take().unwrap().finish(0));
        };

        if let Some(mut input) = self.pending_input.take() {
            input.frames = context.delta_ticks.as_f32();
            if input.confirm || input.pressed != Default::default() {
                debug!("QUIZ: input {:?}", input);
            }
            quiz.handle_input(&input);
        }
        if auto_choose && quiz.is_interactive() {
            // debug fast-forwarding (sweeps, captures) cannot wait for a player
            if !std::mem::replace(&mut self.auto_pick_logged, true) {
                info!("QUIZ: picking an answer while fast-forwarding");
            }
            quiz.auto_pick();
        }
        let sounds = quiz.take_sounds();
        let answer = quiz.result();

        for sound in sounds {
            play_sys_se(
                &adv_state.audio_manager,
                context.asset_server,
                sound,
                Volume::default(),
            );
        }
        let answer = answer?;
        Some(self.token.take().unwrap().finish(answer))
    }
}

impl Debug for QUIZ {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QUIZ")
            .field("layer_id", &self.layer_id)
            .finish()
    }
}
