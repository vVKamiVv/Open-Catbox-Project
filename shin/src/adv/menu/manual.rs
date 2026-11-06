//! The manual (`shin::ManualViewer`, ctor `0x7100085530`, update `0x7100085930`, render `0x7100085980`, keys
//! `0x71000865d0`): the pages `001`, `002`, ... of `/manual.txa` side by side, 1500 px apart, turned with left/right
//! or L/R. The English patch has no `manual.txa`, so the English game shows the Japanese pages, as here.
//!
//! shortcut: the pages cannot be dragged or flicked with the pointer.

use std::sync::Arc;

use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, resize::CanvasSize,
};
use tracing::warn;

use super::{Task, TaskCx};
use crate::{
    asset::texture_archive::NamedTextures,
    layer::user::minigame::{self, Sprite},
    ui,
};

const PAGE_STEP: f32 = 1500.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// `Start $_0`: the screen opens over 10 frames
    Opening,
    Active,
    /// The close button flashes
    Closing,
    /// `Close $_1`: the screen fades over 10 frames
    Fading,
}

pub struct Manual {
    textures: Arc<NamedTextures>,
    pages: usize,
    page: usize,
    /// The open fraction (`+0xc0`)
    open: f32,
    scroll: Tweener,
    close_flash: Tweener,
    state: State,
    target_size: (u32, u32),
}

impl Manual {
    pub fn new(cx: &TaskCx) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/manual.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /manual.txa: {e:?}");
                return None;
            }
        };
        // `0x71000862d0`: as many pages as there are consecutive textures `001`, `002`, ...
        let pages = (1..)
            .take_while(|k| textures.contains(&format!("{k:03}")))
            .count();
        Some(Self {
            textures,
            pages,
            page: 0,
            open: 0.0,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            state: State::Opening,
            target_size: (1920, 1080),
        })
    }

    /// `0x71000865d0`
    fn handle_input(&mut self, cx: &TaskCx) {
        let input = cx.input;
        let to = if input.left || input.l {
            self.page.saturating_sub(1)
        } else if input.right || input.r {
            (self.page + 1).min(self.pages.saturating_sub(1))
        } else {
            self.page
        };
        if to != self.page {
            cx.sound("move");
            self.page = to;
            // 100 px per frame, sine-out: 15 frames a page
            let target = PAGE_STEP * to as f32;
            let frames = (target - self.scroll.value()).abs() / 100.0;
            self.scroll = Tweener::new(self.scroll.value());
            ui::tween(&mut self.scroll, target, frames, Easing::SineOut);
        } else if input.cancel {
            // `0x7100086470`
            cx.sound("cancel");
            self.close_flash = Tweener::new(0.0);
            minigame::flash(&mut self.close_flash);
            self.state = State::Closing;
        }
    }
}

impl Task for Manual {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let size = cx.pre_render.resize_source.handle::<CanvasSize>().get();
        self.target_size = (size.width, size.height);
        match self.state {
            State::Opening => {
                self.open = (self.open + 0.1 * cx.frames).min(1.0);
                if self.open >= 1.0 {
                    self.state = State::Active;
                }
            }
            State::Active => {
                if cx.focused && self.scroll.is_idle() {
                    self.handle_input(cx);
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    self.state = State::Fading;
                }
            }
            State::Fading => {
                self.open = (self.open - 0.1 * cx.frames).max(0.0);
                if self.open <= 0.0 {
                    return false;
                }
            }
        }
        ui::advance(&mut self.scroll, cx.frames);
        ui::advance(&mut self.close_flash, cx.frames);
        true
    }

    fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let a = self.open;
        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0])
            .alpha(a)
            .draw(pass, builder, projection);
        draw_obi(pass, t("obi"), a);

        // the page strip slides in from the right, clipped to the band between the obi
        let strip =
            projection * glam::Mat4::from_translation(glam::vec3(1920.0 - 1920.0 * a, 0.0, 0.0));
        let scroll = self.scroll.value();
        ui::clip(pass, Some([0.0, 162.0, 1920.0, 980.0]), self.target_size);
        for i in 0..self.pages {
            let offset = PAGE_STEP * i as f32 - scroll;
            let x = offset + 240.0;
            if x >= 1920.0 || x + 1440.0 <= 0.0 {
                continue;
            }
            let alpha = (1.0 - (offset / PAGE_STEP).abs() * 0.5).max(0.5);
            Sprite::new(t(&format!("{:03}", i + 1)), [x, 166.0, x + 1440.0, 976.0])
                .alpha(alpha)
                .draw(pass, builder, strip);
        }
        ui::clip(pass, None, self.target_size);

        let top = 160.0 * a;
        Sprite::new(t("caption"), [480.0, top - 160.0, 1440.0, top - 32.0])
            .draw(pass, builder, projection);
        Sprite::new(t("close"), [1764.0, top - 128.0, 1848.0, top - 44.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left (`0x71000860a0`, `0x7100171800`)
        let help = KeyHelp {
            labels: t("help_manual"),
            keys: t("key"),
            label_height: 96.0,
            y: 1080.0 - 88.0 * a + 20.0,
            glyph_offset: 2.0,
            gap: 16.0,
        };
        let x = help.draw(pass, 1872.0, &[4], 48.0, 72.0, FloatColor4::WHITE);
        help.draw(pass, x, &[7, 2], 0.0, 236.0, FloatColor4::WHITE);
    }
}

/// The two translucent bands that close in from the top and the bottom (blend 3)
pub(super) fn draw_obi(pass: &mut RenderPass, obi: &shin_render::gpu_texture::GpuTexture, a: f32) {
    let additive = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
    let projection = ui::projection();
    let top = 160.0 * a;
    Sprite::new(obi, [0.0, top - 160.0, 1920.0, top])
        .uv([0.0, 0.0, 1.0, 0.5])
        .alpha(0.25)
        .draw(pass, additive, projection);
    Sprite::new(obi, [0.0, 1080.0 - top, 1920.0, 1240.0 - top])
        .uv([0.0, 0.5, 1.0, 1.0])
        .alpha(0.25)
        .draw(pass, additive, projection);
}

/// The key help of the paged screens: label rows of 48 in a 320-wide texture, the keys' glyphs to their left
pub(super) struct KeyHelp<'a> {
    pub labels: &'a shin_render::gpu_texture::GpuTexture,
    pub keys: &'a shin_render::gpu_texture::GpuTexture,
    pub label_height: f32,
    pub y: f32,
    /// From the label's left edge to the first glyph's right edge
    pub glyph_offset: f32,
    /// From the last glyph to the next entry's right edge
    pub gap: f32,
}

impl KeyHelp<'_> {
    /// An entry ending at `right`; returns the next entry's right edge
    pub fn draw(
        &self,
        pass: &mut RenderPass,
        right: f32,
        glyphs: &[u8],
        row: f32,
        width: f32,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let y = self.y;
        let v = row / self.label_height;
        let mut label = Sprite::new(self.labels, [right - width, y, right, y + 48.0]).uv([
            0.0,
            v,
            width / 320.0,
            v + 48.0 / self.label_height,
        ]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        let mut x = right - width + self.glyph_offset;
        for &key in glyphs {
            x -= 48.0;
            let (u, v) = ((key & 7) as f32 * 0.125, (key >> 3) as f32 * 0.5);
            let mut icon =
                Sprite::new(self.keys, [x, y, x + 48.0, y + 48.0]).uv([u, v, u + 0.125, v + 0.5]);
            icon.color = color;
            icon.draw(pass, ui::builder(), projection);
        }
        x - self.gap
    }
}
