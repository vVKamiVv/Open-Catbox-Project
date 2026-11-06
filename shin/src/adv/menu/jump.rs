//! The chapter jumps' confirmation (`ADV::JumpNext` `0x7100012c80`, `ADV::JumpPrev` `0x7100012f80`): a Yes/No
//! dialog whose Yes, once the dialog is gone, rebuilds the screen at the jump's target (`$_4` `0x7100018510`, `$_2`
//! `0x71000182c0`).

use shin_render::render_pass::RenderPass;

use super::{AdvRequest, Task, TaskCx, dialog::Dialog};

pub struct JumpDialog {
    dialog: Dialog,
}

impl JumpDialog {
    pub fn new(dialog: Dialog) -> Self {
        Self { dialog }
    }
}

impl Task for JumpDialog {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let running = self.dialog.update(cx);
        if self.dialog.take_events().contains(&(1, 0)) {
            cx.requests.push(AdvRequest::Jump);
        }
        running
    }

    fn render(&self, pass: &mut RenderPass) {
        self.dialog.render(pass);
    }
}
