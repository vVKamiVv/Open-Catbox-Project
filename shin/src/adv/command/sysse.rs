use kira::track::TrackId;
use shin_audio::{AudioData, AudioSettings};
use shin_core::{format::sysse::SysSe, time::Tween, vm::command::types::Pan};

use super::prelude::*;

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
            match context.asset_server.load_sync::<SysSe>("/sysse.bin") {
                Ok(sys_se) => match sys_se.sounds.get(SYS_SE_0) {
                    Some(sound) => {
                        // fire and forget: the handle is not needed to let it play out
                        adv_state.audio_manager.play(AudioData {
                            source: sound.decode(),
                            settings: AudioSettings {
                                track: TrackId::Main,
                                fade_in: Tween::linear(Ticks::ZERO),
                                loop_start: None,
                                volume: self.volume,
                                pan: Pan::default(),
                            },
                        });
                    }
                    None => warn!("SYSSE: sysse.bin has no \"{SYS_SE_0}\" sound"),
                },
                Err(e) => warn!("SYSSE: could not load sysse.bin: {e:?}"),
            }
        }
        self.token.finish().into()
    }
}
