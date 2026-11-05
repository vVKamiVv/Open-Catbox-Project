use std::fmt::{Debug, Formatter};

use shin_core::format::save::RouteData;
use tracing::info;

use super::prelude::*;
use crate::layer::select_layer::SelectLayer;

/// Behaviour in the Switch v1.0.3 executable (`shin::ADV::SELECT`, built by the ADV's listener slot 14
/// `0x7100000d90`, execute `0x7100005ba0`): shows the choice menu and, once an option is picked, writes its index
/// to `dest`. It records the pick in the save data: bit `choice_set_base + index` of the seen-choices set (which tints
/// options picked before), `index + 1` at `choice_index` of the choice table, and the pick in the route.
pub struct SELECT {
    token: Option<command::token::SELECT>,
    /// -1 when the choice is not remembered
    choice_set_base: i32,
    choice_index: i32,
}

impl StartableCommand for command::runtime::SELECT {
    /// Whether the choice is a save point
    type StateInfo = bool;

    /// `0x7100000d90`: a choice asked under a line still shown is not a save point (the line is)
    fn apply_state(&self, state: &mut VmState) -> bool {
        let save_point = !state.save.message_current;
        if save_point {
            state.save.save_point(&mut state.global.persist_data);
        }
        save_point
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _save_point: bool,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        let choice_set_base = self.choice_set_base as i16 as i32;
        let seen_choices = &vm_state.global.save_vectors.select_seen_choices;
        let seen = (0..self.variants.len().min(32))
            .filter(|&i| choice_set_base >= 0 && seen_choices.contains(choice_set_base as usize + i))
            .fold(0, |seen, i| seen | 1 << i);
        vm_state.save.snapshot(&mut vm_state.global);
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
                choice_set_base,
                choice_index: self.choice_index as i16 as i32,
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
        vm_state: &mut VmState,
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
        record_pick(vm_state, self.choice_set_base, self.choice_index, chosen as i32);
        Some(self.token.take().unwrap().finish(chosen as i32))
    }
}

/// `0x7100005ba0` once picked: remembered for the tint and the choice table, then a save point in the route
fn record_pick(vm_state: &mut VmState, choice_set_base: i32, choice_index: i32, chosen: i32) {
    let vectors = &mut vm_state.global.save_vectors;
    if choice_set_base >= 0 {
        vectors
            .select_seen_choices
            .add((choice_set_base + chosen) as usize);
    }
    if choice_index >= 0 {
        let index = choice_index as usize;
        if vectors.select_last_choices.len() <= index {
            // grows in steps of 32 (`0x71000d1a30`)
            vectors.select_last_choices.resize((index + 32) & !31, 0);
        }
        vectors.select_last_choices[index] = chosen as u8 + 1;
    }
    let save = &mut vm_state.save;
    save.record_answer(RouteData::SELECT_BITS, chosen);
    save.snapshot(&mut vm_state.global);
}

impl Debug for SELECT {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SELECT").finish()
    }
}
