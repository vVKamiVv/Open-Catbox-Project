//! The tips (`shin::TipsMode`, ctor `0x71001072f0`, update `0x7100108070`, render `0x71001081f0`, keys
//! `0x7100109630`): a list of tip titles on the left and the chosen tip's title and text on the right, in a window
//! that slides in from the right. From the title it lists every tip of one episode; from the system menu, the tips
//! this playthrough has collected. The pointer picks tips from the list and drags and flicks the text.

use std::sync::Arc;

use glam::{Mat4, Vec2, vec3};
use shin_core::{layout::LayoutParams, primitives::color::FloatColor4, time::Tweener};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, resize::CanvasSize,
};
use tracing::warn;

use super::{
    MenuAssets, Task, TaskCx,
    manual::{KeyHelp, draw_obi},
};
use crate::{
    adv::hud::{OUTLINE, text_layout},
    asset::texture_archive::NamedTextures,
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    render::PreRenderContext,
    ui::{
        self,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
};

/// The text view's height (`0x710015aa50`)
const VIEW: f32 = 730.0;
const TEXT_X: f32 = 602.0;
const TEXT_Y: f32 = 188.0;
const CLIP: [f32; 4] = [86.0, 183.0, 1834.0, 923.0];
/// The texts' outline colour (`0x710015aa4c`)
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.18431, 0.09804, 0.07843, 1.0);

/// Which tips the screen lists
pub enum TipsFilter {
    /// The title: every tip of this episode (0-based)
    Episode(u8),
    /// The system menu: the tips this playthrough has collected, in the table's order
    Collected(Vec<i32>),
}

struct Item {
    entry: usize,
    title_index: u16,
    highlight: Tweener,
    flash: Tweener,
}

/// What a touch went down on (`+0x104`..`+0x10c`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touched {
    Close,
    Item(usize),
    Text,
}

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

pub struct Tips {
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    items: Vec<Item>,
    cursor: usize,
    /// The open fraction (`+0xc4`)
    open: f32,
    scroll: Tweener,
    close_flash: Tweener,
    title: TextBlock,
    text: TextBlock,
    /// The title's height + 5 (`+0x300`)
    title_height: f32,
    /// The title and text's height (`+0x304`)
    total_height: f32,
    state: State,
    target_size: (u32, u32),
    gestures: GestureRecognizer,
    /// The text's coasting after a flick (`+0x110`)
    coast: Coast,
    touched: Option<Touched>,
}

