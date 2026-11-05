use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::WIPEWAIT`, execute `0x7100006b30`, polled every frame):
/// done once the screen's page layer reports no running transition (`0x71000e66e0`). Pairs with a `WIPE` that was
/// started without waiting.
#[derive(Debug)]
pub struct WIPEWAIT {
    token: Option<command::token::WIPEWAIT>,
}

impl StartableCommand for command::runtime::WIPEWAIT {
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
            WIPEWAIT {
                token: Some(self.token),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for WIPEWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        (!adv_state.screen_layer().is_transition_active())
            .then(|| self.token.take().unwrap().finish())
    }
}
