//! The characters (`shin::CharsMode`, ctor `0x710004e1c0`, update `0x7100050c50`, render `0x7100050f80`, keys
//! `0x7100053ae0`): a grid of portraits linked by lines, up to four pages ("worlds") of it, and the chosen
//! character's full picture, name and profile. From the title (grids 0-7, the episodes) the player can switch each
//! character's life state (Y, "Execute"/"Resurrect"), outfit (X, "Change") and the sub-characters of a cell (A); in
//! the game the grid the script chose shows each character as it is.
//!
//! The portraits' looks (programs 48-51 over the `non` card) are composed here in memory: the portrait over the card,
//! dead in red (`rgb × (1, 0.1, 0.1)`), missing in grey (BT.601), half dead red below the diagonal; all four measured
//! against the original in Eden.
//!
//! shortcut: the grid, the buttons and the profile cannot be used with the pointer, and the full pictures load
//! synchronously.

use std::{collections::HashMap, sync::Arc};

use glam::{Mat4, vec3};
use image::RgbaImage;
use shin_core::{
    format::scenario::info::{
        CharsGridConnectorColor, CharsGridSegment, CharsPortraitDisplayMode, CharsSpriteSegment,
    },
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, gpu_texture::GpuTexture, render_pass::RenderPass,
    resize::CanvasSize,
};
use tracing::warn;

use super::{
    MenuAssets, Task, TaskCx,
    manual::{KeyHelp, draw_obi},
    tips::draw_window,
};
use crate::{
    adv::hud::{OUTLINE, text_layout},
    asset::texture_archive::{NamedTextures, TextureImages},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    render::PreRenderContext,
    ui,
};

const VIEW: f32 = 730.0;
const TEXT_X: f32 = 544.0;
const TEXT_Y: f32 = 188.0;
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.18431, 0.09804, 0.07843, 1.0);
/// The pages' colours for the flowers behind the grid (`0x710022cd40`)
const PAGE_COLORS: [[f32; 3]; 3] = [
    [0.651, 0.0, 0.0],
    [0.573, 0.361, 0.0],
    [0.529, 0.529, 0.529],
];
const GREY: FloatColor4 = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);

/// The buttons under the grid (`0x710022cd70`): texture, rect, uv
const BUTTONS: [(&str, [f32; 4], [f32; 4]); 5] = [
    ("lr", [0.0, 0.0, 184.0, 56.0], [0.0, 0.0, 0.5, 1.0]),
    ("lr", [216.0, 0.0, 400.0, 56.0], [0.5, 0.0, 1.0, 1.0]),
    (
        "button",
        [12.0, 56.0, 260.0, 120.0],
        [0.0, 0.0, 0.6667, 0.5],
    ),
    (
        "button",
        [12.0, 120.0, 260.0, 184.0],
        [0.0, 0.5, 0.6667, 1.0],
    ),
    (
        "button",
        [264.0, 56.0, 388.0, 184.0],
        [0.6667, 0.0, 1.0, 1.0],
    ),
];
const PREV: usize = 0;
const NEXT: usize = 1;
const EXECUTE: usize = 2;
const RESURRECT: usize = 3;
const CHANGE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonState {
    Shown,
    Hidden,
    Disabled,
}

struct Variant {
    index: u8,
    mode: CharsPortraitDisplayMode,
    portrait: String,
    full: String,
}

struct CharState {
    index: u8,
    variants: Vec<Variant>,
    name: String,
    description: String,
}

/// A character's states as the sprite table defines them
fn character_states(segments: &[CharsSpriteSegment]) -> Vec<CharState> {
    let mut states: Vec<CharState> = Vec::new();
    for segment in segments {
        match segment {
            CharsSpriteSegment::BeginState { index } => states.push(CharState {
                index: *index,
                variants: Vec::new(),
                name: String::new(),
                description: String::new(),
            }),
            CharsSpriteSegment::SpriteVariant {
                variant_index,
                portrait_display_mode,
                portrait_texture_name,
                full_texture_name,
            } => {
                if let Some(state) = states.last_mut() {
                    state.variants.push(Variant {
                        index: *variant_index,
                        mode: *portrait_display_mode,
                        portrait: portrait_texture_name.as_str().to_string(),
                        full: full_texture_name.as_str().to_string(),
                    });
                }
            }
            CharsSpriteSegment::Texts { name, description } => {
                if let Some(state) = states.last_mut() {
                    state.name = name.as_str().to_string();
                    state.description = description.as_str().to_string();
                }
            }
        }
    }
    states
}

