//! What the adventure screen draws over its layers (ADV render `0x71000125a0`): the scene-title banner (AUTOSAVE), the
//! song-title banner (a new BGM), the skip and auto icons, the "Characters/Tips updated" notification and the
//! event-skip hint, and the touch buttons (`navi`, [`super::touch_buttons`]), all in 1920x1080 top-left space with
//! textures from `/adv.txa`.

use std::{collections::VecDeque, sync::Arc};

use glam::{Mat4, vec3};
use shin_core::{
    layout::LayoutParams, primitives::color::FloatColor4,
    vm::command::types::MessageTextLayout,
};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};

use crate::{
    asset::{font::GpuFontLazy, system::AssetServer, texture_archive::NamedTextures},
    audio::play_sys_se,
    layer::{text_block::TextBlock, user::minigame::Sprite},
    render::{PreRenderContext, top_left_projection_matrix},
};

/// A text object's outline mode 1: a 3 px outline inside 3 px of padding (`0x7100101360`)
pub(crate) const OUTLINE: f32 = 3.0;

/// One step of the original's tweener (`0x71000bb620`)
#[derive(Debug, Clone, Copy)]
struct Step {
    target: f32,
    frames: f32,
    sine_out: bool,
    /// Flag 0x1000: skipping stops here until the step has played (`0x71000bb470`)
    stop: bool,
}

/// A value moving through a fixed sequence of steps
#[derive(Debug, Clone, Default)]
struct Steps {
    start: f32,
    steps: Vec<Step>,
    index: usize,
    elapsed: f32,
}

impl Steps {
    fn new(start: f32, steps: Vec<Step>) -> Self {
        Self {
            start,
            steps,
            index: 0,
            elapsed: 0.0,
        }
    }

    fn running(&self) -> bool {
        self.index < self.steps.len()
    }

    fn step_start(&self, index: usize) -> f32 {
        if index == 0 {
            self.start
        } else {
            self.steps[index - 1].target
        }
    }

