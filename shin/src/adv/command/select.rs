use std::fmt::{Debug, Formatter};

use tracing::info;

use super::prelude::*;
use crate::layer::select_layer::SelectLayer;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SELECT`, built by the ADV's listener slot 14
/// `0x7100000d90`, execute `0x7100005ba0`): shows the choice menu and, once an option is picked, writes its index
/// to `dest`. The original also records the pick in the save data: bit `choice_set_base + index` of the seen-choices
/// set (which tints options picked before) and `index + 1` at `choice_index` of the choice table.
pub struct SELECT {
    token: Option<command::token::SELECT>,
}

impl StartableCommand for command::runtime::SELECT {
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
        // shortcut: there are no save data yet, so no option shows as picked before and picks are not recorded
        let seen = 0;
        adv_state.select_layer = Some(SelectLayer::new(
            context.pre_render,
            &adv_state.fonts,
            &self.variants,
            self.choice_visibility_mask as u32,
            seen,
        ));
        Yield(
            SELECT {
                token: Some(self.token),
            }
            .into(),
        )
    }
}

impl UpdatableCommand for SELECT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let auto_choose = adv_state.auto_choose;
        let layer = adv_state.select_layer.as_mut()?;
        if auto_choose && layer.chosen().is_none() {
            // debug fast-forwarding (sweeps, captures) cannot wait for a player
            info!("SELECT: picking the first option while fast-forwarding");
            layer.pick_now(0);
        }
        let chosen = layer.chosen()?;
        adv_state.select_layer = None;
        Some(self.token.take().unwrap().finish(chosen as i32))
    }
}

impl Debug for SELECT {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SELECT").finish()
    }
}
