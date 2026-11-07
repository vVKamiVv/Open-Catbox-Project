use shin_core::time::{Easing, Tween};
use shin_render::shaders::types::RenderCloneCtx;

use super::prelude::*;
use crate::{
    adv::vm_state::layers::LayerOperationTargetList,
    layer::{LayerProperties, LayerPropertiesState, TweenWaitFlags},
};

impl StartableCommand for command::runtime::LAYERCTRL {
    type StateInfo = LayerOperationTargetList;
    fn apply_state(&self, state: &mut VmState) -> LayerOperationTargetList {
        let layers = &mut state.layers;
        let (target_value, _time, flags, _easing_param, ..) = self.params;
        // the model keeps targets; with the delta flag the value is added to the stored one (the ctor's sign test on
        // the flags' low byte)
        let property = self.property_id;
        let set = |properties: &mut LayerPropertiesState| {
            let base = if flags.delta() {
                properties.get_property(property)
            } else {
                0
            };
            properties.set_property(property, base + target_value);
        };

        let mut affected_layers = Default::default();

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup => set(&mut layers.root_layer_group),
            VLayerIdRepr::ScreenLayer => set(&mut layers.screen_layer),
            VLayerIdRepr::PageLayer => set(&mut layers.page_layer),
            VLayerIdRepr::PlaneLayerGroup => {
                set(&mut layers.plane_layergroups[layers.current_plane].properties)
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

        for &target in &affected_layers {
            set(&mut layers.layerbanks[target.layerbank].properties);
        }

        affected_layers
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        affected_layers: LayerOperationTargetList,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let (target_value, duration, flags, easing_param, ..) = self.params;

        adv_state.create_back_layer_group_if_needed(&mut context.pre_render.render_clone_ctx());

        if flags.unused_1() != 0 || flags.unused_2() != 0 || flags.unused_3() != 0 {
            warn!("LAYERCTRL: unknown flags are set: {:?}", flags);
        }

        let easing = match flags.easing() {
            0 => Easing::Linear,
            1 => Easing::SineIn,
            2 => Easing::SineOut,
            3 => Easing::SineInOut,
            4 => Easing::Jump,
            5 => Easing::Power(easing_param),
            other => {
                warn!("LAYERCTRL: unknown easing {other}; linear instead");
                Easing::Linear
            }
        };

        let mut changed = false;
        let wait_flags = TweenWaitFlags {
            prohibit_fast_forward: flags.prohibit_fast_forward(),
            ignore_wait: flags.ignore_wait(),
        };
        let mut apply_to_properties = |properties: &mut LayerProperties| {
            properties.set_wait_flags(self.property_id, wait_flags);
            let tweener = properties.property_tweener_mut(self.property_id);

            let from_value = tweener.target_value();
            // with the delta flag the value is relative to the last target, as in the model above
            let to_value = if flags.delta() {
                from_value + target_value as f32
            } else {
                target_value as f32
            };
            let mut duration = duration;

            if tweener.value() != to_value {
                changed = true;
            }

            if flags.scale_time() {
                // this flag makes "duration" actually mean change rate (in value per tick)
                let change = (to_value - from_value).abs();
                duration = Ticks::from_f32(change / duration.as_f32());
            }

            if flags.ff_to_current() {
                let current = tweener.value();
                tweener.fast_forward_to(current);
            }
            if flags.ff_to_target() {
                tweener.fast_forward();
            }

            tweener.enqueue(to_value, Tween { duration, easing })
        };

        match self.layer_id.repr() {
            VLayerIdRepr::RootLayerGroup => {
                apply_to_properties(adv_state.root_layer_group_mut().properties_mut());
            }
            VLayerIdRepr::ScreenLayer => {
                apply_to_properties(adv_state.screen_layer_mut().properties_mut());
            }
            VLayerIdRepr::PageLayer => {
                apply_to_properties(adv_state.page_layer_mut().properties_mut());
            }
            VLayerIdRepr::PlaneLayerGroup => {
                apply_to_properties(
                    adv_state
                        .plane_layer_group_mut(vm_state.layers.current_plane)
                        .properties_mut(),
                );
            }
            VLayerIdRepr::Selected | VLayerIdRepr::Layer(_) => {
                for &target in &affected_layers {
                    apply_to_properties(
                        adv_state
                            .plane_layer_group_mut(vm_state.layers.current_plane)
                            .get_layer_mut(target.layerbank)
                            .expect("BUG: layerbank not found")
                            .properties_mut(),
                    );
                }
            }
        }

        self.token.finish().into()
    }
}