/// A portrait of a cell, with the states and outfits it can switch between
struct Portrait {
    default_state: u8,
    states: Vec<CharState>,
    /// The listed states (indexes into `states`), each with its listed variants (indexes) and the current one
    slots: Vec<(usize, Vec<usize>, usize)>,
    slot: usize,
}

impl Portrait {
    fn new(
        states: Vec<CharState>,
        default_state: u8,
        default_variant: u8,
        switchable: bool,
    ) -> Self {
        let mut slots = Vec::new();
        let mut slot = 0;
        for (s, state) in states.iter().enumerate() {
            if !switchable && state.index != default_state {
                continue;
            }
            let variants = (0..state.variants.len())
                .filter(|&v| switchable || state.variants[v].index == default_variant)
                .collect::<Vec<_>>();
            let current = variants
                .iter()
                .position(|&v| state.variants[v].index == default_variant)
                .unwrap_or(0);
            if state.index == default_state {
                slot = slots.len();
            }
            slots.push((s, variants, current));
        }
        Self {
            default_state,
            states,
            slots,
            slot,
        }
    }

    /// The state and outfit shown, or `None` for an empty frame
    fn current(&self) -> Option<(&CharState, &Variant)> {
        if self.default_state == 0 {
            return None;
        }
        let (s, variants, v) = self.slots.get(self.slot)?;
        let state = &self.states[*s];
        Some((state, state.variants.get(*variants.get(*v)?)?))
    }

    fn variant_count(&self) -> usize {
        self.slots
            .get(self.slot)
            .map_or(0, |(_, variants, _)| variants.len())
    }
}

struct Cell {
    page: u8,
    x: u8,
    y: u8,
    portraits: Vec<Portrait>,
    sub: usize,
    hover: Tweener,
    flash: Tweener,
}

impl Cell {
    fn portrait(&self) -> &Portrait {
        &self.portraits[self.sub]
    }

    fn selectable(&self) -> bool {
        self.portraits[0].default_state != 0
    }
}

struct Connector {
    page: u8,
    x: u8,
    y: u8,
    shape: u8,
    color: CharsGridConnectorColor,
}

struct Page {
    present: bool,
    cell: Option<usize>,
    scroll_x: f32,
    origin: (f32, f32),
}

/// An action that applies once its button or cell has flashed (the update tasks `$_2` to `$_5`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    Page(u8),
    Status(usize),
    Dress(usize),
    Sub,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// `Start $_6`: the screen opens over 10 frames
    Opening,
    Active,
    /// The close button flashes
    Closing,
    /// `Exit $_8`: the screen fades over 10 frames
    Fading,
}

pub struct Chars {
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    images: Arc<TextureImages>,
    /// The portraits as drawn: by texture name and display mode
    portraits: HashMap<(String, u8), GpuTexture>,
    switchable: bool,
    cells: Vec<Cell>,
    connectors: Vec<Connector>,
    pages: [Page; 4],
    page_count: usize,
    page: u8,
    page_scroll: Tweener,
    colors: [Tweener; 3],
    buttons: [(ButtonState, Tweener); 5],
    pending: Option<Pending>,
    name: Option<TextBlock>,
    description: Option<TextBlock>,
    name_height: f32,
    total_height: f32,
    profile_scroll: f32,
    full: Option<Arc<NamedTextures>>,
    open: f32,
    close_flash: Tweener,
    state: State,
    target_size: (u32, u32),
}

