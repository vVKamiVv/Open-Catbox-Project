use std::fmt::Debug;

use shin_core::vm::command::types::{AudioWaitStatus, LayerId, PlaneId};

use super::prelude::*;
use crate::{adv::vm_state::layers::LayerOperationTargetList, layer::user::UserLayer};

pub struct MOVIEWAIT {
    token: Option<command::token::MOVIEWAIT>,
    plane: PlaneId,
    layers: LayerOperationTargetList,
    unwanted_statuses: AudioWaitStatus,
}

impl StartableCommand for command::runtime::MOVIEWAIT {
    type StateInfo = LayerOperationTargetList;
    fn apply_state(&self, state: &mut VmState) -> LayerOperationTargetList {
        let layers = &mut state.layers;

        let mut affected_layers = Default::default();

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup
            | VLayerIdRepr::ScreenLayer
            | VLayerIdRepr::PageLayer
            | VLayerIdRepr::PlaneLayerGroup => {
                // nope, that's not a MovieLayer
            }
            VLayerIdRepr::Selected => {
                affected_layers = layers.layerbank_allocator.layers_in_range(
                    layers.current_plane,
                    layers.layer_selection.from,
                    layers.layer_selection.to,
                )
            }
            VLayerIdRepr::Layer(layer_id) => {
                affected_layers = layers.layerbank_allocator.layers_in_range(
                    layers.current_plane,
                    layer_id,
                    layer_id,
                )
            }
        }

        affected_layers
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        state_info: Self::StateInfo,
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(
            MOVIEWAIT {
                token: Some(self.token),
                plane: vm_state.layers.current_plane,
                layers: state_info,
                unwanted_statuses: self.unwanted_statuses,
            }
            .into(),
        )
    }
}

impl UpdatableCommand for MOVIEWAIT {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        let mut should_wait = false;

        for layer in &self.layers {
            let UserLayer::Movie(movie_layer) = adv_state
                .plane_layer_group(vm_state.layers.current_plane)
                .get_layer(layer.layerbank)
                .expect("BUG: layerbank not found")
            else {
                continue;
            };

            // `0x710000bd60`: bit 1 waits while the movie plays. Input does not end the wait; skipping does, through the
            // movie layer's fast-forward, which ends a movie that does not repeat. shortcut: bit 0 (the movie not ready
            // yet) is never used by the script, so it is not waited for; implement it if a script uses MOVIEWAIT with 1
            if self.unwanted_statuses.contains(AudioWaitStatus::PLAYING)
                && !movie_layer.is_finished()
            {
                should_wait = true;
                break;
            }
        }

        if should_wait {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

impl Debug for MOVIEWAIT {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MOVIEWAIT").field(&self.layers).finish()
    }
}
