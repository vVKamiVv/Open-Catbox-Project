//! The touch buttons a leftward flick slides in over the story (`ADV+0x4f8`/`+0x500`, entries `+0x558`..`+0x8f0`):
//! auto, skip, backlog, previous and next chapter, 192 x 120 each, from `navi` in `/adv.txa` (one 80 px row each).
//!
//! Places (`0x7100019580`): auto (1332, 160), skip (1524, 160), backlog (1716, 160), previous chapter (1428, 292),
//! next chapter (1620, 292); they slide from 588 px to the right as the panel comes in (`+0x500`, 0.1 a frame).
//! Each button fades to enabled at 0.25 a frame or to disabled at 0.1 (`0x7100011de0`), drawn grey at
//! `0.5 + 0.5 x` that, with its press highlight added in white (render `0x71000125a0`). The hit test
//! (`0x7100015130`) works only while the panel is all the way in.

use glam::Vec2;
use shin_core::{primitives::color::FloatColor4, time::Tweener};
use shin_render::{gpu_texture::GpuTexture, render_pass::RenderPass};

use crate::{
    layer::user::minigame::{self, Sprite},
    render::top_left_projection_matrix,
    ui,
};

/// What each button does, also its row in `navi`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchButton {
    Auto,
    Skip,
    Backlog,
    PreviousChapter,
    NextChapter,
}

const BUTTONS: [(TouchButton, f32, f32); 5] = [
    (TouchButton::Auto, 1332.0, 160.0),
    (TouchButton::Skip, 1524.0, 160.0),
    (TouchButton::Backlog, 1716.0, 160.0),
    (TouchButton::PreviousChapter, 1428.0, 292.0),
    (TouchButton::NextChapter, 1620.0, 292.0),
];
const W: f32 = 192.0;
const H: f32 = 120.0;
/// The auto-hide while auto or skip runs untouched (`0x7100010fb0`, 60 000 000 µs)
const IDLE_SECONDS: f32 = 60.0;

struct Button {
    enabled: bool,
    /// 1 enabled, 0 disabled (`+0x560`)
    enabled_fade: Tweener,
    /// The press highlight (`+0x5b8`)
    highlight: Tweener,
}

pub struct TouchButtons {
    shown: bool,
    /// The slide in (`+0x500`)
    slide: Tweener,
    buttons: [Button; 5],
    /// Seconds of auto or skip without a touch
    idle: f32,
}

impl Default for Button {
    fn default() -> Self {
        Self {
            enabled: false,
            enabled_fade: Tweener::new(0.0),
            highlight: Tweener::new(0.0),
        }
    }
}

impl Default for TouchButtons {
    fn default() -> Self {
        Self {
            shown: false,
            slide: Tweener::new(0.0),
            buttons: Default::default(),
            idle: 0.0,
        }
    }
}

/// What a touch went down on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A disabled button: the touch is the panel's but does nothing
    Disabled,
    Button(usize),
}

impl TouchButtons {
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// All the way in: only then do the buttons take touches
    fn fully_shown(&self) -> bool {
        // the original compares with FLT_EPSILON; the port's tweens can end a rounding step short of 1
        self.shown && self.slide.value() > 0.999
    }

    /// Slides the panel in (the leftward flick)
    pub fn show(&mut self) {
        if !self.shown {
            self.shown = true;
            tracing::debug!("Touch buttons in");
            self.idle = 0.0;
            minigame::tween_now(&mut self.slide, 1.0, 0.1);
        }
    }

    /// Slides the panel out; false when it was not in
    pub fn hide(&mut self) -> bool {
        if !self.shown {
            return false;
        }
        self.shown = false;
        tracing::debug!("Touch buttons out");
        minigame::tween_now(&mut self.slide, 0.0, 0.1);
        true
    }

    /// Whether a rightward flick hides the panel instead of opening the system menu: it is all the way in
    pub fn flick_hides(&self) -> bool {
        self.fully_shown()
    }

    /// `0x7100015130`: the button under `point`, while the panel is all the way in
    pub fn hit(&self, point: Vec2) -> Option<Hit> {
        if !self.fully_shown() {
            return None;
        }
        let k = BUTTONS
            .iter()
            .position(|&(_, x, y)| point.x >= x && point.x < x + W && point.y >= y && point.y < y + H)?;
        Some(if self.buttons[k].enabled {
            Hit::Button(k)
        } else {
            Hit::Disabled
        })
    }

    pub fn action(index: usize) -> TouchButton {
        BUTTONS[index].0
    }

