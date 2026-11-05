use std::fmt::{Debug, Formatter};

use super::prelude::*;

pub struct MSGWAIT {
    token: Option<command::token::MSGWAIT>,
    signal_num: i32,
}

impl StartableCommand for command::runtime::MSGWAIT {
    type StateInfo = ();
    /// `0x71000008b0`: waiting for the whole line lets the next MSGCLOSE clear it
    fn apply_state(&self, state: &mut VmState) {
        if self.signal_num < 0 {
            state.save.message_pending = false;
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(
            MSGWAIT {
                token: Some(self.token),
                signal_num: self.signal_num,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MSGWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let message_layer = adv_state.root_layer_group.message_layer();

        if message_layer.recv_sync_is_waiting(self.signal_num) {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

impl Debug for MSGWAIT {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MSGWAIT").field(&self.signal_num).finish()
    }
}
