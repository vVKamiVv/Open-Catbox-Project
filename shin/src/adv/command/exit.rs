use shin_core::time::Tween;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::EXIT`, execute `0x7100004b10`), a two-stage command:
/// first it tears the scene down (the touch buttons slide out as SYSCALL 0 does, `0x7100012550`; the scene-title and
/// song-title banners hide, `0x71000643c0` and `0x710009e4a0`; input turns off, `+0x480`; skip and auto stop; the
/// message box closes; the voice and every sound effect stop) and starts a 30-frame uninterruptible wait
/// (`0x71000138d0`, the same timer WAIT uses); then, once that wait, the fade and the task pool are done, it leaves the
/// adventure screen for the title as "Return to title" does (`0x710000fab0`, `0x71000147e0`: write the save, then
/// `CreateTitle(-1)`). The music keeps playing until the title replaces it. A non-zero `arg1` makes
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

        // the touch buttons slide out, the banners hide, skip and auto stop, the box closes, the voice stops and the
        // sound effects end; `Adv::handle_input` takes no input once the script has exited
        adv_state.hud.touch_buttons.hide();
        adv_state.hud.set_hidden(true);
        adv_state.skip_auto = 0;
        let message_layer = adv_state.root_layer_group.message_layer_mut();
        message_layer.close(true);
        message_layer.stop_voices();
        adv_state.se_player.stop_all(Tween::linear(Ticks::ZERO));
        // `0x710000fab0`: the pending SSETs are written; leaving the adventure screen then writes the save file
        vm_state.save.checkpoint(&mut vm_state.global.persist_data);

        // the 30-frame wait and the way out are in `Adv::update`
        CommandStartResult::Exit
    }
}
