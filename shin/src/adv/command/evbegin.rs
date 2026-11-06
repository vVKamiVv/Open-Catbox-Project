use super::prelude::*;

impl StartableCommand for command::runtime::EVBEGIN {
    type StateInfo = ();
    /// `0x71000027e0`: counted, but the event's lines are not save points until EVEND
    fn apply_state(&self, state: &mut VmState) {
        state.save.count(&mut state.global.persist_data);
        state.save.event = Some(self.arg);
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
