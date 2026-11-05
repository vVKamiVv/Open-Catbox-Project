use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::MSGCLOSE`, execute `0x7100005880`): closes the message box,
/// with its slide animation unless skipping (`0x71000b13a0`), and with `wait_for_close` stays until the animation has
/// finished (`0x71000b19c0`). Umineko waits 33 times.
#[derive(Debug)]
pub struct MSGCLOSE {
    token: Option<command::token::MSGCLOSE>,
}

impl StartableCommand for command::runtime::MSGCLOSE {
    type StateInfo = ();
    fn apply_state(&self, state: &mut VmState) {
        state.messagebox_state.messagebox_shown = false;
        state.messagebox_state.text = None;
        // `0x7100000c10`
        if !state.save.message_pending {
            state.save.message_current = false;
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        adv_state.root_layer_group.message_layer_mut().close(true);

        if !self.wait_for_close {
            return self.token.finish().into();
        }
        Yield(
            MSGCLOSE {
                token: Some(self.token),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MSGCLOSE {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // skipping closes the box without the animation
        (is_fast_forwarding || adv_state.message_layer().is_closed())
            .then(|| self.token.take().unwrap().finish())
    }
}
