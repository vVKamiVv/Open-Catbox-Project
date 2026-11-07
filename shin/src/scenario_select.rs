//! The scenario select (`shin::ScenarioSelect`, ctor `0x71000dc8f0`, update `0x71000ddb70`, render `0x71000de100`,
//! keys `0x71000df440`): Start's list of story pieces in tabs, each with its details picture, from `/snrsel.txa`;
//! and the bridge into a new game (`ScenarioSelectBridge`, `0x71000e3a20`): the screen splits into two doors that
//! swing open onto white light, which glows through the opening (black doors on white, blurred at four sizes and
//! added over the screen), then the screen goes from white to black.
//!
//! shortcut: the glow's blur reads black past the edges of its shrunk picture where the original's small textures
//! repeat their edge.

use std::{f32::consts::FRAC_PI_2, sync::Arc};

use enum_map::EnumMap;
use glam::{Mat4, Vec2, vec3};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::SaveData,
        scenario::{info::BgmId, instruction_elements::FromNumber},
    },
    primitives::color::{FloatColor4, UnormColor},
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::Volume,
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::CanvasSize,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};
use tracing::{info, warn};

use crate::{
    adv::menu::{AdvRequest, MenuAssets, Task, TaskCx, dialog::Dialog},
    app::AppAction,
    asset::{executable::EngineText, system::AssetServer, texture_archive::NamedTextures},
    audio::{BgmPlayer, play_sys_se},
    layer::{
        apply_region_blur,
        user::minigame::{self, Sprite},
    },
    render::PreRenderContext,
    ui::{
        self, MenuInput,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
    update::UpdateContext,
};

const START_QUESTION: EngineText = EngineText(0x710016468c, "Start the game?");
const UNLOCK_QUESTION: EngineText = EngineText(
    0x710016119c,
    "All eight main Episodes@rwill be marked as read.@rThe story cannot be experienced@rout of intended order.@rYou should not use this option to skip content@ryou have never read before.@r@rDo you accept?",
);

/// The tabs (`0x7100174768`): the first scenario, how many, the first `menu` cell and the details picture (`None`:
/// one per scenario, numbered like it)
const TABS: [(u8, u8, u8, Option<u8>); 6] = [
    (0, 8, 0, None),
    (8, 8, 0, Some(8)),
    (16, 8, 0, Some(16)),
    (24, 20, 8, None),
    (44, 2, 28, None),
    (46, 2, 30, None),
];

const ROW_H: f32 = 110.0;
const ROW_STEP: f32 = 112.0;
const VIEW: f32 = 670.0;
const LIST_TOP: f32 = 246.0;
const CLIP: [f32; 4] = [0.0, 241.0, 1920.0, 921.0];
/// The details picture shown for a locked piece
const LOCKED_DETAILS: u8 = 48;

struct Entry {
    scenario: u8,
    cell: u8,
    details: u8,
    /// 0 locked, 1 new, 2 available, 3 or more read
    state: i32,
    y: f32,
    selected: Tweener,
    flash: Tweener,
}

struct Tab {
    id: usize,
    new: bool,
    x: f32,
    selected: Tweener,
    flash: Tweener,
    entries: Vec<Entry>,
    cursor: usize,
    scroll: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    /// The entry flashes before it starts or asks
    Deciding,
    /// The close button's flash plays after a tap
    Closing,
    Done,
}

enum Child {
    Start(Dialog),
    Unlock(Dialog),
}

/// Where the scenario select leaves for
pub enum SelectExit {
    /// Back to the title, on Start
    Title,
    /// The bridge into a new game of this scenario
    Start(u8),
    /// The pieces were unlocked; the screen is built again
    Rebuild,
}

pub struct ScenarioSelect {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    pub bgm: BgmPlayer,
    pub global: SaveData,
    tabs: Vec<Tab>,
    tab: usize,
    scroll: Tweener,
    close_flash: Tweener,
    /// The details picture before the last change, fading out over 10 frames
    previous_details: Option<u8>,
    details_fade: Tweener,
    can_unlock: bool,
    /// Frames until the music starts: the screen comes in over 60 frames while the title's music fades
    music_delay: Option<f32>,
    state: State,
    child: Option<Child>,
    requests: Vec<AdvRequest>,
    exit: Option<SelectExit>,
    target_size: (u32, u32),
    /// The touch input (`GestureRecognizer`, down `0x71000e0210`), what the touch went down on (`+0x190..+0x198`)
    /// and the list's coasting after a flick (`+0x19c`)
    gestures: GestureRecognizer,
    touched: Option<Target>,
    coast: Coast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Outside the band y 160..980, or the close button once dragged
    Nothing,
    Close,
    Tab(usize),
    /// The list: the row under the down point, `None` once the touch scrolls the list
    List(Option<usize>),
}

impl ScenarioSelect {
    pub fn new(
        asset_server: &Arc<AssetServer>,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
        bgm: BgmPlayer,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/snrsel.txa")?;
        let mut tabs = Vec::new();
        for (id, &(first, count, first_cell, details)) in TABS.iter().enumerate() {
            let entries = (0..count)
                .map(|i| {
                    let scenario = first + i;
                    let slot = scenario as usize + 1;
                    let mut state = global.persist_data.get(slot);
                    // Episode 1 can always be started
                    if slot == 1 && state <= 1 {
                        state = 2;
                    }
                    Entry {
                        scenario,
                        cell: first_cell + i,
                        details: details.unwrap_or(scenario),
                        state,
                        y: ROW_STEP * i as f32,
                        selected: Tweener::new((i == 0) as i32 as f32),
                        flash: Tweener::new(0.0),
                    }
                })
                .collect::<Vec<_>>();
            // a tab shows only once one of its pieces is open
            if !entries.iter().any(|entry| entry.state >= 1) {
                continue;
            }
            let height = (ROW_STEP * entries.len() as f32 - 2.0).max(VIEW);
            tabs.push(Tab {
                id,
                new: entries.iter().any(|entry| entry.state == 1),
                x: 0.0,
                selected: Tweener::new(tabs.is_empty() as i32 as f32),
                flash: Tweener::new(0.0),
                entries,
                cursor: 0,
                scroll: 0.0,
                height,
            });
        }
        let left = (1920.0 - 300.0 * tabs.len() as f32) / 2.0;
        for (k, tab) in tabs.iter_mut().enumerate() {
            tab.x = left + 300.0 * k as f32;
        }
        let can_unlock = global.persist_data.get(25) == 0;
        info!("Scenario select: {} tabs", tabs.len());
        Ok(Self {
            assets,
            audio: audio.clone(),
            textures,
            bgm,
            global,
            tabs,
            tab: 0,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            previous_details: None,
            details_fade: Tweener::new(0.0),
            can_unlock,
            music_delay: Some(60.0),
            state: State::Active,
            child: None,
            requests: Vec::new(),
            exit: None,
            target_size: (1920, 1080),
            gestures: GestureRecognizer::new(false),
            touched: None,
            coast: Coast::default(),
        })
    }

    /// `First`'s second state: BGM 5 (`umib_055`) at full volume, once the screen is in
    fn start_music(&mut self, asset_server: &AssetServer) {
        let id = BgmId::from_number(5);
        let info = self.assets.scenario.info_tables().bgm_info(id);
        match asset_server.load_sync(info.path()) {
            Ok(track) => {
                self.bgm.play(
                    id,
                    track,
                    info.display_name.as_str(),
                    true,
                    Volume(1.0),
                    Tween::linear(Ticks::ZERO),
                );
            }
            Err(e) => warn!("Could not load the scenario select's BGM: {e:?}"),
        }
    }

    pub fn take_exit(&mut self) -> Option<SelectExit> {
        self.exit.take()
    }

    /// The music fades out over `frames` frames as the screen is left
    pub fn shutdown(&mut self, frames: f32) {
        self.bgm
            .stop(Tween::linear(Ticks::from_f32(frames.max(4.0))));
    }

    fn entry(&self) -> &Entry {
        let tab = &self.tabs[self.tab];
        &tab.entries[tab.cursor]
    }

    fn details(&self) -> u8 {
        let entry = self.entry();
        if entry.state >= 1 {
            entry.details
        } else {
            LOCKED_DETAILS
        }
    }

    /// The details crossfade: the old picture fades out over 10 frames
    fn fade_details(&mut self, old: u8) {
        self.previous_details = Some(old);
        self.details_fade = Tweener::new(1.0);
        ui::tween(&mut self.details_fade, 0.0, 10.0, Easing::Linear);
    }

    /// `0x71000dfc10`: the tab and its remembered cursor and scroll
    fn change_tab(&mut self, to: usize) {
        let old_details = self.details();
        let old = &mut self.tabs[self.tab];
        minigame::tween_now(&mut old.selected, 0.0, 0.25);
        old.scroll = self.scroll.target_value();
        self.coast.stop();
        self.tab = to;
        let new = &mut self.tabs[to];
        minigame::tween_now(&mut new.selected, 1.0, 0.25);
        self.scroll = Tweener::new(new.scroll);
        self.fade_details(old_details);
    }

    /// `0x71000dfea0`: the cursor and the scroll that keeps it in view
    fn set_cursor(&mut self, to: usize) {
        let old_details = self.details();
        let tab = &mut self.tabs[self.tab];
        minigame::tween_now(&mut tab.entries[tab.cursor].selected, 0.0, 0.1);
        tab.cursor = to;
        minigame::tween_now(&mut tab.entries[to].selected, 1.0, 0.25);
        let r = tab.entries[to].y;
        let scroll = self.scroll.target_value();
        let target = if r < scroll {
            Some(r.max(0.0))
        } else if r + ROW_H - scroll > VIEW {
            Some((tab.height - VIEW).min(r + ROW_H - VIEW))
        } else {
            None
        };
        if let Some(target) = target {
            ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
        }
        self.fade_details(old_details);
    }

    fn sound(&self, context: &UpdateContext, name: &str) {
        play_sys_se(&self.audio, context.asset_server, name, Volume::default());
    }

    /// `0x71000df920`
    fn decide(&mut self, context: &UpdateContext) {
        let state = self.entry().state;
        let tab = &mut self.tabs[self.tab];
        let flash = &mut tab.entries[tab.cursor].flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
        if (1..=2).contains(&state) {
            ui::hold(flash, 30.0);
        }
        if state == 0 {
            self.sound(context, "error");
            return;
        }
        self.sound(context, "decide");
        self.state = State::Deciding;
    }

    /// `0x71000dfb20`: "dooropen", the music fades over 120 frames and the bridge opens
    fn start(&mut self, context: &UpdateContext) {
        self.sound(context, "dooropen");
        self.exit = Some(SelectExit::Start(self.entry().scenario));
        self.state = State::Done;
    }

    fn close(&mut self) {
        self.exit = Some(SelectExit::Title);
        self.state = State::Done;
    }

    fn handle_keys(&mut self, context: &mut UpdateContext, input: &MenuInput, plus: bool) {
        let tabs = self.tabs.len();
        let n = self.tabs[self.tab].entries.len();
        let cursor = self.tabs[self.tab].cursor;
        if tabs > 1 && (input.l || input.r) {
            self.sound(context, "move");
            let to = if input.l {
                (self.tab + tabs - 1) % tabs
            } else {
                (self.tab + 1) % tabs
            };
            self.change_tab(to);
        } else if input.up && cursor > 0 {
            self.sound(context, "move");
            self.set_cursor(cursor - 1);
        } else if input.down && cursor + 1 < n {
            self.sound(context, "move");
            self.set_cursor(cursor + 1);
        } else if input.cancel {
            self.sound(context, "cancel");
            self.close();
        } else if input.confirm {
            self.decide(context);
        } else if plus && self.can_unlock {
            self.sound(context, "decide");
            let dialog = Dialog::yes_no(
                &*context.pre_render,
                &self.assets,
                &UNLOCK_QUESTION.get(),
                1,
            );
            self.child = Some(Child::Unlock(dialog));
        }
    }

    fn task_cx<'a, 'i, 'p, 'd, 'e>(
        &'a mut self,
        context: &'a mut UpdateContext<'i, '_, 'p, 'd, 'e>,
        focused: bool,
        input: MenuInput,
    ) -> TaskCx<'a, 'i, 'p, 'd, 'e> {
        TaskCx {
            frames: context.delta_ticks.as_f32(),
            focused,
            input,
            pre_render: &mut *context.pre_render,
            asset_server: context.asset_server,
            audio: &self.audio,
            assets: context.asset_server,
            global: &mut self.global,
            requests: &mut self.requests,
        }
    }

    /// The down hit (`0x71000e0210`): the close button, then the band y 160..980 with the other tabs (2+,
    /// `0x71000e00f0`) and the list x 86..526, y 246..916; `None` is not tracked
    fn target_at(&self, point: Vec2) -> Option<Target> {
        if minigame::contains([1764.0, 32.0, 1848.0, 116.0], point) {
            return Some(Target::Close);
        }
        if !(160.0..980.0).contains(&point.y) {
            return Some(Target::Nothing);
        }
        if self.tabs.len() > 1 {
            for (k, tab) in self.tabs.iter().enumerate() {
                if k != self.tab && minigame::contains([tab.x, 144.0, tab.x + 300.0, 211.5], point)
                {
                    return Some(Target::Tab(k));
                }
            }
        }
        if !minigame::contains([86.0, LIST_TOP, 526.0, 916.0], point) {
            return None;
        }
        let y = point.y + self.scroll.value() - LIST_TOP;
        Some(Target::List(
            self.tabs[self.tab]
                .entries
                .iter()
                .position(|entry| entry.y <= y && y < entry.y + ROW_H),
        ))
    }

    /// The scenario select's gesture listener (down `0x71000e0210`, press `0x71000e0470`, tap `0x71000e0560`, drag
    /// `0x71000e06d0`, flick `0x71000e08e0`, end `0x71000e0430`): a press brightens what is under it (and moves the
    /// cursor), a tap acts, and drags and flicks scroll the list
    fn handle_pointer(&mut self, context: &mut UpdateContext, input: &MenuInput, frames: f32) {
        for gesture in self.gestures.update(input.pointer, frames) {
            match gesture {
                Gesture::Down(at) => {
                    self.touched = self.target_at(at);
                    match self.touched {
                        None => self.gestures.cancel(),
                        // touching a coasting list stops it, and that touch selects nothing
                        Some(Target::List(_)) if self.coast.is_moving() => {
                            self.coast.stop();
                            self.touched = Some(Target::List(None));
                        }
                        _ => {}
                    }
                }
                Gesture::Press => match self.touched {
                    Some(Target::Close) => minigame::raise(&mut self.close_flash),
                    Some(Target::Tab(k)) => minigame::raise(&mut self.tabs[k].flash),
                    Some(Target::List(Some(row))) => {
                        minigame::raise(&mut self.tabs[self.tab].entries[row].flash);
                        if row != self.tabs[self.tab].cursor {
                            self.set_cursor(row);
                        }
                    }
                    _ => {}
                },
                Gesture::Tap(_) => match self.touched.take() {
                    Some(Target::Close) => {
                        self.sound(context, "decide");
                        minigame::flash(&mut self.close_flash);
                        self.state = State::Closing;
                    }
                    Some(Target::Tab(k)) => {
                        minigame::flash(&mut self.tabs[k].flash);
                        self.sound(context, "decide");
                        self.change_tab(k);
                    }
                    Some(Target::List(Some(row))) => {
                        if row != self.tabs[self.tab].cursor {
                            self.set_cursor(row);
                        }
                        self.decide(context);
                    }
                    _ => {}
                },
                Gesture::Drag(from, to) => match self.touched {
                    Some(Target::Nothing | Target::Close) => {
                        minigame::lower(&mut self.close_flash);
                        self.touched = Some(Target::Nothing);
                    }
                    Some(Target::Tab(k)) => minigame::lower(&mut self.tabs[k].flash),
                    Some(Target::List(row)) => {
                        if let Some(row) = row {
                            minigame::lower(&mut self.tabs[self.tab].entries[row].flash);
                            self.touched = Some(Target::List(None));
                        }
                        let max = self.tabs[self.tab].height - VIEW;
                        self.scroll_to((self.scroll.value() - (to.y - from.y)).clamp(0.0, max));
                    }
                    None => {}
                },
                Gesture::Flick(velocity) => {
                    if let Some(Target::List(_)) = self.touched {
                        self.coast.start(velocity.y);
                    }
                }
                Gesture::End => self.touched = None,
                Gesture::LongPress => {}
            }
        }
    }

    /// The list scrolled by a drag (`0x71000e06d0`) or coasting (`0x71000ddb70`): at once, the cursor moving to stay
    /// on a row fully inside the 670 px view (without scrolling to it)
    fn scroll_to(&mut self, scroll: f32) {
        self.scroll.fast_forward_to(scroll);
        let entries = &self.tabs[self.tab].entries;
        let first = entries
            .iter()
            .position(|entry| entry.y >= scroll)
            .unwrap_or(entries.len() - 1);
        let last = entries
            .iter()
            .rposition(|entry| entry.y + ROW_H <= scroll + VIEW)
            .unwrap_or(0);
        let cursor = self.tabs[self.tab].cursor;
        let row = cursor.clamp(first, last.max(first));
        if row != cursor {
            self.set_cursor(row);
        }
    }

    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        state: &EnumMap<AppAction, ActionState>,
        pointer: Pointer,
    ) {
        let frames = context.delta_ticks.as_f32();
        let settings = self.global.settings;
        self.bgm.set_gain(settings.v0_bgmvol, 1.0);
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        let size = context
            .pre_render
            .resize_source
            .handle::<CanvasSize>()
            .get();
        self.target_size = (size.width, size.height);

        if let Some(delay) = &mut self.music_delay {
            *delay -= frames;
            if *delay <= 0.0 {
                self.music_delay = None;
                self.start_music(context.asset_server);
            }
        }

        let input = MenuInput::new(state, pointer);
        if let Some(child) = self.child.take() {
            let (mut dialog, unlock) = match child {
                Child::Start(dialog) => (dialog, false),
                Child::Unlock(dialog) => (dialog, true),
            };
            let running = dialog.update(&mut self.task_cx(context, true, input));
            let events = dialog.take_events();
            self.child = running.then(|| {
                if unlock {
                    Child::Unlock(dialog)
                } else {
                    Child::Start(dialog)
                }
            });
            for event in events {
                match (unlock, event) {
                    // `0x71000e2f50`: Yes once the dialog is gone starts; anything else goes back to the list
                    (false, (1, 0)) => self.start(context),
                    (false, (1, _)) => self.state = State::Active,
                    // `0x71000e3050`: progress 24 and the first 25 pieces at least "new"; the screen is rebuilt
                    (true, (1, 0)) => {
                        let persist = &mut self.global.persist_data;
                        persist.set(0, 24);
                        for slot in 1..=25 {
                            if persist.get(slot) == 0 {
                                persist.set(slot, 1);
                            }
                        }
                        self.exit = Some(SelectExit::Rebuild);
                        self.state = State::Done;
                    }
                    _ => {}
                }
            }
        } else {
            match self.state {
                State::Active => {
                    self.handle_pointer(context, &input, frames);
                    if self.coast.is_moving() {
                        let max = self.tabs[self.tab].height - VIEW;
                        let scroll = self.coast.update(self.scroll.value(), 0.0, max, frames);
                        self.scroll_to(scroll);
                    }
                    // touches and the list moving hold the buttons off (`0x710006b860`)
                    if !self.gestures.is_active()
                        && !self.coast.is_moving()
                        && self.state == State::Active
                        && self.scroll.is_idle()
                    {
                        self.handle_keys(context, &input, state[AppAction::Plus].is_clicked);
                    }
                }
                State::Closing => {
                    if self.close_flash.is_idle() {
                        self.close();
                    }
                }
                State::Deciding => {
                    let tab = &self.tabs[self.tab];
                    if tab.entries[tab.cursor].flash.is_idle() {
                        if self.entry().state < 3 {
                            self.start(context);
                        } else {
                            let dialog = Dialog::yes_no(
                                &*context.pre_render,
                                &self.assets,
                                &START_QUESTION.get(),
                                1,
                            );
                            self.child = Some(Child::Start(dialog));
                        }
                    }
                }
                State::Done => {}
            }
        }

        ui::advance(&mut self.scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        ui::advance(&mut self.details_fade, frames);
        for tab in &mut self.tabs {
            ui::advance(&mut tab.selected, frames);
            ui::advance(&mut tab.flash, frames);
            for entry in &mut tab.entries {
                ui::advance(&mut entry.selected, frames);
                ui::advance(&mut entry.flash, frames);
            }
        }
    }

    fn draw_help(
        &self,
        pass: &mut RenderPass,
        right: f32,
        glyph: u8,
        row: f32,
        width: f32,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let y = 1012.0;
        let x = right - width;
        let v = row / 240.0;
        let mut label = Sprite::new(
            self.textures.get("help_snrsel"),
            [x, y, x + width, y + 48.0],
        )
        .uv([0.0, v, width / 320.0, v + 0.2]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        let x = x + 2.0 - 48.0;
        let keys = self.textures.get("key");
        let rect = [x, y, x + 48.0, y + 48.0];
        ui::prompts::draw(pass, ui::builder(), projection, keys, glyph, rect, color);
        x - 16.0
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0]).draw(pass, opaque, projection);

        // the key guide, right to left (`0x71000df0d0`)
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let mut x = self.draw_help(pass, 1872.0, 4, 48.0, 72.0, FloatColor4::WHITE);
        let start_color = if self.entry().state == 0 {
            grey
        } else {
            FloatColor4::WHITE
        };
        x = self.draw_help(pass, x, 3, 144.0, 74.0, start_color);
        if self.tabs.len() > 1 {
            x = self.draw_help(pass, x, 7, 96.0, 236.0, FloatColor4::WHITE);
        }
        x = self.draw_help(pass, x, 1, 0.0, 206.0, FloatColor4::WHITE);
        if self.can_unlock {
            self.draw_help(pass, x, 14, 192.0, 134.0, FloatColor4::WHITE);
        }

        Sprite::new(t("close"), [1764.0, 32.0, 1848.0, 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the details picture, the previous one fading out over it
        let details_rect = [576.0, 233.0, 1862.0, 929.0];
        Sprite::new(t(&format!("details_{:02}", self.details())), details_rect)
            .draw(pass, builder, projection);
        if let Some(previous) = self.previous_details {
            let fade = self.details_fade.value();
            if fade > 0.0 {
                Sprite::new(t("bg"), details_rect)
                    .uv([
                        576.0 / 1920.0,
                        233.0 / 1080.0,
                        1862.0 / 1920.0,
                        929.0 / 1080.0,
                    ])
                    .alpha(fade)
                    .draw(pass, builder, projection);
                Sprite::new(t(&format!("details_{previous:02}")), details_rect)
                    .alpha(fade)
                    .draw(pass, builder, projection);
            }
        }

        // the list, clipped
        let tab = &self.tabs[self.tab];
        ui::clip(pass, Some(CLIP), self.target_size);
        for entry in &tab.entries {
            let y = entry.y + LIST_TOP - self.scroll.value();
            if y > 921.0 || y + ROW_H < 241.0 {
                continue;
            }
            let (glow, flash) = (entry.selected.value() * 0.5, entry.flash.value());
            let (col, row) = ((entry.cell >> 3) as f32, (entry.cell & 7) as f32);
            Sprite::new(t("menu"), [86.0, y, 526.0, y + ROW_H])
                .uv([
                    col * 0.25,
                    row * 0.125,
                    col * 0.25 + 0.25,
                    row * 0.125 + 0.125,
                ])
                .glow(glow)
                .flash(flash)
                .draw(pass, builder, projection);
            let icon = match entry.state {
                3 => Some(("star", [76.0, y + 33.0, 120.0, y + 77.0])),
                1 => Some(("new", [62.0, y + 35.0, 158.0, y + 75.0])),
                0 => Some(("lock", [62.0, y + 10.0, 152.0, y + 100.0])),
                _ => None,
            };
            if let Some((name, rect)) = icon {
                Sprite::new(t(name), rect)
                    .glow(glow)
                    .flash(flash)
                    .draw(pass, builder, projection);
            }
        }
        ui::clip(pass, None, self.target_size);
        let clear = FloatColor4::WHITE.with_alpha(0.0);
        let bg = t("bg");
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [54.0, 241.0, 526.0, 246.0],
            [
                54.0 / 1920.0,
                241.0 / 1080.0,
                526.0 / 1920.0,
                246.0 / 1080.0,
            ],
            FloatColor4::WHITE,
            clear,
        );
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [54.0, 916.0, 526.0, 921.0],
            [
                54.0 / 1920.0,
                916.0 / 1080.0,
                526.0 / 1920.0,
                921.0 / 1080.0,
            ],
            clear,
            FloatColor4::WHITE,
        );

        // the tabs, the current one last (on top) and 15 % bigger
        if self.tabs.len() > 1 {
            let order = (0..self.tabs.len())
                .filter(|&k| k != self.tab)
                .chain([self.tab]);
            for k in order {
                let tab = &self.tabs[k];
                let s = 1.0 + 0.15 * tab.selected.value();
                let transform = projection
                    * Mat4::from_translation(vec3(tab.x + 150.0, 144.0 + 33.75, 0.0))
                    * Mat4::from_scale(vec3(s, s, 1.0))
                    * Mat4::from_translation(vec3(-150.0, -33.75, 0.0));
                let v = tab.id as f32 / 6.0;
                let flash = tab.flash.value();
                Sprite::new(t("tab_off"), [0.0, 0.0, 300.0, 67.5])
                    .uv([0.0, v, 1.0, v + 1.0 / 6.0])
                    .flash(flash)
                    .draw(pass, builder, transform);
                Sprite::new(t("tab_on"), [0.0, 0.0, 300.0, 67.5])
                    .uv([0.0, v, 1.0, v + 1.0 / 6.0])
                    .alpha(tab.selected.value())
                    .flash(flash)
                    .draw(pass, builder, transform);
                if tab.new {
                    Sprite::new(t("new"), [204.0, -8.0, 300.0, 32.0])
                        .flash(flash)
                        .draw(pass, builder, transform);
                }
            }
        }

        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            projection,
            (536.0, LIST_TOP),
            VIEW,
            self.scroll.value(),
            tab.height,
        );

        if let Some(Child::Start(dialog) | Child::Unlock(dialog)) = &self.child {
            dialog.render(pass);
        }
    }
}