    fn value(&self) -> f32 {
        let Some(step) = self.steps.get(self.index) else {
            return self.steps.last().map_or(self.start, |step| step.target);
        };
        let from = self.step_start(self.index);
        let t = if step.frames > 0.0 {
            (self.elapsed / step.frames).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let t = if step.sine_out {
            (t * std::f32::consts::FRAC_PI_2).sin()
        } else {
            t
        };
        from + (step.target - from) * t
    }

    fn update(&mut self, mut frames: f32) {
        while let Some(step) = self.steps.get(self.index) {
            let left = step.frames - self.elapsed;
            if frames < left {
                self.elapsed += frames;
                return;
            }
            frames -= left.max(0.0);
            self.index += 1;
            self.elapsed = 0.0;
        }
    }

    /// Skipping (`0x71000bb470`): completes steps up to a stop point that has not played yet
    fn finish(&mut self) {
        while let Some(step) = self.steps.get(self.index) {
            if step.stop {
                return;
            }
            self.index += 1;
            self.elapsed = 0.0;
        }
    }
}

fn linear(target: f32, frames: f32) -> Step {
    Step {
        target,
        frames,
        sine_out: false,
        stop: false,
    }
}

/// A hold that skipping plays in real time (flags `0x1080`)
fn stop_hold(value: f32, frames: f32) -> Step {
    Step {
        target: value,
        frames,
        sine_out: false,
        stop: true,
    }
}

/// The layout of a HUD text object: `width` wide with mode-1 padding, `size`, justified, room for rubi
pub(crate) fn text_layout(width: f32, size: f32) -> LayoutParams {
    LayoutParams {
        layout_width: width - 2.0 * OUTLINE,
        text_alignment: MessageTextLayout::Justify,
        line_padding_above: 0.0,
        line_padding_below: 0.0,
        line_padding_between: 0.0,
        rubi_size: size * 0.4,
        text_size: size,
        base_font_horizontal_scale: 1.0,
        follow_kinsoku_shori_rules: true,
        always_leave_space_for_rubi: true,
        perform_soft_breaks: true,
        word_wrap: true,
    }
}

/// Draws a text object whose top-left corner (including its padding) is at the origin of `transform`
pub(crate) fn draw_text(pass: &mut RenderPass, text: &TextBlock, transform: Mat4, tint: FloatColor4, alpha: f32) {
    text.render(
        pass,
        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        transform * Mat4::from_translation(vec3(OUTLINE, OUTLINE, 0.0)),
        tint.with_alpha(alpha),
        FloatColor4::BLACK.with_alpha(alpha),
    );
}

struct SceneTitle {
    text: TextBlock,
    alpha: Steps,
}

struct SongTitle {
    title: TextBlock,
    note: TextBlock,
    /// How far the banner has slid down from above the screen
    y: Steps,
    /// Frames since the banner started (the note's bob)
    clock: f32,
}

/// What the HUD shows for the current frame's modes
#[derive(Debug, Default, Clone, Copy)]
pub struct HudModes {
    pub skip_on: bool,
    pub auto_on: bool,
    pub skipping: bool,
    pub auto: bool,
    /// Setting v11: skipping passes read lines only (the skip icon pulses blue, else red)
    pub skip_read_only: bool,
    /// Settings v14, v15: the scene-title and song-title banners
    pub show_scene_title: bool,
    pub show_song_title: bool,
}

pub struct AdvHud {
    textures: Arc<NamedTextures>,
    system_font: Arc<GpuFontLazy>,
    scene_title: Option<SceneTitle>,
    song_title: Option<SongTitle>,
    /// Pending notification ids (bit 0: tips, bit 4: level 2), at most one per kind (`0x71000148f0`)
    notifications: VecDeque<u8>,
    notification: Option<u8>,
    notify_slide: Steps,
    notify_alpha: Steps,
    hint_alpha: Steps,
    /// The hint strip's row: 0 for the buttons' text, 0.5 (row 2) for the touch screen's (`ADV+0x49c`)
    hint_v: f32,
    /// Seconds since the screen started, for the icons' pulse (`ADV+0x498`)
    clock: f32,
    modes: HudModes,
    /// The banners' hidden flags (`0x7100064230`, `0x710009e150`), set while the interface is hidden
    hidden: bool,
    /// The touch buttons a leftward flick slides in, drawn just before the hint as the original does
    pub touch_buttons: super::touch_buttons::TouchButtons,
}

impl AdvHud {
    pub fn new(textures: Arc<NamedTextures>, system_font: Arc<GpuFontLazy>) -> Self {
        Self {
            textures,
            system_font,
            scene_title: None,
            song_title: None,
            notifications: VecDeque::new(),
            notification: None,
            notify_slide: Steps::default(),
            notify_alpha: Steps::default(),
            hint_alpha: Steps::default(),
            hint_v: 0.0,
            clock: 0.0,
            modes: HudModes::default(),
            hidden: false,
            touch_buttons: Default::default(),
        }
    }

    /// AUTOSAVE (`0x7100064240`): the scene's title bottom left, in at 0.03 per frame, 240 frames, out at 0.03
    pub fn show_scene_title(&mut self, ctx: &PreRenderContext, text: &str) {
        tracing::debug!("Scene title: {text:?}");
        if text.is_empty() {
            self.scene_title = None;
            return;
        }
        let in_frames = 1.0 / 0.03;
        self.scene_title = Some(SceneTitle {
            text: TextBlock::new(ctx, &self.system_font, text, text_layout(1920.0, 48.0), OUTLINE),
            alpha: Steps::new(0.0, vec![
                linear(1.0, in_frames),
                stop_hold(1.0, 240.0),
                linear(0.0, in_frames),
            ]),
        });
    }

    /// A new BGM (`0x710009e160`): its display name slides down from above the top left at 2 px per frame, stays 180
    /// frames and slides back
    pub fn show_song_title(&mut self, ctx: &PreRenderContext, display_name: &str) {
        tracing::debug!("Song title: {display_name:?}");
        if display_name.is_empty() {
            return;
        }
        let layout = text_layout(1920.0, 36.0);
        let title = TextBlock::new(ctx, &self.system_font, display_name, layout, OUTLINE);
        let note = TextBlock::new(ctx, &self.system_font, "\u{266a}", layout, OUTLINE);
        let distance = note.size().y + 4.0;
        self.song_title = Some(SongTitle {
            title,
            note,
            y: Steps::new(0.0, vec![
                linear(distance, distance / 2.0),
                stop_hold(distance, 180.0),
                linear(0.0, distance / 2.0),
            ]),
            clock: 0.0,
        });
    }

