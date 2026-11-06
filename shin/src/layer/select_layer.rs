//! The story choice menu, re-implemented from the original's `shin::SelectLayer` (constructor `0x71000f00f0`,
//! update `0x71000f0eb0`, input `0x71000f1740`, draw `0x71000f13f0`), which the SELECT command creates.
//!
//! Each visible option becomes a bar (`select` from msgtex.txa, 1200 px wide, centred) with its text centred on it.
//! The bars are stacked 32 px apart, 35% of the free height from the top. They slide in from the left one after
//! another; the cursor's bar gets the `select_cur` highlight; a pick flashes, the other bars slide out to the
//! right, then the picked one fades.

use glam::{Mat4, Vec2, vec3};
use shin_core::{
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::MessageTextLayout,
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, gpu_texture::GpuTexture, quad_vertices::QuadVertices,
    render_pass::RenderPass,
};

use crate::{
    adv::assets::AdvFonts,
    layer::{
        message_layer::MessageboxTextures, text_block::TextBlock,
        user::minigame::{PointerTarget, at_rate, tween_now},
    },
    random::MinStdRand,
    render::{PreRenderContext, top_left_projection_matrix},
};

/// The original builds at most this many options
const MAX_CHOICES: usize = 12;
/// Bit of the visibility mask that shuffles the options
const SHUFFLE_BIT: u32 = 1 << 12;

const BAR_LEFT: f32 = 360.0;
const BAR_RIGHT: f32 = 1560.0;
/// The bar's caps above and below the text, and the texture rows they use
const BAR_CAP: f32 = 8.0;
const BAR_CAP_V: (f32, f32) = (0.1, 0.9);
/// Extra bar height around the text, and the gap between bars
const BAR_PADDING: f32 = 16.0;
const BAR_GAP: f32 = 32.0;
/// Share of the free screen height above the stack
const STACK_TOP: f32 = 0.35;
const TEXT_CENTRE_X: f32 = 960.0;
const TEXT_TOP: f32 = 8.0;
const TEXT_LAYOUT_WIDTH: f32 = 1000.0;
const TEXT_SIZE: f32 = 48.0;
/// The text object's outline mode 1: 3 px, which also takes 2 x 3 px off the layout width (`0x7100101360`)
const TEXT_OUTLINE: f32 = 3.0;

/// Where bars slide in from and out to, and the stagger between bars (frames)
const SLIDE_IN_FROM: f32 = -300.0;
const SLIDE_OUT_TO: f32 = 300.0;
const STAGGER: f32 = 4.0;
const SLIDE_FRAMES: f32 = 20.0;
const PICKED_FADE_FRAMES: f32 = 30.0;
/// Fade speed of the whole menu, per frame
const MENU_FADE_PER_FRAME: f32 = 0.1;

/// Highlight and flash speeds per frame (tween flag 0x40: the duration is the distance over the rate)
const HOVER_IN_RATE: f32 = 0.25;
const HOVER_OUT_RATE: f32 = 0.1;
const FLASH_PEAK: f32 = 0.7;
const FLASH_RATE: f32 = 0.2;

/// Already chosen options are tinted light blue
const SEEN_COLOR: FloatColor4 = FloatColor4::from_rgba(0.75, 0.75, 1.0, 1.0);

/// System sounds from sysse.bin
pub const SOUND_APPEAR: &str = "select";
pub const SOUND_MOVE: &str = "move";
pub const SOUND_DECIDE: &str = "seldecide";
pub const SOUND_ERROR: &str = "error";

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SelectInput {
    Up,
    Down,
    Confirm,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    /// Waiting for the menu to fade in before the bars start moving
    Appear,
    Appearing,
    Waiting,
    /// The picked bar flashes
    Picked,
    Leaving,
    Done,
}

struct Choice {
    /// Index of the option in the SELECT command
    variant: u32,
    text: TextBlock,
    y: f32,
    height: f32,
    color: FloatColor4,
    x: Tweener,
    alpha: Tweener,
    hover: Tweener,
    flash: Tweener,
}

impl Choice {
    fn tweeners(&mut self) -> [&mut Tweener; 4] {
        [
            &mut self.x,
            &mut self.alpha,
            &mut self.hover,
            &mut self.flash,
        ]
    }