/// `ScenarioSelectBridge`: the last screen split into two doors that swing open while the camera moves in and the
/// screen turns white; the new game starts at frame 360
pub struct Bridge {
    scenario: u8,
    snapshot: RenderTexture,
    angle: Tweener,
    camera: Tweener,
    white: Tweener,
    timer: Tweener,
    /// The glow (`+0x18..+0x30`, their scratch at `+0x38..`): the light through the opening (black doors on white)
    /// at 1/4, 1/8, 1/16 and 1/32 of the size, blurred, made when first rendered
    glow: Option<Vec<(RenderTexture, RenderTexture)>>,
}

/// The glow textures' shrink factors and their greys when added over the screen (`0x710015a7b0`, `6f4`, `8c8`, `94c`)
const GLOW: [(f32, f32); 4] = [
    (4.0, 1.0 / 15.0),
    (8.0, 2.0 / 15.0),
    (16.0, 4.0 / 15.0),
    (32.0, 8.0 / 15.0),
];

/// Clip space onto the top-left `1 / shrink` of the target
fn glow_region(shrink: f32) -> Mat4 {
    Mat4::from_translation(vec3(-1.0 + 1.0 / shrink, 1.0 - 1.0 / shrink, 0.0))
        * Mat4::from_scale(vec3(1.0 / shrink, 1.0 / shrink, 1.0))
}

