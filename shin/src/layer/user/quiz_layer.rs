//! The quiz ([`LayerType::Quiz`](shin_core::vm::command::types::LayerType::Quiz) with kind 0): a column of answer
//! buttons the QUIZ command waits on.
//!
//! Re-implemented from the behaviour of `shin::QuizLayer` in the Switch v1.0.3 executable (constructor
//! `0x71000c76a0`, update `0x71000c8070`, draw `0x71000c8130`, result `0x71000c85d0`) and its input handler
//! `shin::QuizLayer::QuizOverlayLayer` (keys `0x71000c8910`). The button table is filled by a static initializer at
//! `0x71000c9710`.
//!
//! The second load parameter is a mask: bit `i` shows button `i` of [`BUTTONS`] (A to F, then "Ask Auntie Eva!").
//! The visible buttons are stacked from y 292, 8 px apart, centred on x 240. The cursor starts on the first one;
//! up/down move it with wrap-around, confirm flashes it, and once the flash is over the quiz reports the picked
//! button's bit index.

use std::{fmt::Debug, sync::Arc};

use glam::{Vec2, vec2};
use shin_core::time::Tweener;
use shin_render::{
    PassKind, gpu_texture::GpuTexture, render_pass::RenderPass, shaders::types::RenderClone,
};