impl Chars {
    /// `0x7100055060`: the grid `grid` (0-7 the episodes' from the title), starting on `page`
    pub fn new(cx: &TaskCx, assets: &MenuAssets, grid: i32, page: i32) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/chars.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /chars.txa: {e:?}");
                return None;
            }
        };
        let images = cx.assets.load_sync::<TextureImages>("/chars.txa").ok()?;
        let tables = assets.scenario.info_tables();
        let Some(grid_info) = usize::try_from(grid)
            .ok()
            .and_then(|grid| tables.chars_grid_info.get(grid))
        else {
            warn!("No characters grid {grid}");
            return None;
        };
        let switchable = grid < 8;

        // portraits sharing a place form a cell, in order of first appearance
        let mut cells: Vec<Cell> = Vec::new();
        let mut connectors = Vec::new();
        for segment in &grid_info.segments {
            match segment {
                &CharsGridSegment::Portrait {
                    page,
                    grid_x,
                    grid_y,
                    character_id,
                    default_state,
                    default_variant,
                } => {
                    let states = tables
                        .chars_sprite_info
                        .get(character_id.into_i32() as usize)
                        .map_or_else(Vec::new, |info| character_states(&info.segments));
                    let portrait =
                        Portrait::new(states, default_state, default_variant, switchable);
                    match cells
                        .iter_mut()
                        .find(|cell| (cell.page, cell.x, cell.y) == (page, grid_x, grid_y))
                    {
                        Some(cell) => cell.portraits.push(portrait),
                        None => cells.push(Cell {
                            page,
                            x: grid_x,
                            y: grid_y,
                            portraits: vec![portrait],
                            sub: 0,
                            hover: Tweener::new(0.0),
                            flash: Tweener::new(0.0),
                        }),
                    }
                }
                &CharsGridSegment::Connector {
                    page,
                    grid_x,
                    grid_y,
                    shape,
                    color,
                } => connectors.push(Connector {
                    page,
                    x: grid_x,
                    y: grid_y,
                    shape: shape as u8,
                    color,
                }),
            }
        }

        let mut pages: [Page; 4] = std::array::from_fn(|_| Page {
            present: false,
            cell: None,
            scroll_x: 0.0,
            origin: (0.0, 0.0),
        });
        let mut rank = 0;
        for (p, record) in pages.iter_mut().enumerate() {
            if !cells.iter().any(|cell| cell.page as usize == p) {
                continue;
            }
            *record = Page {
                present: true,
                cell: cells
                    .iter()
                    .position(|cell| cell.page as usize == p && cell.selectable()),
                scroll_x: 400.0 * rank as f32,
                origin: (400.0 * rank as f32 + 20.0, 16.0),
            };
            rank += 1;
        }
        let page_count = rank;
        let page = (page & 3) as usize;
        let page = [page, page + 1, page + 2, page + 3]
            .into_iter()
            .map(|p| p % 4)
            .find(|&p| pages[p].present)
            .unwrap_or(0);
        for record in &pages {
            if let Some(cell) = record.cell {
                cells[cell].hover = Tweener::new(1.0);
            }
        }
        let color = PAGE_COLORS[page.min(2)];

        let mut chars = Self {
            assets: assets.clone(),
            textures,
            images,
            portraits: HashMap::new(),
            switchable,
            cells,
            connectors,
            page_scroll: Tweener::new(pages[page].scroll_x),
            pages,
            page_count,
            page: page as u8,
            colors: color.map(Tweener::new),
            buttons: std::array::from_fn(|_| (ButtonState::Shown, Tweener::new(0.0))),
            pending: None,
            name: None,
            description: None,
            name_height: 0.0,
            total_height: 0.0,
            profile_scroll: 0.0,
            full: None,
            open: 0.0,
            close_flash: Tweener::new(0.0),
            state: State::Opening,
            target_size: (1920, 1080),
        };
        chars.update_buttons();
        chars.build_texts(cx);
        chars.build_portraits(cx.pre_render);
        Some(chars)
    }

    fn current_cell(&self) -> Option<&Cell> {
        self.pages[self.page as usize]
            .cell
            .map(|cell| &self.cells[cell])
    }

    /// `0x71000502b0`
    fn update_buttons(&mut self) {
        let present = |p: i32| (0..4).contains(&p) && self.pages[p as usize].present;
        let page = self.page as i32;
        let side = |before: bool| {
            let any = if before {
                (0..page).any(present)
            } else {
                (page + 1..4).any(present)
            };
            if self.page_count < 2 {
                ButtonState::Hidden
            } else if any {
                ButtonState::Shown
            } else {
                ButtonState::Disabled
            }
        };
        let (prev, next) = (side(true), side(false));
        let portrait = self.current_cell().map(Cell::portrait);
        let states = portrait.map_or(0, |p| p.slots.len());
        let slot = portrait.map_or(0, |p| p.slot);
        let variants = portrait.map_or(0, Portrait::variant_count);
        let enabled = |on: bool| {
            if on {
                ButtonState::Shown
            } else {
                ButtonState::Disabled
            }
        };
        let (execute, resurrect, change) = if !self.switchable {
            (
                ButtonState::Hidden,
                ButtonState::Hidden,
                ButtonState::Hidden,
            )
        } else {
            (
                enabled(states >= 2 && slot + 1 < states),
                enabled(states >= 2 && slot > 0),
                enabled(variants >= 2),
            )
        };
        for (k, state) in [prev, next, execute, resurrect, change]
            .into_iter()
            .enumerate()
        {
            self.buttons[k].0 = state;
        }
    }

    /// `0x7100050410`: the chosen character's name (48 px) and profile (32 px), 966 px wide, and its full picture
    fn build_texts(&mut self, cx: &TaskCx) {
        let Some((state, variant)) = self
            .current_cell()
            .and_then(|cell| cell.portrait().current())
        else {
            return;
        };
        let (name, description, full) = (
            state.name.clone(),
            state.description.clone(),
            variant.full.clone(),
        );
        let font = &self.assets.system_font;
        let layout = |size: f32, profile: bool| LayoutParams {
            always_leave_space_for_rubi: profile,
            perform_soft_breaks: profile,
            ..text_layout(966.0, size)
        };
        let name = TextBlock::new(cx.pre_render, font, &name, layout(48.0, false), OUTLINE);
        let description = TextBlock::new(
            cx.pre_render,
            font,
            &description,
            layout(32.0, true),
            OUTLINE,
        );
        let height = name.size().y;
        self.name_height = height + 5.0;
        self.total_height = height + 8.0 + description.size().y + 5.0;
        self.name = Some(name);
        self.description = Some(description);
        self.profile_scroll = 0.0;
        self.load_full(cx, &full);
    }

    /// `0x7100052c00`: `/chars/<name>.txa`; the old picture stays when it cannot load
    fn load_full(&mut self, cx: &TaskCx, name: &str) {
        let path = format!("/chars/{}.txa", name.to_ascii_lowercase());
        match cx.assets.load_sync::<NamedTextures>(&path) {
            Ok(full) => self.full = Some(full),
            Err(e) => warn!("Could not load {path}: {e:?}"),
        }
    }

    /// The portraits every cell shows now, composed over the card in their display mode
    fn build_portraits(&mut self, ctx: &PreRenderContext) {
        let shown = self
            .cells
            .iter()
            .map(|cell| portrait_key(cell.portrait()))
            .collect::<Vec<_>>();
        let Some(card) = self.images.by_name.get("non") else {
            return;
        };
        for key in shown {
            if self.portraits.contains_key(&key) {
                continue;
            }
            let Some(texture) = self.images.by_name.get(&key.0) else {
                warn!("chars.txa has no portrait {}", key.0);
                continue;
            };
            let image = compose_portrait(card, texture, key.1);
            let gpu = GpuTexture::new_static_from_rgba_image(
                ctx.device,
                ctx.queue,
                Some(&format!("chars/{}/{}", key.0, key.1)),
                &image,
            );
            self.portraits.insert(key, gpu);
        }
    }

    /// `0x7100053ae0`
    fn handle_input(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        // L/R: the previous or next present page
        if self.page_count > 0 && (input.l || input.r) {
            let page = self.page as i32;
            let to = if input.l {
                (0..page).rev().find(|&p| self.pages[p as usize].present)
            } else {
                (page + 1..4).find(|&p| self.pages[p as usize].present)
            };
            if let Some(to) = to {
                // `0x7100052f40`
                cx.sound("move");
                let button = if to < page { PREV } else { NEXT };
                minigame::flash(&mut self.buttons[button].1);
                self.pending = Some(Pending::Page(to as u8));
            }
            return;
        }
        let dx = if input.right {
            1.0
        } else if input.left {
            -1.0
        } else {
            0.0
        };
        let dy = if input.down {
            1.0
        } else if input.up {
            -1.0
        } else {
            0.0
        };
        if dx != 0.0 || dy != 0.0 {
            self.move_cursor(cx, glam::vec2(dx, dy));
            return;
        }
        let Some(cell) = self.pages[self.page as usize].cell else {
            if input.cancel {
                self.close(cx);
            }
            return;
        };
        if self.switchable && input.x {
            // `0x7100053550`: the next outfit, wrapping to the first
            let portrait = self.cells[cell].portrait();
            let count = portrait.variant_count();
            let current = portrait.slots.get(portrait.slot).map_or(0, |slot| slot.2);
            let to = if current + 1 < count { current + 1 } else { 0 };
            if to != current {
                cx.sound("decide");
                minigame::flash(&mut self.buttons[CHANGE].1);
                self.pending = Some(Pending::Dress(to));
            }
        } else if self.switchable && input.y {
            self.change_status(cx, cell, 1);
        } else if input.confirm {
            // `0x71000530e0`
            if self.cells[cell].portraits.len() >= 2 {
                cx.sound("decide");
                let flash = &mut self.cells[cell].flash;
                *flash = Tweener::new(0.0);
                minigame::flash(flash);
                self.pending = Some(Pending::Sub);
            }
        } else if input.cancel {
            self.close(cx);
        } else if input.scroll != 0.0 {
            self.profile_scroll = (self.profile_scroll + input.scroll * 16.0)
                .clamp(0.0, (self.total_height - VIEW).max(0.0));
        }
    }

    /// `0x71000532f0`: the next (or previous) life state, wrapping both ways
    fn change_status(&mut self, cx: &TaskCx, cell: usize, dir: isize) {
        let portrait = self.cells[cell].portrait();
        let count = portrait.slots.len() as isize;
        if count == 0 {
            return;
        }
        let from = portrait.slot;
        let to = (from as isize + dir).rem_euclid(count) as usize;
        if to == from {
            return;
        }
        let (button, sound) = if to >= from {
            (EXECUTE, "execute")
        } else {
            (RESURRECT, "resurrect")
        };
        cx.sound(sound);
        minigame::flash(&mut self.buttons[button].1);
        self.pending = Some(Pending::Status(to));
    }

    /// `0x7100053840`
    fn close(&mut self, cx: &TaskCx) {
        cx.sound("cancel");
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// The d-pad: the nearest selectable cell of the page in the pressed direction, favouring straight lines
    fn move_cursor(&mut self, cx: &mut TaskCx, dir: glam::Vec2) {
        let Some(current) = self.pages[self.page as usize].cell else {
            return;
        };
        let from = &self.cells[current];
        let origin = glam::vec2(from.x as f32, from.y as f32);
        let dir = dir.normalize();
        let mut best = (f32::MAX, None);
        for (i, cell) in self.cells.iter().enumerate() {
            if i == current || cell.page != self.page || !cell.selectable() {
                continue;
            }
            let d = glam::vec2(cell.x as f32, cell.y as f32) - origin;
            let length = d.length();
            if length == 0.0 {
                continue;
            }
            let c = dir.dot(d / length);
            if c > f32::EPSILON {
                let score = length / c.powi(4);
                if score < best.0 {
                    best = (score, Some(i));
                }
            }
        }
        let Some(to) = best.1 else {
            return;
        };
        cx.sound("move");
        minigame::tween_now(&mut self.cells[current].hover, 0.0, 0.1);
        self.pages[self.page as usize].cell = Some(to);
        minigame::tween_now(&mut self.cells[to].hover, 1.0, 0.25);
        self.update_buttons();
        self.build_texts(cx);
    }

    /// The action whose flash has ended
    fn apply(&mut self, cx: &mut TaskCx, pending: Pending) {
        let cell = self.pages[self.page as usize].cell;
        match pending {
            Pending::Page(page) => {
                // `$_2`: the grid slides to the page in 10 frames and the flowers take its colour
                let target = self.pages[page as usize].scroll_x;
                ui::tween(&mut self.page_scroll, target, 10.0, Easing::SineOut);
                let color = PAGE_COLORS[(page as usize).min(2)];
                for (tweener, value) in self.colors.iter_mut().zip(color) {
                    ui::tween(tweener, value, 10.0, Easing::Linear);
                }
                self.page = page;
                self.update_buttons();
                self.build_texts(cx);
            }
            Pending::Status(slot) => {
                if let Some(cell) = cell {
                    let sub = self.cells[cell].sub;
                    self.cells[cell].portraits[sub].slot = slot;
                }
                self.update_buttons();
                self.build_texts(cx);
                self.build_portraits(cx.pre_render);
            }
            Pending::Dress(variant) => {
                if let Some(cell) = cell {
                    let sub = self.cells[cell].sub;
                    let portrait = &mut self.cells[cell].portraits[sub];
                    let slot = portrait.slot;
                    if let Some(record) = portrait.slots.get_mut(slot) {
                        record.2 = variant;
                    }
                    let full = portrait.current().map(|(_, v)| v.full.clone());
                    if let Some(full) = full {
                        self.load_full(cx, &full);
                    }
                }
                self.build_portraits(cx.pre_render);
            }
            Pending::Sub => {
                if let Some(cell) = cell {
                    let cell = &mut self.cells[cell];
                    cell.sub = (cell.sub + 1) % cell.portraits.len();
                }
                self.update_buttons();
                self.build_texts(cx);
                self.build_portraits(cx.pre_render);
            }
        }
    }

    fn pending_done(&self, pending: Pending) -> bool {
        match pending {
            Pending::Page(_) => self.buttons[PREV].1.is_idle() && self.buttons[NEXT].1.is_idle(),
            Pending::Status(_) => {
                self.buttons[EXECUTE].1.is_idle() && self.buttons[RESURRECT].1.is_idle()
            }
            Pending::Dress(_) => self.buttons[CHANGE].1.is_idle(),
            Pending::Sub => self.current_cell().is_none_or(|cell| cell.flash.is_idle()),
        }
    }

    fn render_grid(&self, pass: &mut RenderPass, transform: Mat4) {
        let t = |name: &str| self.textures.get(name);
        let builder = ui::builder();
        ui::fill(
            pass,
            transform,
            [0.0, 0.0, 400.0, 526.0],
            FloatColor4::BLACK.with_alpha(0.75),
        );
        let [r, g, b] = self.colors.each_ref().map(Tweener::value);
        let mut hana = Sprite::new(t("hana"), [0.0, 0.0, 380.0, 440.0]).uv([1.0, 1.0, 0.0, 0.0]);
        hana.color = FloatColor4::from_rgba(r, g, b, 0.5);
        hana.draw(pass, builder, transform);

        let scroll = self.page_scroll.value();
        let line = t("line");
        for connector in &self.connectors {
            let origin = self.pages[connector.page as usize].origin;
            let x = origin.0 + 24.0 * connector.x as f32 - scroll;
            let y = origin.1 + 26.0 * connector.y as f32;
            if x >= 400.0 || x + 24.0 < 0.0 {
                continue;
            }
            let (u, v) = (
                ((connector.shape & 3) as f32 * 32.0 + 4.0) / 128.0,
                ((connector.shape >> 2) as f32 * 36.0 + 5.0) / 144.0,
            );
            let mut sprite =
                Sprite::new(line, [x, y, x + 24.0, y + 26.0]).uv([u, v, u + 0.1875, v + 0.1805556]);
            sprite.color = match connector.color {
                CharsGridConnectorColor::Red => FloatColor4::from_rgba(0.753, 0.078, 0.235, 1.0),
                CharsGridConnectorColor::Blue => FloatColor4::from_rgba(0.235, 0.086, 0.878, 1.0),
                CharsGridConnectorColor::Yellow => FloatColor4::from_rgba(0.941, 0.816, 0.078, 1.0),
            };
            sprite.draw(pass, builder, transform);
        }

        for cell in &self.cells {
            let origin = self.pages[cell.page as usize].origin;
            let x = origin.0 + 24.0 * cell.x as f32 - scroll - 26.0;
            let y = origin.1 + 26.0 * cell.y as f32 - 30.0;
            if x >= 400.0 || x + 76.0 < 0.0 {
                continue;
            }
            let rect = [x, y, x + 76.0, y + 86.0];
            if let Some(texture) = self.portraits.get(&portrait_key(cell.portrait())) {
                Sprite::new(texture, rect).draw(pass, builder, transform);
            }
            if cell.portraits.len() >= 2 {
                Sprite::new(t("numbase"), [x + 36.0, y + 66.0, x + 76.0, y + 86.0])
                    .draw(pass, builder, transform);
                let text = format!("{}/{}", cell.sub + 1, cell.portraits.len());
                self.draw_number(pass, transform, x + 72.0, y + 68.0, &text);
            }
            let hover = cell.hover.value() * 0.5;
            if hover > 0.0 {
                Sprite::new(t("cur"), rect)
                    .alpha(hover)
                    .draw(pass, builder, transform);
            }
            let flash = cell.flash.value();
            if flash > 0.0 {
                Sprite::new(t("cur"), rect)
                    .alpha(flash)
                    .draw(pass, builder, transform);
            }
        }
    }

    /// `0x71000527b0`: right-aligned `num` glyphs, 10 px apart
    fn draw_number(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        right: f32,
        top: f32,
        text: &str,
    ) {
        let glyph = |c: char| match c {
            '0'..='9' => Some((c as u32 - '0' as u32) as f32 * 16.0),
            '/' => Some(160.0),
            _ => None,
        };
        let advance: f32 = text
            .chars()
            .map(|c| if glyph(c).is_some() { 10.0 } else { 12.0 })
            .sum();
        let mut pen = right - advance;
        for c in text.chars() {
            let Some(u) = glyph(c) else {
                pen += 12.0;
                continue;
            };
            let x = pen - 3.0;
            Sprite::new(self.textures.get("num"), [x, top, x + 16.0, top + 16.0])
                .uv([u / 176.0, 0.0, (u + 16.0) / 176.0, 1.0])
                .draw(pass, ui::builder(), transform);
            pen += 10.0;
        }
    }
}