    /// The press over an enabled button: its highlight up to 0.7 at 0.2 a frame
    pub fn press(&mut self, index: usize) {
        minigame::tween_now(&mut self.buttons[index].highlight, 0.7, 0.2);
    }

    /// A drag, a long press or the end of the touch: the highlight back down at 0.2 a frame
    pub fn release(&mut self, index: usize) {
        minigame::tween_now(&mut self.buttons[index].highlight, 0.0, 0.2);
    }

    /// The tap: the highlight flashes up to 0.7 and back, at 0.2 a frame
    pub fn flash(&mut self, index: usize) {
        let highlight = &mut self.buttons[index].highlight;
        minigame::tween_now(highlight, 0.7, 0.2);
        highlight.enqueue(0.0, minigame::at_rate(0.7, 0.0, 0.2));
    }

    /// Every frame: which buttons are enabled (`0x7100010fb0`), whether auto or skip runs (the idle clock only runs
    /// then) and whether the pointer is down (which restarts it)
    pub fn update(&mut self, frames: f32, enabled: [bool; 5], auto_or_skip: bool, touched: bool) {
        for (button, enabled) in self.buttons.iter_mut().zip(enabled) {
            if enabled != button.enabled {
                button.enabled = enabled;
                if enabled {
                    minigame::tween_now(&mut button.enabled_fade, 1.0, 0.25);
                } else {
                    minigame::tween_now(&mut button.enabled_fade, 0.0, 0.1);
                    minigame::tween_now(&mut button.highlight, 0.0, 0.2);
                }
            }
            ui::advance(&mut button.enabled_fade, frames);
            ui::advance(&mut button.highlight, frames);
        }
        ui::advance(&mut self.slide, frames);

        if auto_or_skip && !touched {
            self.idle += frames / 60.0;
            if self.idle >= IDLE_SECONDS {
                self.hide();
            }
        } else {
            self.idle = 0.0;
        }
    }

    pub fn render(&self, pass: &mut RenderPass, navi: &GpuTexture) {
        let slide = self.slide.value();
        if slide <= 0.0 {
            return;
        }
        let projection = top_left_projection_matrix();
        let offset = (1.0 - slide) * 588.0;
        for (k, &(button, x, y)) in BUTTONS.iter().enumerate() {
            let x = x + offset;
            let v = button as usize as f32 * 80.0 / 400.0;
            let grey = 0.5 + 0.5 * self.buttons[k].enabled_fade.value();
            let mut sprite = Sprite::new(navi, [x, y, x + W, y + H])
                .uv([0.0, v, 1.0, v + 0.2])
                .flash(self.buttons[k].highlight.value());
            sprite.color = FloatColor4::from_rgba(grey, grey, grey, 1.0);
            sprite.draw(pass, ui::builder(), projection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(buttons: &mut TouchButtons) {
        for _ in 0..30 {
            buttons.update(1.0, [true, true, false, true, true], false, false);
        }
    }

    #[test]
    fn buttons_take_touches_only_when_all_the_way_in() {
        let mut buttons = TouchButtons::default();
        assert_eq!(buttons.hit(Vec2::new(1400.0, 200.0)), None);
        buttons.show();
        buttons.update(1.0, [true; 5], false, false);
        assert_eq!(buttons.hit(Vec2::new(1400.0, 200.0)), None);
        settle(&mut buttons);
        assert_eq!(buttons.hit(Vec2::new(1400.0, 200.0)), Some(Hit::Button(0)));
        assert_eq!(buttons.hit(Vec2::new(1800.0, 200.0)), Some(Hit::Disabled));
        assert_eq!(buttons.hit(Vec2::new(1650.0, 300.0)), Some(Hit::Button(4)));
        assert_eq!(buttons.hit(Vec2::new(1400.0, 300.0)), None);
    }

    #[test]
    fn auto_or_skip_hides_them_after_a_minute_untouched() {
        let mut buttons = TouchButtons::default();
        buttons.show();
        settle(&mut buttons);
        for _ in 0..3599 {
            buttons.update(1.0, [true; 5], true, false);
        }
        assert!(buttons.shown());
        buttons.update(1.0, [true; 5], true, true);
        for _ in 0..3599 {
            buttons.update(1.0, [true; 5], true, false);
        }
        assert!(buttons.shown());
        buttons.update(1.0, [true; 5], true, false);
        buttons.update(1.0, [true; 5], true, false);
        assert!(!buttons.shown());
    }
}