impl Tips {
    pub fn new(cx: &TaskCx, assets: &MenuAssets, filter: TipsFilter) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/tips.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /tips.txa: {e:?}");
                return None;
            }
        };
        let items = assets
            .scenario
            .info_tables()
            .tips_info
            .iter()
            .enumerate()
            .filter(|(i, entry)| match &filter {
                TipsFilter::Episode(episode) => entry.episode == *episode,
                TipsFilter::Collected(tips) => tips.contains(&(*i as i32)),
            })
            .enumerate()
            .map(|(k, (entry, info))| Item {
                entry,
                title_index: info.title_index,
                highlight: Tweener::new((k == 0) as i32 as f32),
                flash: Tweener::new(0.0),
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            warn!("No tips to show");
            return None;
        }
        let empty = || {
            TextBlock::new(
                cx.pre_render,
                &assets.system_font,
                "",
                layout(48.0, false),
                OUTLINE,
            )
        };
        let mut tips = Self {
            assets: assets.clone(),
            textures,
            items,
            cursor: 0,
            open: 0.0,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            title: empty(),
            text: empty(),
            title_height: 0.0,
            total_height: 0.0,
            state: State::Opening,
            target_size: (1920, 1080),
            gestures: GestureRecognizer::new(false),
            coast: Coast::default(),
            touched: None,
        };
        tips.build_texts(cx.pre_render);
        Some(tips)
    }

    /// `0x7100107960`: the cursor tip's title (48 px) and text (32 px), 1222 px wide; the text scrolls back up
    fn build_texts(&mut self, ctx: &PreRenderContext) {
        let info = &self.assets.scenario.info_tables().tips_info[self.items[self.cursor].entry];
        let font = &self.assets.system_font;
        self.title = TextBlock::new(ctx, font, info.title.as_str(), layout(48.0, false), OUTLINE);
        self.text = TextBlock::new(
            ctx,
            font,
            info.content.as_str(),
            layout(32.0, true),
            OUTLINE,
        );
        let title = self.title.size().y;
        self.title_height = title + 5.0;
        self.total_height = title + 8.0 + self.text.size().y + 5.0;
        self.scroll = Tweener::new(0.0);
    }

    fn max_scroll(&self) -> f32 {
        (self.total_height - VIEW).max(0.0)
    }

    /// `0x7100109630`
    fn handle_input(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        let n = self.items.len();
        let to = if input.up {
            self.cursor.saturating_sub(1)
        } else if input.down {
            (self.cursor + 1).min(n - 1)
        } else {
            self.cursor
        };
        if input.up || input.down {
            if to != self.cursor {
                cx.sound("move");
                self.set_cursor(cx.pre_render, to);
            }
        } else if input.cancel {
            // `0x7100109390`
            cx.sound("cancel");
            self.close_flash = Tweener::new(0.0);
            minigame::flash(&mut self.close_flash);
            self.state = State::Closing;
        } else if input.scroll != 0.0 {
            let scroll = (self.scroll.value() + input.scroll * 32.0).clamp(0.0, self.max_scroll());
            self.scroll = Tweener::new(scroll);
        }
    }

    /// The highlight moves to `to` and its texts are built
    fn set_cursor(&mut self, ctx: &PreRenderContext, to: usize) {
        if to == self.cursor {
            return;
        }
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.1);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.25);
        self.build_texts(ctx);
    }

    /// The down listener's hit (`0x7100109b80`, list `0x7100109800`): the close button, then in the band
    /// 150 <= y < 992 a list row (the cursor's own counts as empty) or the text
    fn hit(&self, at: Vec2) -> Option<Touched> {
        if minigame::contains([1764.0, 32.0, 1848.0, 116.0], at) {
            return Some(Touched::Close);
        }
        if !(150.0..992.0).contains(&at.y) {
            return None;
        }
        if minigame::contains([86.0, 188.0, 552.0, 918.0], at) {
            // the rows' rects, 88 px apart from y 193
            let row = (at.y - 193.0) / 88.0;
            return (row >= 0.0)
                .then_some(row as usize)
                .filter(|&k| k < self.items.len() && k != self.cursor)
                .map(Touched::Item);
        }
        minigame::contains([TEXT_X, TEXT_Y, 1834.0, 918.0], at).then_some(Touched::Text)
    }

    /// The tips' gesture listener (press `0x7100109df0`, tap `0x7100109f20`, drag `0x710010a0a0`, flick
    /// `0x710010a1c0`): a press brightens a row and makes it the cursor, a tap decides it, and the text is dragged
    /// and flicked
    fn handle_pointer(&mut self, cx: &mut TaskCx) {
        for gesture in self.gestures.update(cx.input.pointer, cx.frames) {
            match gesture {
                Gesture::Down(at) => {
                    self.touched = self.hit(at);
                    // grabbing the text stops its coasting
                    if self.touched == Some(Touched::Text) {
                        self.coast.stop();
                    }
                }
                Gesture::Press => match self.touched {
                    Some(Touched::Item(k)) => {
                        minigame::raise(&mut self.items[k].flash);
                        self.set_cursor(cx.pre_render, k);
                    }
                    Some(Touched::Close) => minigame::raise(&mut self.close_flash),
                    _ => {}
                },
                Gesture::Tap(_) => match self.touched {
                    Some(Touched::Item(k)) => {
                        cx.sound("decide");
                        minigame::flash(&mut self.items[k].flash);
                        self.set_cursor(cx.pre_render, k);
                    }
                    Some(Touched::Close) => {
                        // `0x7100109390` without the cancel sound
                        cx.sound("decide");
                        minigame::flash(&mut self.close_flash);
                        self.state = State::Closing;
                    }
                    _ => {}
                },
                Gesture::Drag(from, to) => match self.touched {
                    Some(Touched::Text) => {
                        let scroll =
                            (self.scroll.value() - (to.y - from.y)).clamp(0.0, self.max_scroll());
                        self.scroll = Tweener::new(scroll);
                    }
                    Some(Touched::Item(k)) => minigame::lower(&mut self.items[k].flash),
                    Some(Touched::Close) => {
                        minigame::lower(&mut self.close_flash);
                        self.touched = None;
                    }
                    None => {}
                },
                Gesture::Flick(velocity) if self.touched == Some(Touched::Text) => {
                    self.coast.start(velocity.y)
                }
                _ => {}
            }
        }
    }
}

/// The tips' text layout: system.fnt, 1216 px between the outlines; the content breaks softly and keeps room for
/// rubi, the title does neither
fn layout(size: f32, content: bool) -> LayoutParams {
    LayoutParams {
        always_leave_space_for_rubi: content,
        perform_soft_breaks: content,
        ..text_layout(1222.0, size)
    }
}

/// The menus' window (`0x7100108d70`, `0x71000523b0`): three columns at `xs` with texture columns `us`, and three
/// rows from y 112; the centre is opaque
pub(super) fn draw_window(
    pass: &mut RenderPass,
    window: &shin_render::gpu_texture::GpuTexture,
    transform: Mat4,
    xs: [f32; 4],
    us: [f32; 4],
) {
    let rows = [0.0, 80.0, 810.0, 898.0];
    let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
    for row in 0..3 {
        for column in 0..3 {
            let builder = if (row, column) == (1, 1) {
                opaque
            } else {
                ui::builder()
            };
            Sprite::new(
                window,
                [
                    xs[column],
                    112.0 + rows[row],
                    xs[column + 1],
                    112.0 + rows[row + 1],
                ],
            )
            .uv([
                us[column],
                rows[row] / 898.0,
                us[column + 1],
                rows[row + 1] / 898.0,
            ])
            .draw(pass, builder, transform);
        }
    }
}

