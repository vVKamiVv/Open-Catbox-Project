use super::prelude::*;

impl StartableCommand for command::runtime::TIPSGET {
    type StateInfo = ();
    /// `0x710000da40`: the tips join the playthrough's tips
    fn apply_state(&self, state: &mut VmState) {
        for &tip in self.tip_ids.iter() {
            if !state.tips.contains(&tip) {
                state.tips.push(tip);
            }
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // `0x710000dc80`: "Tips updated."
        if vm_state.notify_level > 0 {
            adv_state
                .hud
                .queue_notification((16 * (vm_state.notify_level - 1)) as u8 | 1);
        }
        self.token.finish().into()
    }
}