    fn is_idle(&self) -> bool {
        [&self.x, &self.alpha, &self.hover, &self.flash]
            .iter()
            .all(|t| t.is_idle())
    }
}

/// Holds `tweener` at its current value for `frames`, then moves it to `target`
fn after(tweener: &mut Tweener, frames: f32, target: f32, duration: Ticks, easing: Easing) {
    if frames > 0.0 {
        let current = tweener.target_value();
        tweener.enqueue(current, Tween::linear(Ticks::from_f32(frames)));
    }
    tweener.enqueue(target, Tween { duration, easing });
}

pub struct SelectLayer {
    choices: Vec<Choice>,
    alpha: f32,
    state: State,
    cursor: Option<usize>,
    /// The bar the touch went down on (`+0x84`)
    pointer_hover: Option<usize>,
    sounds: Vec<&'static str>,
}

impl SelectLayer {
    /// `visible`: options to show (bit per option, bit 12 shuffles them); `seen`: options picked before
    pub fn new(
        ctx: &PreRenderContext,
        fonts: &AdvFonts,
        variants: &[String],
        visible: u32,
        seen: u32,
    ) -> Self {
        let layout = LayoutParams {
            layout_width: TEXT_LAYOUT_WIDTH - 2.0 * TEXT_OUTLINE,
            text_alignment: MessageTextLayout::Justify,
            line_padding_above: 0.0,
            line_padding_below: 0.0,
            line_padding_between: 4.0,
            rubi_size: 20.0,
            text_size: TEXT_SIZE,
            base_font_horizontal_scale: 1.0,
            follow_kinsoku_shori_rules: true,
            always_leave_space_for_rubi: false,
            perform_soft_breaks: true,
        };

        let mut choices = (0..)
            .zip(variants)
            .filter(|&(variant, _)| variant < 32 && visible & (1 << variant) != 0)
            .take(MAX_CHOICES)
            .map(|(variant, text)| {
                let text = TextBlock::new(ctx, &fonts.medium_font, text, layout, TEXT_OUTLINE);
                let height = text.size().y + BAR_PADDING;
                Choice {
                    variant,
                    text,
                    y: 0.0,
                    height,
                    color: if seen & (1 << variant) != 0 {
                        SEEN_COLOR
                    } else {
                        FloatColor4::WHITE
                    },
                    x: Tweener::new(SLIDE_IN_FROM),
                    alpha: Tweener::new(0.0),
                    hover: Tweener::new(0.0),
                    flash: Tweener::new(0.0),
                }
            })
            .collect::<Vec<_>>();

        if visible & SHUFFLE_BIT != 0 {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.subsec_nanos());
            let mut random = MinStdRand::new(seed);
            let count = choices.len();
            for i in 0..count.saturating_sub(1) {
                let j = i + (random.next() * (count - i) as f32) as usize;
                choices.swap(i, j.min(count - 1));
            }
        }

        let mut y = 0.0;
        for choice in &mut choices {
            choice.y = y;
            y += choice.height + BAR_GAP;
        }
        let total = (y - BAR_GAP).max(0.0);
        let top = (1080.0 - total) * STACK_TOP;
        for choice in &mut choices {
            choice.y += top;
        }

