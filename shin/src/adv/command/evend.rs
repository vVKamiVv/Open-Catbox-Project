use super::prelude::*;

impl StartableCommand for command::runtime::EVEND {
    type StateInfo = ();
    /// `0x7100002960`
    fn apply_state(&self, state: &mut VmState) {
        state.save.count(&mut state.global.persist_data);
        state.save.event = None;
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
