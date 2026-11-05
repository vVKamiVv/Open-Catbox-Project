//! Builds the scene of a loaded save from the scene model the replay left (`ADV::SceneLoader`, `0x7100019ea0`
//! state 1): the layers and plane masks (loaded in the background, like LAYERLOAD and MASKLOAD), the properties of
//! every layer group, the BGM and the looping sound effects. Nothing animates in: the values are set at once.
//!
//! shortcut: the original also cross-fades from the previous screen (0.3 s), restores the message box style and a
//! minigame's answer, and unlocks the restored pictures; the port starts on a black screen and skips the rest.

use std::sync::Arc;

use shin_core::{
    format::scenario::Scenario,
    time::{Ticks, Tween},
    vm::command::types::{LayerId, LayerbankId, MaskFlags, PlaneId},
};
use shin_tasks::AsyncTask;
use tracing::info;

use crate::{
    adv::{AdvState, VmState},
    asset::mask::MaskTexture,
    layer::{DrawableLayer, LayerProperties, LayerPropertiesState, user::UserLayer},
    update::UpdateContext,
};

struct PendingLayer {
    plane: PlaneId,
    layerbank: LayerbankId,
    layer_id: LayerId,
    properties: LayerPropertiesState,
    task: AsyncTask<UserLayer>,
}

struct PendingMask {
    plane: PlaneId,
    flags: MaskFlags,
    task: AsyncTask<Arc<MaskTexture>>,
}

pub struct SceneRestore {
    layers: Vec<PendingLayer>,
    masks: Vec<PendingMask>,
}

impl SceneRestore {
    /// Sets what can be set at once and starts loading the layers and masks
    pub fn start(
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &VmState,
        adv_state: &mut AdvState,
    ) -> Self {
        let model = &vm_state.layers;
        model
            .root_layer_group
            .restore(adv_state.root_layer_group_mut().properties_mut());
        model
            .screen_layer
            .restore(adv_state.screen_layer_mut().properties_mut());
        model
            .page_layer
            .restore(adv_state.page_layer_mut().properties_mut());

        let mut masks = Vec::new();
        for (plane, group) in &model.plane_layergroups {
            group
                .properties
                .restore(adv_state.plane_layer_group_mut(plane).properties_mut());
            if let Some(mask_id) = group.mask_id.repr() {
                let path = scenario.info_tables().mask_info(mask_id).path();
                let asset_server = context.asset_server.clone();
                masks.push(PendingMask {
                    plane,
                    flags: group.mask_flags,
                    task: shin_tasks::async_io::spawn(async move {
                        asset_server
                            .load(&path)
                            .await
                            .expect("Failed to load mask texture")
                    }),
                });
            }
        }

        let mut layers = Vec::new();
        for (layerbank, state) in &model.layerbanks {
            let Some(layer_type) = state.layer_type else {
                continue;
            };
            let device = context.pre_render.device.clone();
            let asset_server = context.asset_server.clone();
            let audio_manager = adv_state.audio_manager.clone();
            let scenario = scenario.clone();
            let params = state.params;
            layers.push(PendingLayer {
                plane: state.plane,
                layerbank,
                layer_id: state.layer_id,
                properties: state.properties.clone(),
                task: shin_tasks::async_io::spawn(async move {
                    UserLayer::load(
                        &device,
                        &asset_server,
                        &audio_manager,
                        &scenario,
                        layer_type,
                        params,
                    )
                    .await
                }),
            });
        }

        restore_audio(context, scenario, vm_state, adv_state);
        info!(
            "Restoring the scene: {} layers, {} masks",
            layers.len(),
            masks.len()
        );
        Self { layers, masks }
    }

    /// Adds what has loaded; true once everything has
    pub fn poll(&mut self, adv_state: &mut AdvState) -> bool {
        let mut i = 0;
        while i < self.layers.len() {
            let Some(mut layer) = self.layers[i].task.poll_naive() else {
                i += 1;
                continue;
            };
            let pending = self.layers.swap_remove(i);
            let mut properties = LayerProperties::new();
            properties.set_layer_id(pending.layer_id);
            pending.properties.restore(&mut properties);
            *layer.properties_mut() = properties;
            adv_state
                .plane_layer_group_mut(pending.plane)
                .add_layer(pending.layerbank, layer);
        }
        let mut i = 0;
        while i < self.masks.len() {
            let Some(texture) = self.masks[i].task.poll_naive() else {
                i += 1;
                continue;
            };
            let pending = self.masks.swap_remove(i);
            adv_state
                .plane_layer_group_mut(pending.plane)
                .set_mask_texture(texture, pending.flags);
        }
        self.layers.is_empty() && self.masks.is_empty()
    }
}

/// The BGM and the looping sound effects of the model, as BGMPLAY and SEPLAY start them
fn restore_audio(
    context: &mut UpdateContext,
    scenario: &Arc<Scenario>,
    vm_state: &VmState,
    adv_state: &mut AdvState,
) {
    let audio = &vm_state.audio;
    if let Some(bgm) = audio.bgm {
        let info = scenario.info_tables().bgm_info(bgm.bgm_id);
        let track = context
            .asset_server
            .load_sync(info.path())
            .expect("Failed to load BGM track");
        adv_state.bgm_player.play(
            track,
            info.display_name.as_str(),
            true,
            bgm.volume,
            Tween::linear(Ticks::ZERO),
        );
    }
    for (slot, se) in audio.se.iter().enumerate() {
        let Some(se) = se else { continue };
        let info = scenario.info_tables().se_info(se.se_id);
        let sound = context
            .asset_server
            .load_sync(info.path())
            .expect("Failed to load sound effect");
        adv_state.se_player.play(
            slot as i32,
            sound,
            true,
            se.volume,
            se.pan,
            Tween::linear(Ticks::ZERO),
        );
    }
}
