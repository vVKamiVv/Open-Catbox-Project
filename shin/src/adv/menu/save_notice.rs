//! The notice shown while the save file is written (`SaveGame`, `0x71000d52f0`, update `0x71000d3600`): a dialog
//! without buttons, "Writing save data. Please do not close the game.", up for at least a second; its write is asked
//! for when it opens. A screen that writes shows it over itself and goes on once it is gone.

use shin_render::render_pass::RenderPass;

use super::{AdvRequest, MenuAssets, Task, TaskCx, dialog::Dialog};
use crate::{asset::executable::EngineText, render::PreRenderContext};

const WRITING: EngineText = EngineText(
    0x710015f024,
    "Writing save data.@rPlease do not close the game.",
);
/// How long the notice stays up (the original's write takes at least a second)
const MIN_FRAMES: f32 = 60.0;

pub struct SaveNotice {
    dialog: Dialog,
    elapsed: f32,
    requested: bool,
    /// Asked of the adventure screen once the notice is gone
    then: Option<AdvRequest>,
}

impl SaveNotice {
    pub fn new(ctx: &PreRenderContext, assets: &MenuAssets, then: Option<AdvRequest>) -> Self {
        Self {
            dialog: Dialog::new(ctx, assets, &WRITING.get(), &[], -1, false),
            elapsed: 0.0,
            requested: false,
            then,
        }
    }
}

impl Task for SaveNotice {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        if !self.requested {
            self.requested = true;
            cx.requests.push(AdvRequest::WriteSave);
        }
        self.elapsed += cx.frames;
        // `$_1` `0x71000d5500`: the write is done (the port's is immediate), so the notice closes after its second
        if self.elapsed >= MIN_FRAMES {
            self.dialog.close_now();
        }
        let running = self.dialog.update(cx);
        if !running {
            if let Some(then) = self.then.take() {
                cx.requests.push(then);
            }
        }
        running
    }

    fn render(&self, pass: &mut RenderPass) {
        self.dialog.render(pass);
    }
}