/// The texture and display mode a portrait is drawn with: an empty frame shows the bare card
fn portrait_key(portrait: &Portrait) -> (String, u8) {
    match portrait.current() {
        Some((_, variant)) => (variant.portrait.clone(), variant.mode as u8),
        None => ("null".to_string(), 0),
    }
}

/// Programs 48-51: the portrait over its card, in full colour, dead red, missing grey, or half dead (red below the
/// diagonal from the top right to the bottom left)
fn compose_portrait(card: &RgbaImage, texture: &RgbaImage, mode: u8) -> RgbaImage {
    let (w, h) = card.dimensions();
    RgbaImage::from_fn(w, h, |x, y| {
        let c = card.get_pixel(x, y).0.map(|v| v as f32 / 255.0);
        let t = if x < texture.width() && y < texture.height() {
            texture.get_pixel(x, y).0.map(|v| v as f32 / 255.0)
        } else {
            [0.0; 4]
        };
        let dead = mode == 1 || (mode == 3 && x as f32 / w as f32 + y as f32 / h as f32 > 1.0);
        let rgb = if dead {
            [t[0], t[1] * 0.1, t[2] * 0.1]
        } else if mode == 2 {
            let grey = 0.299 * t[0] + 0.587 * t[1] + 0.114 * t[2];
            [grey; 3]
        } else {
            [t[0], t[1], t[2]]
        };
        let mix = |i: usize| ((c[i] * (1.0 - t[3]) + rgb[i] * t[3]) * 255.0).round() as u8;
        image::Rgba([mix(0), mix(1), mix(2), card.get_pixel(x, y).0[3]])
    })
}

