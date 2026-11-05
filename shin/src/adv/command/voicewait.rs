use shin_core::vm::command::types::AudioWaitStatus;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::VOICEWAIT`, execute `0x7100007760`, polled every frame):
/// done once the voice player's status (`0x710011a980`) has none of the `unwanted_statuses` bits. That status is 3
/// while a voice is in its first state, 2 while one is in its second, else 0; Umineko always waits on 2 (PLAYING),
/// i.e. until every voice has finished.
#[derive(Debug)]
pub struct VOICEWAIT {
    token: Option<command::token::VOICEWAIT>,
    unwanted_statuses: AudioWaitStatus,
}

impl StartableCommand for command::runtime::VOICEWAIT {
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
            VOICEWAIT {
                token: Some(self.token),
                unwanted_statuses: self.unwanted_statuses,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for VOICEWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let status = adv_state.message_layer().voice_wait_status();
        (status & self.unwanted_statuses)
            .is_empty()
            .then(|| self.token.take().unwrap().finish())
    }
}
