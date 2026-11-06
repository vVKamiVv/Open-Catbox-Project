//! `ADV::SystemMenu` (ctor `0x710001c100`, update `0x710001d000`, render `0x710001d6e0`): the in-game menu Y opens
//! over the blurred scene. Its items slide in from the left one after another; on the right two panels show the
//! scene's title and the play time. An item flashes, then opens its screen while the menu slides away, and the menu
//! slides back when that screen closes.
//!
//! shortcut: Characters, Tips and Manual do not exist in the port yet; choosing them plays the error sound.

use glam::{Mat4, vec3};
use shin_core::{
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::render_pass::RenderPass;
use tracing::warn;

use super::{
    AdvRequest, MenuAssets, Task, TaskCx,
    chars::Chars,
    config_menu::ConfigMenu,
    dialog::Dialog,
    manual::Manual,
    save_menu::{Mode, SaveMenu},
    tips::{Tips, TipsFilter},
};
use crate::{
    adv::hud::{OUTLINE, draw_text, text_layout},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    render::PreRenderContext,
    ui,
};

/// The item ids, which are also their rows in `sysmenu`
const SAVE: u8 = 0;
const LOAD: u8 = 1;
const CONFIG: u8 = 2;
const CHARACTERS: u8 = 3;
const TIPS: u8 = 4;
const MANUAL: u8 = 5;
const TITLE: u8 = 6;

/// "Go back to the title screen?" (`0x7100162ebe`, English IPS)
const TITLE_QUESTION: &str = "Go back to the title screen?";

/// Item layout: 396 x 120 at x 24 when shown, 504 further left when hidden, from y 72 every 128 px
const ITEM_X: f32 = 24.0;
const ITEM_SLIDE: f32 = 504.0;
const ITEM_TOP: f32 = 72.0;
const ITEM_STEP: f32 = 128.0;
const ITEM_W: f32 = 396.0;
const ITEM_H: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    Closing,
    Done,
}

struct Item {
    id: u8,
    slide: Tweener,
    flash: Tweener,
}

/// What the menu has opened over itself
enum Child {
    Title(Dialog),
    /// A screen that reports back once it has closed (the original's callback with 1)
    Screen(Box<dyn Task>),
}

pub struct SystemMenu {
    assets: MenuAssets,
    items: Vec<Item>,
    cursor: usize,
    state: State,
    /// The info panels: slide (1 = out to the right) and alpha
    panel_slide: Tweener,
    panel_alpha: Tweener,
    scene_title: Option<TextBlock>,
    play_time: TextBlock,
    play_time_string: String,
    /// The tips this playthrough has collected (`Context+0x9120`)
    tips: Vec<i32>,
    /// CHARS' grid and page (`Context+0x9118`)
    chars: (i32, i32),
    /// The item whose flash is playing before it opens
    deciding: Option<u8>,
    child: Option<Child>,
    /// The pointer as last frame, and the item a press went down on
    pointer_held: bool,
    pressed: Option<usize>,
}

/// The panels' text objects: 756 wide, size 38, no room for rubi (`0x710001edc0`)
fn panel_layout() -> LayoutParams {
    LayoutParams {
        always_leave_space_for_rubi: false,
        ..text_layout(756.0, 38.0)
    }
}

fn play_time_string(play_seconds: u32) -> String {
    // the original counts minutes from its frame counter and caps them below 60000 (`0x710001d000`)
    let minutes = play_seconds / 60;
    let (hours, minutes) = if minutes < 60000 {
        (minutes / 60, minutes % 60)
    } else {
        (999, 59)
    };
    if hours == 0 {
        format!("{minutes}m")
    } else {
        format!("{hours}h {minutes}m")
    }
}

impl SystemMenu {
    /// `scene_title`: the committed level-1 SAVEINFO text; Characters shows when CHARS has set a grid (`chars.0 >= 0`), Tips when `tips` (the
    /// collected tips) has any
    pub fn new(
        ctx: &PreRenderContext,
        assets: &MenuAssets,
        scene_title: &str,
        chars: (i32, i32),
        tips: Vec<i32>,
        play_seconds: u32,
    ) -> Self {
        let ids = [SAVE, LOAD, CONFIG]
            .into_iter()
            .chain((chars.0 >= 0).then_some(CHARACTERS))
            .chain((!tips.is_empty()).then_some(TIPS))
            .chain([MANUAL, TITLE]);
        let items = ids
            .map(|id| Item {
                id,
                slide: Tweener::new(0.0),
                flash: Tweener::new(0.0),
            })
            .collect();
        let layout = panel_layout();
        let scene_title = (!scene_title.is_empty())
            .then(|| TextBlock::new(ctx, &assets.system_font, scene_title, layout, OUTLINE));
        let play_time_string = play_time_string(play_seconds);
        let play_time =
            TextBlock::new(ctx, &assets.system_font, &play_time_string, layout, OUTLINE);
        let mut menu = Self {
            assets: assets.clone(),
            items,
            cursor: 0,
            state: State::Active,
            panel_slide: Tweener::new(1.0),
            panel_alpha: Tweener::new(0.0),
            scene_title,
            play_time,
            play_time_string,
            tips,
            chars,
            deciding: None,
            child: None,
            pointer_held: false,
            pressed: None,
        };
        menu.start();
        menu
    }

