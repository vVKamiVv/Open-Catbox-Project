use super::prelude::*;

impl StartableCommand for command::runtime::NOTIFYSET {
    type StateInfo = ();
    /// `0x7100004830`: the level of the "updated" banners (0 none)
    fn apply_state(&self, state: &mut VmState) {
        state.notify_level = self.arg;
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        self.token.finish().into()
    }
}