use crate::{
    asset::texture_archive::TextureArchive,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::minigame::{
            self, Minigame, MinigameInput, PointerTarget, SOUND_DECIDE, SOUND_MOVE, Sprite, contains,
            from_top_left, seconds,
        },
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

/// All of `/quiz.txa`
#[derive(TextureArchive)]
pub struct QuizTextures {
    /// 256 x 384: the six answer buttons, one 64 px row each
    #[txa(name = "button")]
    pub button: GpuTexture,
    /// 768 x 176: the "Ask" button, two 368 px wide frames side by side
    #[txa(name = "consult")]
    pub consult: GpuTexture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Button,
    Consult,
}

/// One entry of the original's button table (`0x710022d108`, 7 x 0x14 bytes): which texture, and the pixel rect
#[derive(Debug)]
struct ButtonSprite {
    sheet: Sheet,
    /// u, v, width, height in pixels
    rect: [f32; 4],
}

const fn answer_row(v: f32) -> ButtonSprite {
    ButtonSprite {
        sheet: Sheet::Button,
        rect: [0.0, v, 256.0, 64.0],
    }
}

const BUTTONS: [ButtonSprite; 7] = [
    answer_row(0.0),
    answer_row(64.0),
    answer_row(128.0),
    answer_row(192.0),
    answer_row(256.0),
    answer_row(320.0),
    ButtonSprite {
        sheet: Sheet::Consult,
        rect: [0.0, 0.0, 368.0, 176.0],
    },
];

/// The texture sizes the uvs are divided by (`0x710022d0e8`)
fn sheet_size(sheet: Sheet) -> Vec2 {
    match sheet {
        Sheet::Button => vec2(256.0, 384.0),
        Sheet::Consult => vec2(768.0, 176.0),
    }
}

const CENTRE_X: f32 = 240.0;
const STACK_TOP: f32 = 288.0;
/// Each button sits this far below the running top
const BUTTON_TOP: f32 = 4.0;
const BUTTON_GAP: f32 = 8.0;

const HOVER_IN_SECONDS: f32 = 0.25;
const HOVER_OUT_SECONDS: f32 = 0.05;
/// The consult sheet's two frames alternate every half of this many frames
const BLINK_PERIOD_FRAMES: f32 = 60.0;

/// The visible buttons' bit indices and top-left corners, in 1920x1080 top-left space
fn layout(mask: u32) -> Vec<(usize, Vec2)> {
    let mut top = STACK_TOP;
    (0..BUTTONS.len())
        .filter(|id| mask & (1 << id) != 0)
        .map(|id| {
            let [_, _, width, height] = BUTTONS[id].rect;
            let position = vec2(CENTRE_X - width * 0.5, top + BUTTON_TOP);
            top += height + BUTTON_GAP;
            (id, position)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    /// Picked; waiting for the flash to end
    Picked,
    Done,
}

#[derive(Clone)]
struct Button {
    /// Bit index in the load mask, which is also what the quiz reports
    id: usize,
    position: Vec2,
    hover: Tweener,
    flash: Tweener,
}

#[derive(Clone, RenderClone)]
pub struct QuizLayerImpl {
    textures: Arc<QuizTextures>,
    buttons: Vec<Button>,
    state: State,
    cursor: usize,
    /// Fraction of the blink period; the consult button shows its second frame in the upper half
    blink_phase: f32,
    sounds: Vec<&'static str>,
    /// The button under the pointer (`+0x48`) and the one a press raised
    hovered: Option<usize>,
    pressed: Option<usize>,
}

impl QuizLayerImpl {
    pub fn new(textures: Arc<QuizTextures>, mask: u32) -> Self {
        let buttons = layout(mask)
            .into_iter()
            .enumerate()
            .map(|(index, (id, position))| Button {
                id,
                position,
                // the cursor starts on the first button
                hover: Tweener::new(if index == 0 { 1.0 } else { 0.0 }),
                flash: Tweener::new(0.0),
            })
            .collect();
        Self {
            textures,
            buttons,
            state: State::Waiting,
            cursor: 0,
            blink_phase: 0.0,
            sounds: Vec::new(),
            hovered: None,
            pressed: None,
        }
    }

    fn move_cursor(&mut self, target: usize) {
        if target == self.cursor {
            return;
        }
        self.sounds.push(SOUND_MOVE);
        self.buttons[self.cursor]
            .hover
            .enqueue_now(0.0, seconds(HOVER_OUT_SECONDS));
        self.cursor = target;
        self.buttons[target]
            .hover
            .enqueue_now(1.0, seconds(HOVER_IN_SECONDS));
    }

    fn pick(&mut self) {
        self.sounds.push(SOUND_DECIDE);
        minigame::flash(&mut self.buttons[self.cursor].flash);
        self.state = State::Picked;
    }
}

/// `QuizOverlayLayer` slots 18-25 (`0x71000c8ad0`-`0x71000c8e00`)
impl PointerTarget for QuizLayerImpl {
    fn pointer_hit(&mut self, position: Vec2) -> bool {
        self.hovered = None;
        if self.state == State::Waiting {
            self.hovered = self.buttons.iter().position(|button| {
                let [_, _, width, height] = BUTTONS[button.id].rect;
                let p = button.position;
                contains([p.x, p.y, p.x + width, p.y + height], position)
            });
        }
        self.hovered.is_some()
    }

    fn pointer_leave(&mut self) {
        self.hovered = None;
    }

    /// Raises the flash and moves the glow there, without a sound
    fn pointer_press(&mut self) {
        let Some(hovered) = self.hovered else { return };
        minigame::raise(&mut self.buttons[hovered].flash);
        if hovered != self.cursor {
            self.buttons[self.cursor]
                .hover
                .enqueue_now(0.0, seconds(HOVER_OUT_SECONDS));
            self.cursor = hovered;
            self.buttons[hovered]
                .hover
                .enqueue_now(1.0, seconds(HOVER_IN_SECONDS));
        }
        self.pressed = Some(hovered);
    }

    fn pointer_release(&mut self) {
        self.pressed = None;
        if let Some(hovered) = self.hovered.take() {
            self.cursor = hovered;
            self.pick();
        }
    }

    fn pointer_cancel(&mut self) {
        if let Some(pressed) = self.pressed.take() {
            minigame::lower(&mut self.buttons[pressed].flash);
        }
    }
}

impl Minigame for QuizLayerImpl {
    fn handle_input(&mut self, input: &MinigameInput) {
        if !self.is_interactive() {
            return;
        }
        let count = self.buttons.len();
        if input.confirm {
            self.pick();
        } else if input.pressed.up {
            self.move_cursor((self.cursor + count - 1) % count);
        } else if input.pressed.down {
            self.move_cursor((self.cursor + 1) % count);
        }
    }

    /// The picked button's bit index, once the pick flash is over
    fn result(&self) -> Option<i32> {
        (self.state == State::Done)
            .then(|| self.buttons.get(self.cursor).map_or(0, |b| b.id as i32))
    }

    fn is_interactive(&self) -> bool {
        self.state == State::Waiting && !self.buttons.is_empty()
    }

    fn auto_pick(&mut self) {
        if self.is_interactive() {
            self.pick();
        }
    }

    fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

pub type QuizLayer = NewDrawableLayerWrapper<QuizLayerImpl>;

impl QuizLayer {
    pub fn new(textures: Arc<QuizTextures>, mask: u32) -> Self {
        Self::from_inner(QuizLayerImpl::new(textures, mask))
    }
}

impl NewDrawableLayerNeedsSeparatePass for QuizLayerImpl {}

impl NewDrawableLayer for QuizLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        &DrawableParams {
            color_multiplier, ..
        }: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent || color_multiplier.a <= 0.0 {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );

        let transform = transform.compute_final_transform();
        let builder = minigame::builder(stencil_ref);

        pass.push_debug("QuizLayer");
        for button in &self.buttons {
            let sprite = &BUTTONS[button.id];
            let [mut u, v, width, height] = sprite.rect;
            if sprite.sheet == Sheet::Consult && self.blink_phase >= 0.5 {
                u += width;
            }
            let sheet = sheet_size(sprite.sheet);
            let texture = match sprite.sheet {
                Sheet::Button => &self.textures.button,
                Sheet::Consult => &self.textures.consult,
            };
            let p = button.position;
            Sprite::new(
                texture,
                from_top_left([p.x, p.y, p.x + width, p.y + height]),
            )
            .uv([
                u / sheet.x,
                v / sheet.y,
                (u + width) / sheet.x,
                (v + height) / sheet.y,
            ])
            .alpha(color_multiplier.a)
            .glow(button.hover.value() * 0.5)
            .flash(button.flash.value())
            .draw(pass, builder, transform);
        }
        pass.pop_debug();
    }

    fn update_from_props(&mut self, _props: &LayerProperties) {}
}

impl NewDrawableLayerFastForward for QuizLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for QuizLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        if self.state == State::Picked && self.buttons[self.cursor].flash.is_idle() {
            self.state = State::Done;
        }
        for button in &mut self.buttons {
            button.hover.update(ctx.delta_ticks);
            button.flash.update(ctx.delta_ticks);
        }
        self.blink_phase =
            (self.blink_phase + ctx.delta_ticks.as_f32() / BLINK_PERIOD_FRAMES).fract();
    }
}

impl Debug for QuizLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuizLayer")
            .field(
                "buttons",
                &self.buttons.iter().map(|b| b.id).collect::<Vec<_>>(),
            )
            .field("state", &self.state)
            .field("cursor", &self.cursor)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_stack_from_the_top_8px_apart() {
        // answers A-D plus "Ask": bits 0-3 and 6
        let layout = layout(0b100_1111);
        assert_eq!(
            layout.iter().map(|&(id, _)| id).collect::<Vec<_>>(),
            [0, 1, 2, 3, 6]
        );
        assert_eq!(layout[0].1, vec2(112.0, 292.0));
        assert_eq!(layout[1].1, vec2(112.0, 364.0));
        assert_eq!(layout[4].1, vec2(56.0, 580.0));
    }
}
