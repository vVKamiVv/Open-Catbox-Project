use super::prelude::*;
use shin_core::vm::command::types::{LAYERS_COUNT, LayerId};

use crate::adv::vm_state::layers::LayerSelection;

impl StartableCommand for command::runtime::LAYERSELECT {
    type StateInfo = ();
    /// `0x7100003740`: both ids are clamped to 0..=255, then put in order
    fn apply_state(&self, state: &mut VmState) {
        let clamp = |id: i32| LayerId::new(id.clamp(0, LAYERS_COUNT as i32 - 1) as u16);
        let (mut from, mut to) = (clamp(self.selection_start_id), clamp(self.selection_end_id));
        if from > to {
            std::mem::swap(&mut from, &mut to);
        }
        state.layers.layer_selection = LayerSelection { from, to }
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        self.token.finish().into()
    }
}