        Self {
            choices,
            alpha: 0.0,
            state: State::Appear,
            // the original starts on the first option only when a flag (`+0x79` bit 7) is set; in practice
            // (Eden, played with a controller) no option is highlighted until the first Up or Down
            cursor: None,
            pointer_hover: None,
            sounds: Vec::new(),
        }
    }

    /// Whether the options are in place and take input
    pub fn is_interactive(&self) -> bool {
        self.state == State::Waiting && self.alpha >= 1.0
    }

    /// The picked option's index in the command, once the menu has finished leaving
    pub fn chosen(&self) -> Option<u32> {
        match self.state {
            State::Done => self.cursor.map(|i| self.choices[i].variant),
            _ => None,
        }
    }

    /// System sounds to play, in order
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    fn move_cursor(&mut self, to: usize) {
        if self.cursor == Some(to) {
            return;
        }
        self.sounds.push(SOUND_MOVE);
        if let Some(from) = self.cursor {
            tween_now(&mut self.choices[from].hover, 0.0, HOVER_OUT_RATE);
        }
        tween_now(&mut self.choices[to].hover, 1.0, HOVER_IN_RATE);
        self.cursor = Some(to);
    }

    pub fn handle_input(&mut self, input: SelectInput) {
        if !self.is_interactive() || self.choices.is_empty() {
            return;
        }
        let last = self.choices.len() - 1;
        match input {
            SelectInput::Up => {
                let to = match self.cursor {
                    Some(i) if i > 0 => i - 1,
                    _ => last,
                };
                self.move_cursor(to);
            }
            SelectInput::Down => {
                let to = match self.cursor {
                    Some(i) if i < last => i + 1,
                    _ => 0,
                };
                self.move_cursor(to);
            }
            SelectInput::Confirm => match self.cursor {
                Some(i) => self.pick(i),
                None => self.sounds.push(SOUND_ERROR),
            },
        }
    }

    /// Moves the highlight without a sound (a press)
    fn point_at(&mut self, to: usize) {
        if self.cursor == Some(to) {
            return;
        }
        if let Some(from) = self.cursor {
            tween_now(&mut self.choices[from].hover, 0.0, HOVER_OUT_RATE);
        }
        tween_now(&mut self.choices[to].hover, 1.0, HOVER_IN_RATE);
        self.cursor = Some(to);
    }

    /// Picks an option without input (used while the script is fast-forwarded)
    pub fn pick_now(&mut self, index: usize) {
        if index < self.choices.len() {
            self.cursor = Some(index);
            self.state = State::Done;
        }
    }

    fn pick(&mut self, index: usize) {
        self.sounds.push(SOUND_DECIDE);
        self.cursor = Some(index);
        let flash = &mut self.choices[index].flash;
        tween_now(flash, FLASH_PEAK, FLASH_RATE);
        flash.enqueue(0.0, at_rate(FLASH_PEAK, 0.0, FLASH_RATE));
        self.state = State::Picked;
    }

    pub fn update(&mut self, delta: Ticks) {
        self.alpha = (self.alpha + delta.as_f32() * MENU_FADE_PER_FRAME).min(1.0);
        if self.alpha < 1.0 {
            return;
        }

        let slide = Ticks::from_f32(SLIDE_FRAMES);
        match self.state {
            State::Appear => {
                for (i, choice) in self.choices.iter_mut().enumerate() {
                    let delay = i as f32 * STAGGER;
                    after(&mut choice.x, delay, 0.0, slide, Easing::SineOut);
                    after(&mut choice.alpha, delay, 1.0, slide, Easing::Linear);
                }
                self.sounds.push(SOUND_APPEAR);
                self.state = State::Appearing;
            }
            State::Appearing if self.choices.iter().all(Choice::is_idle) => {
                self.state = State::Waiting;
            }
            State::Picked => {
                let picked = self.cursor.unwrap_or(0);
                if self.choices[picked].flash.is_idle() {
                    let mut delay = 0.0;
                    for (i, choice) in self.choices.iter_mut().enumerate() {
                        if i != picked {
                            after(&mut choice.x, delay, SLIDE_OUT_TO, slide, Easing::SineIn);
                            after(&mut choice.alpha, delay, 0.0, slide, Easing::Linear);
                        }
                        delay += STAGGER;
                    }
                    let alpha = &mut self.choices[picked].alpha;
                    let fade = Ticks::from_f32(PICKED_FADE_FRAMES);
                    after(alpha, delay + SLIDE_FRAMES, 0.0, fade, Easing::Linear);
                    self.state = State::Leaving;
                }
            }
            State::Leaving if self.choices.iter().all(Choice::is_idle) => {
                self.state = State::Done;
            }
            _ => {}
        }

        for choice in &mut self.choices {
            for tweener in choice.tweeners() {
                tweener.update(delta);
            }
        }
    }

    pub fn render(&self, pass: &mut RenderPass, textures: &MessageboxTextures) {
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let projection = top_left_projection_matrix();

        pass.push_debug("SelectLayer");
        for choice in &self.choices {
            let alpha = choice.alpha.value() * self.alpha;
            let transform =
                projection * Mat4::from_translation(vec3(choice.x.value(), choice.y, 0.0));

            render_bar(
                pass,
                builder,
                &textures.select,
                transform,
                choice.height,
                alpha,
            );
            let text_offset = Vec2::new(TEXT_CENTRE_X - choice.text.size().x * 0.5, TEXT_TOP);
            choice.text.render(
                pass,
                builder,
                transform * Mat4::from_translation(text_offset.extend(0.0)),
                choice.color * FloatColor4::WHITE.with_alpha(alpha),
                FloatColor4::BLACK.with_alpha(alpha),
            );
            let hover = alpha * choice.hover.value() * 0.5;
            render_bar(
                pass,
                builder,
                &textures.select_cursor,
                transform,
                choice.height,
                hover,
            );
            let flash = choice.flash.value();
            render_bar(
                pass,
                builder,
                &textures.select_cursor,
                transform,
                choice.height,
                flash,
            );
        }
        pass.pop_debug();
    }
}

