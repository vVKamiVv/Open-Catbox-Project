use std::fmt::{Debug, Formatter};

use super::prelude::*;
use crate::layer::message_layer::{MessageFlags, MsgsetParams};

pub struct MSGSET {
    #[allow(unused)]
    token: Option<command::token::MSGSET>,
}

impl StartableCommand for command::runtime::MSGSET {
    /// Whether the line has been read before
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        // TODO: think about async messages (those where you would use MSGWAIT)
        state.messagebox_state.text = Some(self.text.clone());
        state.messagebox_state.messagebox_shown = true;

        // `0x7100005140`: a line is a save point unless an event is playing
        if state.save.event.is_none() {
            state.save.save_point(&mut state.global.persist_data);
            state.save.line();
        }
        state.save.message_current = true;
        state.save.message_pending = !self.auto_wait;

        // the read set is indexed by the script's message id - 1 (engine-internals, section 10)
        let id = self.msg_id.0 as i64 - 1;
        let read = id < 0
            || state
                .global
                .save_vectors
                .seen_messages
                .contains(id as usize);
        state.save.line_read = read;
        read
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        vm_state: &mut VmState,
        read: bool,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        // shortcut: the original snapshots on the first frame after the text reached the message layer; the same frame
        vm_state.save.snapshot(&mut vm_state.global);
        adv_state.root_layer_group.message_layer_mut().on_msgset(
            context.pre_render,
            scenario,
            &self.text,
            MsgsetParams {
                // `0x71000052e0`: read | event << 1; lines inside an event play on by themselves
                flags: {
                    let mut flags = MessageFlags::empty();
                    if read {
                        flags |= MessageFlags::UNUSED_FLAG;
                    }
                    if vm_state.save.event.is_some() {
                        flags |= MessageFlags::IGNORE_INPUT;
                    }
                    flags
                },
                messagebox_type: vm_state.messagebox_state.msginit.messagebox_type,
                text_layout: vm_state.messagebox_state.msginit.text_layout,
                message_id: self.msg_id,
            },
            // the text appears at once while skipping (`0x71000052e0`)
            !adv_state.skipping,
        );

        if self.auto_wait {
            Yield(
                MSGSET {
                    token: Some(self.token),
                }
                .into(),
            )
        } else {
            self.token.finish().into()
        }
    }
}

impl UpdatableCommand for MSGSET {
    fn update(
        &mut self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        // TODO: I think we __should__ somehow to react to fast-forwarding here
        // rn it's kludged in the ADV update function though
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if adv_state
            .root_layer_group
            .message_layer()
            .recv_sync_is_waiting(-1)
        {
            None
        } else {
            Some(self.token.take().unwrap().finish())
        }
    }
}

impl Debug for MSGSET {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MSGSET").finish()
    }
}