impl Task for Chars {
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
                if let Some(pending) = self.pending {
                    if self.pending_done(pending) {
                        self.pending = None;
                        self.apply(cx, pending);
                    }
                } else if cx.focused && self.page_scroll.is_idle() {
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
        let frames = cx.frames;
        ui::advance(&mut self.page_scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        for tweener in &mut self.colors {
            ui::advance(tweener, frames);
        }
        for (_, flash) in &mut self.buttons {
            ui::advance(flash, frames);
        }
        for cell in &mut self.cells {
            ui::advance(&mut cell.hover, frames);
            ui::advance(&mut cell.flash, frames);
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

        let slide = 1920.0 - 1920.0 * a;
        let window = projection * Mat4::from_translation(vec3(slide, 0.0, 0.0));
        draw_window(
            pass,
            t("window"),
            window,
            [0.0, 80.0, 1526.0, 1606.0],
            [0.0, 0.0499376, 0.9525593, 1.0024968],
        );
        Sprite::new(t("separator"), [510.0, 203.0, 528.0, 903.0]).draw(pass, builder, window);

        // the grid, clipped to its 400x526 viewport
        ui::clip(
            pass,
            Some([90.0 + slide, 198.0, 490.0 + slide, 724.0]),
            self.target_size,
        );
        self.render_grid(
            pass,
            window * Mat4::from_translation(vec3(90.0, 198.0, 0.0)),
        );
        ui::clip(pass, None, self.target_size);

        let buttons = window * Mat4::from_translation(vec3(90.0, 724.0, 0.0));
        for (k, (state, flash)) in self.buttons.iter().enumerate() {
            if *state == ButtonState::Hidden {
                continue;
            }
            let (texture, rect, uv) = BUTTONS[k];
            let mut sprite = Sprite::new(t(texture), rect).uv(uv).flash(flash.value());
            if *state == ButtonState::Disabled {
                sprite.color = GREY;
            }
            sprite.draw(pass, builder, buttons);
        }

        // the profile: the rule under the name, the full picture, then the texts over it
        let profile = Some([TEXT_X + slide, TEXT_Y, 1520.0 + slide, 918.0]);
        let scroll = self.profile_scroll;
        if self.name.is_some() {
            ui::clip(pass, profile, self.target_size);
            let rule = TEXT_Y + self.name_height - scroll;
            Sprite::new(t("bar"), [TEXT_X, rule - 20.0, 1520.0, rule - 4.0]).draw(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                window,
            );
            ui::clip(pass, None, self.target_size);
        }
        if let Some(full) = self.full.as_ref().and_then(|full| full.first()) {
            Sprite::new(full, [1380.0, 100.0, 2082.0, 1072.0]).draw(pass, builder, window);
        }
        if let (Some(name), Some(description)) = (&self.name, &self.description) {
            ui::clip(pass, profile, self.target_size);
            let text = window * Mat4::from_translation(vec3(TEXT_X, TEXT_Y - scroll, 0.0));
            ui::draw_text(
                pass,
                name,
                text * Mat4::from_translation(vec3(5.0 - OUTLINE, 5.0 - OUTLINE, 0.0)),
                FloatColor4::WHITE,
                BORDER,
            );
            ui::draw_text(
                pass,
                description,
                text * Mat4::from_translation(vec3(
                    5.0 - OUTLINE,
                    self.name_height + 3.0 - OUTLINE,
                    0.0,
                )),
                FloatColor4::WHITE,
                BORDER,
            );
            ui::clip(pass, None, self.target_size);
        }
        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            window,
            (1520.0, TEXT_Y),
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

        // the key help, right to left (`0x71000529c0`, `0x710016ef90`)
        let help = KeyHelp {
            labels: t("help_chars"),
            keys: t("key"),
            label_height: 336.0,
            y: 1080.0 - 88.0 * a + 20.0,
            glyph_offset: 2.0,
            gap: 16.0,
        };
        let white = FloatColor4::WHITE;
        let on = |yes: bool| if yes { white } else { GREY };
        let cell = self.current_cell();
        let portrait = cell.map(Cell::portrait);
        let mut x = help.draw(pass, 1872.0, &[4], 48.0, 72.0, white);
        x = help.draw(
            pass,
            x,
            &[9],
            144.0,
            176.0,
            on(self.total_height - VIEW > 0.0),
        );
        x = help.draw(pass, x, &[0], 0.0, 206.0, white);
        if self.page_count >= 2 {
            x = help.draw(pass, x, &[7], 288.0, 238.0, white);
        }
        let subs = cell.is_some_and(|cell| cell.portraits.len() >= 2);
        x = help.draw(pass, x, &[3], 96.0, 272.0, on(subs));
        if self.switchable {
            let states = portrait.is_some_and(|p| p.slots.len() >= 2);
            x = help.draw(pass, x, &[6], 240.0, 140.0, on(states));
            let variants = portrait.is_some_and(|p| p.variant_count() >= 2);
            help.draw(pass, x, &[5], 192.0, 140.0, on(variants));
        }
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::compose_portrait;

    #[test]
    fn portrait_modes() {
        let card = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        let face = RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]));
        assert_eq!(
            compose_portrait(&card, &face, 0).get_pixel(0, 0).0,
            [200, 100, 50, 255]
        );
        assert_eq!(
            compose_portrait(&card, &face, 1).get_pixel(0, 0).0,
            [200, 10, 5, 255]
        );
        let grey = (0.299f32 * 200.0 + 0.587 * 100.0 + 0.114 * 50.0).round() as u8;
        assert_eq!(
            compose_portrait(&card, &face, 2).get_pixel(0, 0).0,
            [grey, grey, grey, 255]
        );
        // half dead: full colour at the top left, red at the bottom right
        let half = compose_portrait(&card, &face, 3);
        assert_eq!(half.get_pixel(0, 0).0, [200, 100, 50, 255]);
        assert_eq!(half.get_pixel(3, 3).0, [200, 10, 5, 255]);
        // where the portrait is transparent the card shows
        let clear = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        assert_eq!(
            compose_portrait(&card, &clear, 1).get_pixel(1, 1).0,
            [10, 20, 30, 255]
        );
    }
}
