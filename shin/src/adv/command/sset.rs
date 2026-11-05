use super::prelude::*;

impl StartableCommand for command::runtime::SSET {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // reaches the persistent data at the next save point
        vm_state.save.sset(self.slot_number, self.value);
        self.token.finish().into()
    }
}
