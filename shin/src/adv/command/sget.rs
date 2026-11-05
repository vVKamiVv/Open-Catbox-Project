use shin_core::format::scenario::Scenario;

use super::prelude::*;

impl StartableCommand for command::runtime::SGET {
    type StateInfo = ();

    fn apply_state(&self, _state: &mut VmState) {
        // nothing to do
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // recorded in the route, so loading a save gets the same value back
        let value = vm_state
            .save
            .sget(&vm_state.global.persist_data, self.slot_number);
        self.token.finish(value).into()
    }
}
