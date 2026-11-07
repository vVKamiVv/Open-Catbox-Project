use std::fmt::{Debug, Formatter};

use tracing::debug;

use super::prelude::*;
use crate::update::UpdateContext;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::WAIT`, execute `0x7100005090`): starts the ADV wait timer
/// (`0x71000138d0`: context `+0x490` = interruptible, `+0x494` = frames left) and is done once it runs out; skip mode
/// cancels it (`0x71000138f0`). The timer counts down after the frame's command loop, so a wait of N frames ends on
/// the frame after its N-th. An interruptible wait is also ended by a confirm or down the text did not take
/// (`0x7100012194`; Umineko: 49 uses, e.g. "wait up to a minute or until a click" at `0x0a14a2`).
pub struct WAIT {
    token: Option<command::token::WAIT>,
    waiting_left: Ticks,
    allow_interrupt: bool,
    interrupted: bool,
}

impl WAIT {
    /// The player pressed advance and the text did not take it; ends the wait if it is interruptible
    pub fn interrupt(&mut self) {
        self.interrupted |= self.allow_interrupt;
    }
}

impl StartableCommand for command::runtime::WAIT {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {
        // nothing to do
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
            WAIT {
                token: Some(self.token),
                waiting_left: self.wait_amount,
                allow_interrupt: self.allow_interrupt,
                interrupted: false,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for WAIT {
    fn update(
        &mut self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.waiting_left <= Ticks::ZERO || is_fast_forwarding || self.interrupted {
            debug!("WAIT: done");
            return Some(self.token.take().unwrap().finish());
        }
        self.waiting_left -= context.delta_ticks;
        None
    }
}

impl Debug for WAIT {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WAIT").field(&self.waiting_left).finish()
    }
}
