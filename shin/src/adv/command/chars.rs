use super::prelude::*;

impl StartableCommand for command::runtime::CHARS {
    type StateInfo = ();
    /// `0x7100004370`: the character screen's state
    fn apply_state(&self, state: &mut VmState) {
        state.chars = (self.arg1, self.arg2);
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // `0x710000da00`: "Characters updated."
        if vm_state.notify_level > 0 {
            adv_state
                .hud
                .queue_notification((16 * (vm_state.notify_level - 1)) as u8);
        }
        self.token.finish().into()
    }
}
