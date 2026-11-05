use super::prelude::*;
use crate::audio::VoicePlayFlags;

impl StartableCommand for command::runtime::VOICEPLAY {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // `0x7100007420`: skipped while skipping; the flags are the voice player's (lip-sync, character muting)
        if adv_state.skipping {
            return self.token.finish().into();
        }
        adv_state.root_layer_group.message_layer_mut().play_voice_command(
            scenario,
            &self.name,
            self.volume,
            VoicePlayFlags::from_bits_truncate(self.flags),
        );
        self.token.finish().into()
    }
}