    /// CHARS and TIPSGET: a "Characters/Tips updated" banner, one pending per kind (`0x71000148f0`)
    pub fn queue_notification(&mut self, id: u8) {
        if self.notification == Some(id) {
            return;
        }
        match self
            .notifications
            .iter_mut()
            .find(|pending| (**pending ^ id) & 1 == 0)
        {
            Some(pending) => *pending = id,
            None => self.notifications.push_back(id),
        }
    }

    /// Confirm inside an event: "Press B to Skip." for 32 + 120 + 32 frames (`0x7100011f2c`), or with `touch` (a
    /// tap, `0x71000157d0`) the touch screen's hint; false when already shown, which keeps its row
    pub fn show_event_hint(&mut self, touch: bool) -> bool {
        if self.hint_alpha.running() {
            return false;
        }
        self.hint_v = if touch { 0.5 } else { 0.0 };
        self.hint_alpha = Steps::new(0.0, vec![
            linear(1.0, 32.0),
            linear(1.0, 120.0),
            linear(0.0, 32.0),
        ]);
        true
    }

    /// Hides the scene-title and song-title banners, or shows them again
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    /// Advances the HUD by `frames` (`0x7100010fb0`)
    pub fn update(&mut self, frames: f32, modes: HudModes, audio: &shin_audio::AudioManager, assets: &AssetServer) {
        self.modes = modes;
        self.clock += frames / 60.0;

        if let Some(scene) = &mut self.scene_title {
            if modes.skipping {
                scene.alpha.finish();
            }
            scene.alpha.update(frames);
            if !scene.alpha.running() {
                self.scene_title = None;
            }
        }
        if let Some(song) = &mut self.song_title {
            if modes.skipping {
                song.y.finish();
            }
            song.y.update(frames);
            song.clock += frames;
            if !song.y.running() {
                self.song_title = None;
            }
        }

        if !self.notify_alpha.running() {
            self.notification = self.notifications.pop_front();
            if let Some(id) = self.notification {
                self.notify_slide = Steps::new(0.0, vec![
                    Step {
                        target: 1.0,
                        frames: 30.0,
                        sine_out: true,
                        stop: false,
                    },
                    stop_hold(1.0, 30.0),
                    linear(1.0, 180.0),
                ]);
                self.notify_alpha = Steps::new(0.0, vec![
                    linear(1.0, 30.0),
                    stop_hold(1.0, 30.0),
                    linear(1.0, 120.0),
                    linear(0.0, 60.0),
                ]);
                let sound = if id & 0x10 != 0 { "notify2" } else { "notify1" };
                play_sys_se(audio, assets, sound, Default::default());
            }
        }
        if modes.skipping {
            self.notify_slide.finish();
            self.notify_alpha.finish();
        }
        self.notify_slide.update(frames);
        self.notify_alpha.update(frames);
        self.hint_alpha.update(frames);
    }

