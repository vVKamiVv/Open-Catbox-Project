use super::prelude::*;
use crate::audio::play_sys_se;

/// The only system sound the command can play (the original passes this name, at `0x710015d64e`)
const SYS_SE_0: &str = "horror";

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SYSSE`, execute `0x71000077d0`): with id 0 it plays the
/// built-in "horror" sound from `/sysse.bin` once at `volume` (`0x71000272b0`); any other id does nothing. It never
/// waits. Umineko uses it once, in a script helper around `0x0a1495`.
impl StartableCommand for command::runtime::SYSSE {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if self.sys_se_id == 0 {
            play_sys_se(
                &adv_state.audio_manager,
                context.asset_server,
                SYS_SE_0,
                self.volume,
            );
        }
        self.token.finish().into()
    }
}
