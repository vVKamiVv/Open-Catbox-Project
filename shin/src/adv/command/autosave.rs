use super::prelude::*;

impl StartableCommand for command::runtime::AUTOSAVE {
    type StateInfo = ();

    /// `0x71000026e0`: a save point
    fn apply_state(&self, state: &mut VmState) {
        state.save.save_point(&mut state.global.persist_data);
        state.save.chapter();
    }

    fn start(
        self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        _state_info: (),
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // `0x71000078d0`: refreshes the auto slot, which reaches the file at the next write (the command writes
        // nothing itself)
        vm_state.save.snapshot(&mut vm_state.global);
        // the scene-title banner with the level-1 SAVEINFO text (`0x7100064240`)
        let title = vm_state.save_info.info[1].clone();
        adv_state.hud.show_scene_title(context.pre_render, &title);
        self.token.finish().into()
    }
}
