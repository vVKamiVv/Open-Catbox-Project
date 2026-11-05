use super::prelude::*;

impl StartableCommand for command::runtime::UNLOCK {
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
        crate::adv::unlocks::unlock_command(
            &mut vm_state.global.save_vectors,
            self.unlock_type,
            self.unlock_indices.iter().copied(),
        );
        self.token.finish().into()
    }
}