impl Bridge {
    pub fn new(snapshot: RenderTexture, scenario: u8) -> Self {
        let mut angle = Tweener::new(0.0);
        ui::hold(&mut angle, 60.0);
        ui::tween(&mut angle, 1.0, 180.0, Easing::SineIn);
        let mut camera = Tweener::new(0.0);
        ui::hold(&mut camera, 90.0);
        ui::tween(&mut camera, 500.0, 150.0, Easing::SineIn);
        let mut white = Tweener::new(0.0);
        ui::hold(&mut white, 60.0);
        ui::tween(&mut white, 1.0, 180.0, Easing::SineIn);
        // the last quad's grey: 1 from the start, held 240 frames, then down to black over 120 (`0x71000e3a20`)
        let mut timer = Tweener::new(1.0);
        ui::hold(&mut timer, 240.0);
        timer.enqueue(1.0, Tween::linear(Ticks::ZERO));
        ui::tween(&mut timer, 0.0, 120.0, Easing::Linear);
        Self {
            scenario,
            snapshot,
            angle,
            camera,
            white,
            timer,
            glow: None,
        }
    }

    /// `0x71000e5400`: black doors on white into each glow texture, then the blur across and down
    pub fn pre_render(&mut self, ctx: &mut PreRenderContext) {
        let mut glow = self.glow.take().unwrap_or_else(|| {
            (0..GLOW.len())
                .map(|i| {
                    (
                        ctx.new_render_texture(format!("bridge/glow{i}")),
                        ctx.new_render_texture(format!("bridge/glow{i}/scratch")),
                    )
                })
                .collect()
        });
        for ((texture, scratch), (shrink, _)) in glow.iter_mut().zip(GLOW) {
            let depth_stencil = ctx.depth_stencil;
            let mut pass = ctx.begin_pass(
                texture.as_texture_target(),
                Some(depth_stencil),
                "bridge/glow",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            self.draw_doors(&mut pass, glow_region(shrink), FloatColor4::BLACK);
            drop(pass);
            apply_region_blur(ctx, texture, scratch, shrink);
        }
        self.glow = Some(glow);
    }

    /// The scenario to start, once the bridge is over
    pub fn update(&mut self, frames: f32) -> Option<u8> {
        for tweener in [
            &mut self.angle,
            &mut self.camera,
            &mut self.white,
            &mut self.timer,
        ] {
            ui::advance(tweener, frames);
        }
        self.timer.is_idle().then_some(self.scenario)
    }

    /// `0x71000e4be0`: white, then the two doors tinted by `tint`, in clip space mapped by `region`
    fn draw_doors(&self, pass: &mut RenderPass, region: Mat4, tint: FloatColor4) {
        let full = [0.0, 0.0, 1920.0, 1080.0];
        ui::fill_with(
            pass,
            region * ui::projection(),
            full,
            FloatColor4::WHITE,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
        );
        // a camera 1000 px away sees exactly the screen (fovy = 2 atan 0.54); the doors hinge on the outer edges and
        // swing away from the camera
        let projection = region
            * Mat4::perspective_rh(2.0 * 0.54f32.atan(), 16.0 / 9.0, 1.0, 10000.0)
            * Mat4::from_translation(vec3(0.0, 0.0, -1000.0 + self.camera.value()));
        let angle = self.angle.value() * FRAC_PI_2;
        let doors = [
            (
                [-960.0, -540.0, 0.0, 540.0],
                [0.0, 0.0, 0.5, 1.0],
                Mat4::from_translation(vec3(-960.0, 0.0, 0.0))
                    * Mat4::from_rotation_y(angle)
                    * Mat4::from_translation(vec3(960.0, 0.0, 0.0)),
            ),
            (
                [0.0, -540.0, 960.0, 540.0],
                [0.5, 0.0, 1.0, 1.0],
                Mat4::from_translation(vec3(960.0, 0.0, 0.0))
                    * Mat4::from_rotation_y(-angle)
                    * Mat4::from_translation(vec3(-960.0, 0.0, 0.0)),
            ),
        ];
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        for ([x0, y0, x1, y1], [u0, v0, u1, v1], model) in doors {
            let color = tint.into_unorm();
            let vertex = |x: f32, y: f32, u: f32, v: f32| PosColTexVertex {
                position: vec3(x, y, 0.0),
                color,
                texture_position: glam::vec2(u, v),
            };
            let vertices = [
                vertex(x0, y0, u0, v0),
                vertex(x1, y0, u1, v0),
                vertex(x0, y1, u0, v1),
                vertex(x1, y1, u1, v1),
            ];
            pass.run(opaque.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    sprite: self.snapshot.as_texture_source(),
                    transform: projection * model,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }

    /// `0x71000e50d0`: the doors on white, the glow added over them, then the last quad in the timer's grey: added
    /// while `white` rises, opaque once it is full (so the white screen goes to black as the timer runs down)
    pub fn render(&self, pass: &mut RenderPass) {
        let full = [0.0, 0.0, 1920.0, 1080.0];
        self.draw_doors(pass, Mat4::IDENTITY, FloatColor4::WHITE);
        let additive = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
        for ((texture, _), (shrink, grey)) in self.glow.iter().flatten().zip(GLOW) {
            ui::draw_texture(
                pass,
                texture.as_texture_source(),
                ui::projection(),
                full,
                [0.0, 0.0, 1.0 / shrink, 1.0 / shrink],
                FloatColor4::from_rgba(grey, grey, grey, 1.0),
                additive,
            );
        }
        let white = self.white.value();
        let grey = self.timer.value();
        let blend = if white < 1.0 {
            ColorBlendType::Layer2
        } else {
            ColorBlendType::Opaque
        };
        ui::fill_with(
            pass,
            ui::projection(),
            full,
            FloatColor4::from_rgba(grey, grey, grey, white),
            RenderRequestBuilder::new().color_blend_type(blend),
        );
    }
}
