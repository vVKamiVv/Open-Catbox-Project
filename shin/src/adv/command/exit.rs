use shin_core::time::Tween;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::EXIT`, execute `0x7100004b10`), a two-stage command:
/// first it tears the scene down (the SYSCALL 0 fade `0x7100012550`, clears the layer banks, closes the message box,
/// stops the voice, fades and stops every sound effect) and starts a 30-frame uninterruptible wait
/// (`0x71000138d0`, the same timer WAIT uses); then, once that wait, the fade and the task pool are done, it leaves the
/// adventure screen for the menus (`0x710000fab0`, `0x71000147e0`). The music keeps playing. A non-zero `arg1` makes
/// the VM treat it as a no-op. Umineko always passes (0, 0), at the end of each playable piece and in the developer
/// menu.
impl StartableCommand for command::runtime::EXIT {
    type StateInfo = ();
    /// `0x7100000230`
    fn apply_state(&self, state: &mut VmState) {
        if self.arg1 == 0 {
            state.save.exit_seen = true;
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
        if self.arg1 != 0 {
            return self.token.finish().into();
        }

        adv_state.se_player.stop_all(Tween::linear(Ticks::ZERO));
        // `0x710000fab0`: the pending SSETs are written; leaving the adventure screen then writes the save file
        vm_state.save.checkpoint(&mut vm_state.global.persist_data);

        // shortcut: there are no menus to return to yet, so the adventure screen just stops running the script
        // (see `Adv::update`); the layer clearing and the voice stop happen with the menus once they exist
        CommandStartResult::Exit
    }
}
