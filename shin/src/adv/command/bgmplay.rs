use std::ops::Not;

use shin_core::{format::scenario::info::BgmInfoItem, time::Tween};

use super::prelude::*;
use crate::adv::vm_state::audio::BgmState;

impl StartableCommand for command::runtime::BGMPLAY {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.audio.bgm = self.no_repeat.not().then_some(BgmState {
            bgm_id: self.bgm_data_id,
            volume: self.volume,
        });
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let bgm_info @ BgmInfoItem {
            name: _,
            display_name,
            linked_bgm_id: _,
        } = scenario.info_tables().bgm_info(self.bgm_data_id);

        let audio = context
            .asset_server
            // loaded at once; the original streams it in the background (status bit 1 while loading), which only saves a hitch
            .load_sync(bgm_info.path())
            .expect("Failed to load BGM track");

        let started = adv_state.bgm_player.play(
            self.bgm_data_id,
            audio,
            display_name.as_str(),
            !self.no_repeat,
            self.volume,
            Tween::linear(if adv_state.skipping { Ticks::ZERO } else { self.fade_in_time }),
        );

        if started {
            crate::adv::unlocks::bgm_started(&mut vm_state.global.save_vectors, scenario, self.bgm_data_id);
            // `0x710009e160`: the song title slides in
            adv_state
                .hud
                .show_song_title(context.pre_render, display_name.as_str());
        }

        self.token.finish().into()
    }
}
