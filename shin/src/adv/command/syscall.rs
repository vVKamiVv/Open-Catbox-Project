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
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if self.call_id == 0 {
            warn!("SYSCALL 0: context fade not implemented (argument {})", self.argument);
        }
        self.token.finish().into()
    }
}