    /// Draws the HUD in the original's order (`0x71000125a0`)
    pub fn render(&self, pass: &mut RenderPass) {
        let projection = top_left_projection_matrix();
        let modes = self.modes;

        if let Some(scene) = &self.scene_title.as_ref().filter(|_| modes.show_scene_title && !self.hidden) {
            // bottom-left corner at (4, 1076)
            let top = 1076.0 - scene.text.size().y;
            let transform = projection * Mat4::from_translation(vec3(4.0, top, 0.0));
            draw_text(pass, &scene.text, transform, FloatColor4::WHITE, scene.alpha.value());
        }

        if let Some(song) = &self.song_title.as_ref().filter(|_| modes.show_song_title && !self.hidden) {
            let y = song.y.value();
            let [w, h] = song.note.size().to_array();
            let tint = FloatColor4::from_rgba(0.75, 1.0, 1.0, 1.0);
            let title_top = y - song.title.size().y;
            let transform = projection * Mat4::from_translation(vec3(w + 4.0, title_top, 0.0));
            draw_text(pass, &song.title, transform, tint, 1.0);

            // the note bobs up on an 80-frame cycle, and squashes on its baseline in the other half
            let s = (song.clock / 40.0 * std::f32::consts::PI).sin();
            let (note_y, scale) = if s >= 0.0 {
                (y - 8.0 * s, (1.0, 1.0))
            } else {
                (y, (1.0 - 0.4 * s, 1.0 + 0.3 * s))
            };
            let transform = projection
                * Mat4::from_translation(vec3(w / 2.0 + 4.0 - 0.5 * w * scale.0, note_y - h * scale.1, 0.0))
                * Mat4::from_scale(vec3(scale.0, scale.1, 1.0));
            draw_text(pass, &song.note, transform, tint, 1.0);
        }

        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let pulse = 0.75 + 0.25 * (std::f32::consts::TAU * self.clock).sin();
        let grey = FloatColor4::from_rgba(0.25, 0.25, 0.25, 1.0);
        let icons = self.textures.get("adv");
        if modes.skip_on {
            let color = match (modes.skipping, modes.skip_read_only) {
                (false, _) => grey,
                (true, true) => FloatColor4::from_rgba(pulse, pulse, 1.0, 1.0),
                (true, false) => FloatColor4::from_rgba(1.0, pulse, pulse, 1.0),
            };
            let mut sprite = Sprite::new(icons, [1824.0, 24.0, 1896.0, 96.0]).uv([0.0, 0.5, 1.0, 1.0]);
            sprite.color = color;
            sprite.draw(pass, builder, projection);
        }
        if modes.auto_on {
            let x = if modes.skip_on { 1744.0 } else { 1824.0 };
            let color = if modes.auto {
                FloatColor4::from_rgba(pulse, 1.0, pulse, 1.0)
            } else {
                grey
            };
            let mut sprite = Sprite::new(icons, [x, 24.0, x + 72.0, 96.0]).uv([0.0, 0.0, 1.0, 0.5]);
            sprite.color = color;
            sprite.draw(pass, builder, projection);
        }

        if let Some(id) = self.notification.filter(|_| self.notify_alpha.running()) {
            let x = 1380.0 - 100.0 * self.notify_slide.value();
            let v = (id & 1) as f32 * 0.5;
            let sprite = Sprite::new(self.textures.get("notify"), [x, 100.0, x + 640.0, 164.0])
                .uv([0.0, v, 1.0, v + 0.5])
                .alpha(self.notify_alpha.value());
            sprite.draw(pass, builder, projection);
        }

        self.touch_buttons.render(pass, self.textures.get("navi"));

        let hint = self.hint_alpha.value();
        if self.hint_alpha.running() && hint > 0.0 {
            // row 0: "Press B to Skip.", row 2: the touch screen's
            // shortcut: the original picks row 1 for the buttons' hint by a system flag (`0x71000fc1f0`); always row 0
            let sprite = Sprite::new(self.textures.get("skip"), [16.0, 1000.0, 656.0, 1064.0])
                .uv([0.0, self.hint_v, 1.0, self.hint_v + 0.25])
                .alpha(hint);
            sprite.draw(pass, builder, projection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_stops_at_a_hold_until_it_has_played() {
        let mut steps = Steps::new(0.0, vec![linear(1.0, 10.0), stop_hold(1.0, 20.0), linear(0.0, 10.0)]);
        steps.finish();
        assert_eq!(steps.value(), 1.0);
        assert!(steps.running());
        steps.update(19.0);
        steps.finish();
        assert!(steps.running());
        steps.update(1.0);
        steps.finish();
        assert!(!steps.running());
        assert_eq!(steps.value(), 0.0);
    }

    #[test]
    fn steps_carry_the_leftover_time() {
        let mut steps = Steps::new(0.0, vec![linear(1.0, 10.0), linear(0.0, 10.0)]);
        steps.update(15.0);
        assert!((steps.value() - 0.5).abs() < 1e-6);
    }
}
