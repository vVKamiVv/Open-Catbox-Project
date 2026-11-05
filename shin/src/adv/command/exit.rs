use shin_core::time::Tween;

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::EXIT`, execute `0x7100004b10`), a two-stage command:
/// first it tears the scene down (the SYSCALL 0 fade `0x7100012550`, clears the layer banks, stops the voice, fades
/// and stops every sound effect, fades the music out over 30 frames with `0x71000138d0`); then it waits until the
/// music has stopped, that fade is over and the task pool is idle, and leaves the adventure screen for the menus
/// (`0x710000fab0`, `0x71000147e0`). A non-zero `arg1` makes the VM treat it as a no-op. Umineko always passes
/// (0, 0), at the end of each playable piece and in the developer menu.
impl StartableCommand for command::runtime::EXIT {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if self.arg1 != 0 {
            return self.token.finish().into();
        }

        adv_state.se_player.stop_all(Tween::linear(Ticks::ZERO));
        if adv_state
            .bgm_player
            .get_wait_status()
            .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING)
        {
            adv_state
                .bgm_player
                .stop(Tween::linear(Ticks::from_f32(30.0)));
        }

        // shortcut: there are no menus to return to yet, so the adventure screen just stops running the script
        // (see `Adv::update`); the layer clearing and the voice stop happen with the menus once they exist
        CommandStartResult::Exit
    }
}
