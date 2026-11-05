use tracing::warn;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SYSCALL`, execute `0x7100007c10`): any call id other than 0
/// does nothing. Call id 0 (used e.g. at script address `0x0a1456`, after clearing all layers and sound effects) starts
/// a short tween of an unidentified ADV context value (`+0x500`) to 0 if a context flag (`+0x4f8`) is set, then clears
/// that flag and the field at `+0x47c`. It never waits.
// shortcut: call 0's fade is not reproduced because the value it fades is not identified yet; implement it once that
// context field is known (see docs/engine-internals.md, open questions).
impl StartableCommand for command::runtime::SYSCALL {
    type StateInfo = ();
    /// `0x7100002ce0`: SYSCALL 0 sets whether the system (skip, auto, menus) is enabled
    fn apply_state(&self, state: &mut VmState) {
        if self.call_id == 0 {
            state.save.system_enabled = self.argument != 0;
        }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if self.call_id == 0 {
            // `0x7100007c10`: skip and auto turn off
            // TODO: it also fades out the touch screen's on-screen buttons, which the port does not draw
            adv_state.skip_auto = 0;
        }
        self.token.finish().into()
    }
}
