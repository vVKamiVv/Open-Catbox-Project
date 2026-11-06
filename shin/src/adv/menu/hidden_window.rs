//! `ADV::HiddenWindow` (ctor `0x710001bd80`, update `0x710001bf90`): B hides the message window and the banners
//! until A, B, X, Y or a tap. Unlike the system menu it pauses nothing: the scene, voices and waits go on.

use shin_render::render_pass::RenderPass;

use super::{AdvRequest, Task, TaskCx};

#[derive(Default)]
pub struct HiddenWindow {
    /// The pointer went down while the window was hidden; its release brings the window back
    pressed: bool,
    done: bool,
}

impl Task for HiddenWindow {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        if self.done {
            return false;
        }
        if cx.focused {
            let input = &cx.input;
            if input.pointer.held {
                self.pressed = true;
            }
            let released = self.pressed && !input.pointer.held;
            // the press that closes it is not seen by the ADV: this task keeps the focus for the frame
            if input.confirm || input.cancel || input.x || input.y || released {
                self.done = true;
                cx.requests.push(AdvRequest::Unhide);
            }
        }
        true
    }

    /// Draws nothing (`0x7100017b50`)
    fn render(&self, _pass: &mut RenderPass) {}
}
