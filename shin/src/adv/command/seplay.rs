use std::ops::Not;

use shin_core::{time::Tween, vm::command::types::AudioWaitStatus};

use super::prelude::*;
use crate::adv::vm_state::audio::SeState;

impl StartableCommand for command::runtime::SEPLAY {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.audio.se[self.se_slot as usize] = self.no_repeat.not().then_some(SeState {
            se_id: self.se_data_id,
            volume: self.volume,
            pan: self.pan,
            play_speed: self.play_speed as f32 / 1000.0,
        });
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if self.play_speed != 1000 {
            warn!("TODO: SEPLAY: ignoring play_speed={}", self.play_speed);
        }

        // `0x7100006ec0`: while skipping, a one-shot sound stops its slot and does not play
        if adv_state.skipping && self.no_repeat {
            if adv_state.se_player.get_wait_status(self.se_slot).contains(AudioWaitStatus::PLAYING) {
                adv_state.se_player.stop(self.se_slot, Tween::linear(Ticks::ZERO));
            }
            return self.token.finish().into();
        }

        let se_info = scenario.info_tables().se_info(self.se_data_id);

        let audio = context
            .asset_server
            // TODO: sync - bad!!
            .load_sync(se_info.path())
            .expect("Failed to load BGM track");

        adv_state.se_player.play(
            self.se_slot,
            self.se_data_id,
            audio,
            !self.no_repeat,
            self.volume,
            self.pan,
            Tween::linear(if adv_state.skipping { Ticks::ZERO } else { self.fade_in_time }),
        );

        self.token.finish().into()
    }
}