    /// `0x710001d460`: the items slide in 2 frames apart over 10 frames each, the panels with them
    fn start(&mut self) {
        for (i, item) in self.items.iter_mut().enumerate() {
            ui::hold(&mut item.slide, 2.0 * i as f32);
            ui::tween(&mut item.slide, 1.0, 10.0, Easing::SineOut);
        }
        ui::tween(&mut self.panel_slide, 0.0, 10.0, Easing::SineOut);
        ui::tween(&mut self.panel_alpha, 1.0, 10.0, Easing::Linear);
    }

    /// `0x710001e010`: everything slides back out the same way
    fn close(&mut self) {
        for (i, item) in self.items.iter_mut().enumerate() {
            ui::hold(&mut item.slide, 2.0 * i as f32);
            ui::tween(&mut item.slide, 0.0, 10.0, Easing::SineIn);
        }
        ui::tween(&mut self.panel_slide, 1.0, 10.0, Easing::SineIn);
        ui::tween(&mut self.panel_alpha, 0.0, 10.0, Easing::Linear);
    }

    fn animating(&self) -> bool {
        self.items
            .iter()
            .any(|item| !item.slide.is_idle() || !item.flash.is_idle())
            || !self.panel_slide.is_idle()
            || !self.panel_alpha.is_idle()
    }

    /// `0x710001d560`: "decide", the item flashes, and opens once the flash is over
    fn decide(&mut self, cx: &TaskCx) {
        cx.sound("decide");
        let item = &mut self.items[self.cursor];
        item.flash = Tweener::new(0.0);
        minigame::flash(&mut item.flash);
        self.deciding = Some(item.id);
    }

    /// `0x710001e100`: opens the item's screen over the menu, which slides away meanwhile
    fn open(&mut self, cx: &TaskCx, id: u8) {
        match id {
            TITLE => {
                self.child = Some(Child::Title(Dialog::yes_no(
                    cx.pre_render,
                    &self.assets,
                    TITLE_QUESTION,
                    1,
                )));
            }
            CONFIG => match ConfigMenu::new(cx, &self.assets) {
                Some(config) => self.child = Some(Child::Screen(Box::new(config))),
                None => {
                    cx.sound("error");
                    return;
                }
            },
            SAVE | LOAD => {
                let mode = if id == SAVE { Mode::Save } else { Mode::Load };
                match SaveMenu::new(cx, &self.assets, mode) {
                    Some(menu) => self.child = Some(Child::Screen(Box::new(menu))),
                    None => {
                        cx.sound("error");
                        return;
                    }
                }
            }
            MANUAL => match Manual::new(cx) {
                Some(manual) => self.child = Some(Child::Screen(Box::new(manual))),
                None => {
                    cx.sound("error");
                    return;
                }
            },
            TIPS => match Tips::new(cx, &self.assets, TipsFilter::Collected(self.tips.clone())) {
                Some(tips) => self.child = Some(Child::Screen(Box::new(tips))),
                None => {
                    cx.sound("error");
                    return;
                }
            },
            CHARACTERS => match Chars::new(cx, &self.assets, self.chars.0, self.chars.1) {
                Some(chars) => self.child = Some(Child::Screen(Box::new(chars))),
                None => {
                    cx.sound("error");
                    return;
                }
            },
            _ => unreachable!(),
        }
        self.close();
    }

    fn move_cursor(&mut self, cx: &TaskCx, to: usize) {
        if to != self.cursor {
            self.cursor = to;
            cx.sound("move");
        }
    }

    fn item_at(&self, point: glam::Vec2) -> Option<usize> {
        (0..self.items.len()).find(|&i| {
            let top = ITEM_TOP + ITEM_STEP * i as f32;
            minigame::contains([ITEM_X, top, ITEM_X + ITEM_W, top + ITEM_H], point)
        })
    }

