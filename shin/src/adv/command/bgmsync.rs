use shin_core::vm::command::types::AudioWaitStatus;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::BGMSYNC`, execute `0x7100006e20`, polled every frame): done
/// at once when no music is PLAYING, otherwise done once the track's playback position (microseconds,
/// `0x7100033e70`) reaches `sync_time` frames of 1/60 s. Used to time scenes to the music.
#[derive(Debug)]
pub struct BGMSYNC {
    token: Option<command::token::BGMSYNC>,
    sync_time: Ticks,
}

impl StartableCommand for command::runtime::BGMSYNC {
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
        Yield(
            BGMSYNC {
                token: Some(self.token),
                sync_time: Ticks::from_f32(self.sync_time as f32),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for BGMSYNC {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let bgm = &adv_state.bgm_player;
        let done = !bgm.get_wait_status().contains(AudioWaitStatus::PLAYING)
            || bgm.position() >= self.sync_time;
        done.then(|| self.token.take().unwrap().finish())
    }
}
