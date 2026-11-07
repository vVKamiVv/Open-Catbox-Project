use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::TROPHY`, execute `0x7100007c80`): the Switch build only
/// locks and unlocks a mutex (`0x71001171f0`); there is nothing to unlock, so it does nothing here either.
impl StartableCommand for command::runtime::TROPHY {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        tracing::debug!("TROPHY {:?}", self);
        self.token.finish().into()
    }
}