    fn handle_input(&mut self, cx: &mut TaskCx) {
        // the pointer (vfuncs 7 and 8): a press flashes an item, a release on it decides, elsewhere closes
        let pointer = cx.input.pointer;
        let went_down = pointer.held && !self.pointer_held;
        let went_up = !pointer.held && self.pointer_held;
        self.pointer_held = pointer.held;
        let hit = pointer.position.and_then(|p| self.item_at(p));
        if went_down {
            if let Some(i) = hit {
                self.pressed = Some(i);
                minigame::raise(&mut self.items[i].flash);
                self.cursor = i;
            }
            return;
        }
        if went_up {
            match self.pressed.take() {
                Some(i) if hit == Some(i) => {
                    // the press's flash goes on into the decide flash
                    self.decide(cx);
                }
                Some(i) => minigame::lower(&mut self.items[i].flash),
                None => {
                    cx.sound("cancel");
                    self.close();
                    self.state = State::Closing;
                }
            }
            return;
        }
        if self.pressed.is_some() || self.animating() {
            return;
        }

        let input = cx.input;
        let n = self.items.len();
        if input.cancel || input.y {
            cx.sound("cancel");
            self.close();
            self.state = State::Closing;
        } else if input.confirm {
            self.decide(cx);
        } else if input.up {
            self.move_cursor(
                cx,
                if self.cursor < 1 {
                    n - 1
                } else {
                    self.cursor - 1
                },
            );
        } else if input.down {
            self.move_cursor(
                cx,
                if self.cursor + 1 < n {
                    self.cursor + 1
                } else {
                    0
                },
            );
        } else if input.left {
            self.move_cursor(cx, 0);
        } else if input.right {
            self.move_cursor(cx, n - 1);
        }
    }

    fn update_child(&mut self, cx: &mut TaskCx) {
        let Some(child) = &mut self.child else {
            return;
        };
        match child {
            Child::Title(dialog) => {
                let running = dialog.update(&mut cx.reborrow(cx.focused));
                for event in dialog.take_events() {
                    match event {
                        // Yes, once the dialog is gone: save and return to the title
                        (1, 0) => cx.requests.push(AdvRequest::ExitToTitle),
                        // No or cancel, as the dialog starts closing: the menu comes back
                        (0, button) if button != 0 => self.start(),
                        _ => {}
                    }
                }
                if !running {
                    self.child = None;
                }
            }
            Child::Screen(screen) => {
                if !screen.update(&mut cx.reborrow(cx.focused)) {
                    self.child = None;
                    self.start();
                }
            }
        }
    }
}

impl Task for SystemMenu {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        if self.child.is_some() {
            self.update_child(cx);
        } else {
            match self.state {
                State::Active => {
                    if let Some(id) = self.deciding {
                        if self.items[self.cursor].flash.is_idle() {
                            self.deciding = None;
                            self.open(cx, id);
                        }
                    } else if cx.focused {
                        self.handle_input(cx);
                    }
                }
                State::Closing => {
                    if !self.animating() {
                        cx.requests.push(AdvRequest::SystemMenuClosed);
                        self.state = State::Done;
                    }
                }
                State::Done => return false,
            }
        }

        for item in &mut self.items {
            ui::advance(&mut item.slide, cx.frames);
            ui::advance(&mut item.flash, cx.frames);
        }
        ui::advance(&mut self.panel_slide, cx.frames);
        ui::advance(&mut self.panel_alpha, cx.frames);

        let play_time = play_time_string(cx.global.play_seconds);
        if play_time != self.play_time_string {
            self.play_time = TextBlock::new(
                cx.pre_render,
                &self.assets.system_font,
                &play_time,
                panel_layout(),
                OUTLINE,
            );
            self.play_time_string = play_time;
        }
        true
    }

    fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();

        let sysmenu = self.assets.adv_textures.get("sysmenu");
        for (i, item) in self.items.iter().enumerate() {
            let t = item.slide.value();
            let x = (t - 1.0) * ITEM_SLIDE + ITEM_X;
            let y = ITEM_TOP + ITEM_STEP * i as f32;
            // the left column is the highlighted art, the right one the plain
            let u = if i == self.cursor { 0.0 } else { 0.5 };
            let v = item.id as f32 / 7.0;
            Sprite::new(sysmenu, [x, y, x + ITEM_W, y + ITEM_H])
                .uv([u, v, u + 0.5, v + 1.0 / 7.0])
                .alpha(t)
                .flash(item.flash.value())
                .draw(pass, builder, projection);
        }

        let alpha = self.panel_alpha.value();
        let info = self.assets.adv_textures.get("info");
        let panels = [
            (795.0, 0.0, self.scene_title.as_ref()),
            (930.0, 0.5, Some(&self.play_time)),
        ];
        for (top, v, text) in panels {
            let Some(text) = text else { continue };
            let panel = projection
                * Mat4::from_translation(vec3(-48.0 + 120.0 * self.panel_slide.value(), top, 0.0));
            Sprite::new(info, [1176.0, 0.0, 1932.0, 64.0])
                .uv([0.0, v, 1.0, v + 0.5])
                .alpha(alpha)
                .draw(pass, builder, panel);
            let transform = panel * Mat4::from_translation(vec3(1920.0 - text.size().x, 54.0, 0.0));
            draw_text(pass, text, transform, FloatColor4::WHITE, alpha);
        }

        match &self.child {
            Some(Child::Title(dialog)) => dialog.render(pass),
            Some(Child::Screen(screen)) => screen.render(pass),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::play_time_string;

    #[test]
    fn play_time_reads_like_the_original() {
        assert_eq!(play_time_string(59), "0m");
        assert_eq!(play_time_string(61 * 60), "1h 1m");
        assert_eq!(play_time_string(60000 * 60), "999h 59m");
    }
}