/// One bar: the texture's top and bottom rows as 8 px caps around a body `height` tall (`0x71000f1280`)
fn render_bar(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    texture: &GpuTexture,
    transform: Mat4,
    height: f32,
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    let color = FloatColor4::WHITE.with_alpha(alpha).into_unorm();
    let (v_top, v_bottom) = BAR_CAP_V;
    for (top, bottom, v0, v1) in [
        (-BAR_CAP, 0.0, 0.0, v_top),
        (0.0, height, v_top, v_bottom),
        (height, height + BAR_CAP, v_bottom, 1.0),
    ] {
        QuadVertices::new()
            .with_box(BAR_LEFT, top, BAR_RIGHT, bottom)
            .with_tex_box(0.0, v0, 1.0, v1)
            .with_color(color)
            .render_sprite(pass, builder, texture.as_source(), transform);
    }
}

/// Bars are hit between these x (`0x710015a668`, `0x710015a888`)
const POINTER_X: (f32, f32) = (460.0, 1460.0);

/// Pointer slots (`0x71000f1940` hit test, `0x71000f19e0` press, `0x71000f1aa0` release; the ADV listener's press
/// `0x7100015670` and long press `0x7100015c00`): a press moves the highlight there and raises its flash, a tap
/// picks it, a long press, a drag or a cancel drops the flash and picks nothing
impl PointerTarget for SelectLayer {
    fn pointer_hit(&mut self, position: Vec2) -> bool {
        self.pointer_hover = None;
        if self.is_interactive() && (POINTER_X.0..POINTER_X.1).contains(&position.x) {
            self.pointer_hover = self
                .choices
                .iter()
                .position(|choice| (choice.y..choice.y + choice.height).contains(&position.y));
        }
        self.pointer_hover.is_some()
    }

    fn pointer_press(&mut self) {
        let Some(hovered) = self.pointer_hover.filter(|_| self.is_interactive()) else {
            return;
        };
        self.point_at(hovered);
        tween_now(&mut self.choices[hovered].flash, FLASH_PEAK, FLASH_RATE);
    }

    fn pointer_release(&mut self) {
        if let Some(hovered) = self.pointer_hover.take().filter(|_| self.is_interactive()) {
            self.point_at(hovered);
            self.pick(hovered);
        }
    }

    fn pointer_cancel(&mut self) {
        if let Some(hovered) = self.pointer_hover.take().filter(|_| self.is_interactive()) {
            tween_now(&mut self.choices[hovered].flash, 0.0, FLASH_RATE);
        }
    }

    /// The press's highlight move, without the flash
    fn pointer_hover(&mut self, position: Vec2) {
        if self.pointer_hit(position) {
            let hovered = self.pointer_hover.take().unwrap();
            self.point_at(hovered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_tween_holds_then_moves() {
        let mut tweener = Tweener::new(-300.0);
        after(
            &mut tweener,
            8.0,
            0.0,
            Ticks::from_f32(20.0),
            Easing::Linear,
        );
        tweener.update(Ticks::from_f32(8.0));
        assert_eq!(tweener.value(), -300.0);
        tweener.update(Ticks::from_f32(20.0));
        assert_eq!(tweener.value(), 0.0);
    }
}
