use std::fmt::{Debug, Formatter};

use shin_core::{
    format::scenario::{info::SeId, instruction_elements::FromNumber},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};

use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SEONCE`, execute `0x7100007320`, polled every frame until
/// it reports done): in skip mode the command ends at once without playing (`0x7100012430`, also used by SESTOPALL).
/// Otherwise, on its first run it plays sound effect `arg1` once (no loop, no fade-in) at volume `arg3` and pan
/// `arg4` on the first free automatic channel (-1, -2, ... where free = not PLAYING), then waits until that channel's
/// status has none of the `arg2` bits (Umineko passes 0 = don't wait, or 1 = wait until it finishes). `arg5` (always
/// 1000, probably the play speed) is not read by the execute function.
pub struct SEONCE {
    token: Option<command::token::SEONCE>,
    se_id: SeId,
    unwanted_statuses: AudioWaitStatus,
    volume: Volume,
    pan: Pan,
    /// Channel it plays on, once started
    slot: Option<i32>,
}

impl StartableCommand for command::runtime::SEONCE {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {
        // a one-shot sound is not part of the restorable state
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(
            SEONCE {
                token: Some(self.token),
                se_id: SeId::from_number(self.arg1),
                unwanted_statuses: AudioWaitStatus::from_bits_truncate(self.arg2),
                volume: Volume::from_number(self.arg3),
                pan: Pan::from_number(self.arg4),
                slot: None,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for SEONCE {
    fn update(
        &mut self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        // skipping (`isSkipping`: skip mode or the held button, v11's read test) plays no one-shot sound
        if is_fast_forwarding {
            return Some(self.token.take().unwrap().finish());
        }

        let slot = match self.slot {
            Some(slot) => slot,
            None => {
                let se_info = scenario.info_tables().se_info(self.se_id);
                let audio = context
                    .asset_server
                    // loaded at once; the original streams it in the background (status bit 1 while loading), which only saves a hitch
                    .load_sync(se_info.path())
                    .expect("Failed to load SE");
                let slot = adv_state.se_player.play_once(audio, self.volume, self.pan);
                self.slot = Some(slot);
                slot
            }
        };

        let status = adv_state.se_player.get_wait_status(slot);
        (status & self.unwanted_statuses)
            .is_empty()
            .then(|| self.token.take().unwrap().finish())
    }
}

impl Debug for SEONCE {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SEONCE")
            .field("se_id", &self.se_id)
            .field("slot", &self.slot)
            .finish()
    }
}
