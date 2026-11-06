use super::prelude::*;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SHOWCHARS`, listener `0x7100004710`, execute
/// `0x710000e3c0`): turns skip and auto off and opens the character screen with the scenario's CHARS state as a task,
/// then waits until it closes. Umineko uses it once, in Episode 5, right after a CHARS. The screen itself is opened by
/// the ADV scene, which owns the menu tasks: the command asks through [`AdvState::show_chars`].
#[derive(Debug)]
pub struct SHOWCHARS {
    token: Option<command::token::SHOWCHARS>,
}

/// Where a SHOWCHARS stands, between the command and the ADV scene that opens the screen
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ShowChars {
    #[default]
    None,
    /// The command asked for the screen; the scene opens it on its next update
    Requested,
    /// The screen is open; the scene sets `None` when it closes
    Open,
}

impl StartableCommand for command::runtime::SHOWCHARS {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {}

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if adv_state.auto_choose {
            // debug fast-forwarding (sweeps, captures) cannot wait for a player
            return self.token.finish().into();
        }
        adv_state.skip_auto = 0;
        adv_state.show_chars = ShowChars::Requested;
        Yield(
            SHOWCHARS {
                token: Some(self.token),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for SHOWCHARS {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        (adv_state.show_chars == ShowChars::None).then(|| self.token.take().unwrap().finish())
    }
}