impl Task for Tips {
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
                if cx.focused {
                    self.handle_pointer(cx);
                    // touches and the text coasting hold the buttons off (`0x710006b860`)
                    if self.state == State::Active
                        && !self.gestures.is_active()
                        && !self.coast.is_moving()
                    {
                        self.handle_input(cx);
                    }
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
        if self.coast.is_moving() {
            let scroll = self
                .coast
                .update(self.scroll.value(), 0.0, self.max_scroll(), cx.frames);
            self.scroll = Tweener::new(scroll);
        }
        ui::advance(&mut self.scroll, cx.frames);
        ui::advance(&mut self.close_flash, cx.frames);
        for item in &mut self.items {
            ui::advance(&mut item.highlight, cx.frames);
            ui::advance(&mut item.flash, cx.frames);
        }
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

        // the window slides in from the right
        let slide = 1920.0 - 1920.0 * a;
        let window = projection * Mat4::from_translation(vec3(slide, 0.0, 0.0));
        draw_window(
            pass,
            t("window"),
            window,
            [0.0, 80.0, 1840.0, 1920.0],
            [0.0, 0.041667, 0.958333, 1.0],
        );
        Sprite::new(t("separator"), [568.0, 203.0, 586.0, 903.0]).draw(pass, builder, window);

        let list = window * Mat4::from_translation(vec3(86.0, 188.0, 0.0));
        for (k, item) in self.items.iter().enumerate() {
            let y = 5.0 + 88.0 * k as f32;
            let (glow, flash) = (item.highlight.value() * 0.5, item.flash.value());
            Sprite::new(t("menu"), [4.0, y, 462.0, y + 88.0])
                .glow(glow)
                .flash(flash)
                .draw(pass, builder, list);
            let v = item.title_index as f32 / 26.0;
            Sprite::new(t("items"), [4.0, y + 12.0, 462.0, y + 76.0])
                .uv([0.0, v, 1.0, v + 1.0 / 26.0])
                .glow(glow)
                .flash(flash)
                .draw(pass, builder, list);
        }

        let scroll = self.scroll.value();
        let [x0, y0, x1, y1] = CLIP;
        ui::clip(
            pass,
            Some([x0 + slide, y0, x1 + slide, y1]),
            self.target_size,
        );
        let rule = TEXT_Y + self.title_height - scroll;
        Sprite::new(t("bar"), [TEXT_X, rule - 20.0, 1834.0, rule - 4.0]).draw(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            window,
        );
        let text = window * Mat4::from_translation(vec3(TEXT_X, TEXT_Y - scroll, 0.0));
        ui::draw_text(
            pass,
            &self.title,
            text * Mat4::from_translation(vec3(5.0 - OUTLINE, 5.0 - OUTLINE, 0.0)),
            FloatColor4::WHITE,
            BORDER,
        );
        ui::draw_text(
            pass,
            &self.text,
            text * Mat4::from_translation(vec3(
                5.0 - OUTLINE,
                self.title_height + 3.0 - OUTLINE,
                0.0,
            )),
            FloatColor4::WHITE,
            BORDER,
        );
        ui::clip(pass, None, self.target_size);

        // the window's edges over the clipped text, and the scroll bar
        let edge = t("window");
        Sprite::new(edge, [86.0, 183.0, 1834.0, 188.0])
            .uv([0.0447917, 0.0790646, 0.955208, 0.0846325])
            .draw(pass, builder, window);
        Sprite::new(edge, [86.0, 918.0, 1834.0, 923.0])
            .uv([0.0447917, 0.89755, 0.955208, 0.903118])
            .draw(pass, builder, window);
        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            window,
            (1838.0, TEXT_Y),
            VIEW,
            scroll,
            self.total_height,
        );

        let top = 160.0 * a;
        Sprite::new(t("caption"), [480.0, top - 160.0, 1440.0, top - 32.0])
            .draw(pass, builder, projection);
        Sprite::new(t("close"), [1764.0, top - 128.0, 1848.0, top - 44.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left (`0x7100109040`, `0x710018741c`)
        let help = KeyHelp {
            labels: t("help_tips"),
            keys: t("key"),
            label_height: 192.0,
            y: 1080.0 - 88.0 * a + 20.0,
            glyph_offset: 0.0,
            glyph_step: 48.0,
            gap: 10.0,
        };
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let scrolls = if self.total_height - VIEW > 0.0 {
            FloatColor4::WHITE
        } else {
            grey
        };
        let mut x = help.draw(pass, 1872.0, &[4], 48.0, 72.0, FloatColor4::WHITE);
        x = help.draw(pass, x, &[9], 144.0, 174.0, scrolls);
        help.draw(pass, x, &[1], 0.0, 206.0, FloatColor4::WHITE);
    }
}
