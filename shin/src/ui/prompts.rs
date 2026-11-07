//! Port-only: the button prompts follow the device. The screens ask for the Switch glyphs of the game's `key` atlas
//! (8x2 cells of 48: d-pads 0-2 and 8-9, A B X Y 3-6, L/R 7, L R ZL ZR 10-13, + − 14-15); [`draw`] turns the glyph
//! into the action it stands for, finds the button or key bound to it, and draws that in the active style: the
//! game's own glyphs for a Switch pad, glyphs painted here at startup for Xbox and PlayStation pads (by position: the
//! Switch's A, the east button, is Xbox B and PlayStation ○), and key caps with the key's name in the game's font for
//! the keyboard. The style is the PC tab's "Button prompts", or with Auto the device used last.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

use glam::{Mat4, Vec2, vec2, vec3};
use image::RgbaImage;
use shin_core::primitives::color::FloatColor4;
use shin_input::inputs::{GamepadButton, GamepadFamily, InputDevice, VirtualGamepadButton};
use shin_render::{RenderRequestBuilder, gpu_texture::GpuTexture, render_pass::RenderPass};

use crate::{
    adv::hud::{OUTLINE, text_layout},
    app::AppAction,
    asset::font::GpuFontLazy,
    bindings::{self, Input},
    layer::{text_block::TextBlock, user::minigame::Sprite},
    port_settings::{self, ButtonPrompts},
    render::PreRenderContext,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Switch,
    Xbox,
    PlayStation,
    Keyboard,
}

impl Style {
    fn of_family(family: GamepadFamily) -> Self {
        match family {
            GamepadFamily::Nintendo => Style::Switch,
            GamepadFamily::PlayStation => Style::PlayStation,
            GamepadFamily::Xbox | GamepadFamily::Generic => Style::Xbox,
        }
    }

    const ALL: [Style; 4] = [
        Style::Switch,
        Style::Xbox,
        Style::PlayStation,
        Style::Keyboard,
    ];
}

/// A pad's buttons by position, as the prompts tell them apart
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadGlyph {
    South,
    East,
    West,
    North,
    L1,
    R1,
    L2,
    R2,
    Start,
    Select,
    DPad,
    DPadUpDown,
    DPadLeftRight,
    DPadUp,
    DPadDown,
    /// L1 over R1, the Switch's "L/R"
    L1R1,
    L3,
    R3,
}

const PAD_GLYPHS: usize = 18;

impl PadGlyph {
    /// The game's `key` cell, where it has one
    fn switch_index(self) -> Option<u8> {
        use PadGlyph::*;
        Some(match self {
            DPad => 0,
            DPadUpDown => 1,
            DPadLeftRight => 2,
            East => 3,
            South => 4,
            North => 5,
            West => 6,
            L1R1 => 7,
            DPadUp => 8,
            DPadDown => 9,
            L1 => 10,
            R1 => 11,
            L2 => 12,
            R2 => 13,
            Start => 14,
            Select => 15,
            L3 | R3 => return None,
        })
    }

    fn from_switch(glyph: u8) -> Option<Self> {
        use PadGlyph::*;
        [
            DPad,
            DPadUpDown,
            DPadLeftRight,
            East,
            South,
            North,
            West,
            L1R1,
            DPadUp,
            DPadDown,
            L1,
            R1,
            L2,
            R2,
            Start,
            Select,
        ]
        .get(glyph as usize)
        .copied()
    }

    /// The glyph of a pad input; a stick direction shows its stick
    pub fn of_input(input: Input) -> Option<Self> {
        use PadGlyph::*;
        Some(match input {
            // with the bottom-confirm setting the two buttons sit at each other's place (shin_input)
            Input::Pad(button) => match button {
                GamepadButton::B if shin_input::inputs::bottom_confirms() => East,
                GamepadButton::A if shin_input::inputs::bottom_confirms() => South,
                GamepadButton::B => South,
                GamepadButton::A => East,
                GamepadButton::Y => West,
                GamepadButton::X => North,
                GamepadButton::L => L1,
                GamepadButton::R => R1,
                GamepadButton::ZL => L2,
                GamepadButton::ZR => R2,
                GamepadButton::Plus => Start,
                GamepadButton::Minus => Select,
                GamepadButton::Up => DPadUp,
                GamepadButton::Down => DPadDown,
                GamepadButton::Left | GamepadButton::Right => DPadLeftRight,
                GamepadButton::StickL => L3,
                GamepadButton::StickR => R3,
            },
            Input::Stick(direction) => match direction {
                VirtualGamepadButton::StickLUp
                | VirtualGamepadButton::StickLDown
                | VirtualGamepadButton::StickLLeft
                | VirtualGamepadButton::StickLRight => L3,
                _ => R3,
            },
            Input::Key(_) | Input::Mouse(_) => return None,
        })
    }
}

/// The action a Switch glyph stands for on the screens' key help
fn action_of(glyph: u8) -> Option<AppAction> {
    Some(match glyph {
        3 => AppAction::Enter,
        4 => AppAction::Cancel,
        5 => AppAction::ButtonX,
        6 => AppAction::ButtonY,
        10 => AppAction::AutoToggle,
        11 => AppAction::SkipToggle,
        // ZR: the galleries' Save entry (the original showed −)
        13 => AppAction::Save,
        14 => AppAction::Plus,
        15 => AppAction::Minus,
        _ => return None,
    })
}

/// What to draw for a glyph
#[derive(Debug, Clone, PartialEq)]
enum Drawn {
    /// A cell of the screen's own `key` atlas
    Switch(u8),
    /// A cell of the painted atlas
    Painted(usize),
    /// A key cap with this name
    Key(String),
}

/// The painted atlas: a row of [`PadGlyph`]s each for Xbox and PlayStation, then the key caps
const CELL: u32 = 96;
const COLUMNS: u32 = PAD_GLYPHS as u32;
const ROWS: u32 = 3;
const KEY_CAP: usize = 2 * PAD_GLYPHS;
/// The arrow key caps, in the order of the d-pad glyphs from [`PadGlyph::DPad`]
const ARROW_CAPS: usize = KEY_CAP + 1;

fn painted(style: Style, glyph: PadGlyph) -> usize {
    let row = if style == Style::PlayStation { 1 } else { 0 };
    row * PAD_GLYPHS + glyph as usize
}

/// The first input of `action` of the kind the style shows
fn first_input(table: &bindings::Table, action: AppAction, pad: bool) -> Option<Input> {
    table[action].iter().copied().find(|i| i.is_pad() == pad)
}

fn key_name(table: &bindings::Table, action: AppAction) -> Option<String> {
    first_input(table, action, false).map(|i| i.label())
}

/// What glyph `glyph` of the `key` atlas becomes in `style` with the bindings `table`
fn resolve(glyph: u8, style: Style, table: &bindings::Table) -> Drawn {
    use PadGlyph::*;
    if style == Style::Keyboard {
        let arrows = match PadGlyph::from_switch(glyph) {
            Some(d @ (DPad | DPadUpDown | DPadLeftRight | DPadUp | DPadDown)) => {
                Some(ARROW_CAPS + d as usize - DPad as usize)
            }
            _ => None,
        };
        if let Some(cell) = arrows {
            return Drawn::Painted(cell);
        }
        let name = if glyph == 7 {
            key_name(table, AppAction::AutoToggle)
                .zip(key_name(table, AppAction::SkipToggle))
                .map(|(l, r)| format!("{l}/{r}"))
        } else {
            action_of(glyph).and_then(|action| key_name(table, action))
        };
        // ZL stands for no action
        return name.map_or(Drawn::Switch(glyph), Drawn::Key);
    }
    let bound = |action| first_input(table, action, true).and_then(PadGlyph::of_input);
    let pad = if glyph == 7 {
        // shortcut: L/R stays one glyph only while auto and skip are on the shoulders, otherwise auto's button shows
        match (bound(AppAction::AutoToggle), bound(AppAction::SkipToggle)) {
            (Some(L1), Some(R1)) => Some(L1R1),
            (auto, _) => auto,
        }
    } else {
        match action_of(glyph) {
            Some(action) => bound(action),
            None => PadGlyph::from_switch(glyph),
        }
    };
    match (style, pad) {
        (Style::Switch, Some(pad)) => Drawn::Switch(pad.switch_index().unwrap_or(glyph)),
        (_, Some(pad)) => Drawn::Painted(painted(style, pad)),
        // nothing bound on the pad: the original glyph
        (Style::Switch, None) => Drawn::Switch(glyph),
        (_, None) => PadGlyph::from_switch(glyph).map_or(Drawn::Switch(glyph), |pad| {
            Drawn::Painted(painted(style, pad))
        }),
    }
}

static ATLAS: OnceLock<GpuTexture> = OnceLock::new();
/// The PC tab's setting (as [`ButtonPrompts`]) and the device used last
static MODE: AtomicU8 = AtomicU8::new(0);
static DEVICE: AtomicU8 = AtomicU8::new(0);
/// The last gamepad's style, for the Controls screen while the keyboard is in use
static LAST_PAD: AtomicU8 = AtomicU8::new(1);
static TEXT_GENERATION: AtomicU32 = AtomicU32::new(u32::MAX);

thread_local! {
    /// The key caps' names, laid out in the game's font
    static KEY_TEXTS: RefCell<HashMap<String, TextBlock>> = RefCell::new(HashMap::new());
}

fn style_index(style: Style) -> u8 {
    Style::ALL.iter().position(|&s| s == style).unwrap() as u8
}

/// Paints the atlas (once, at startup)
pub fn init(device: &wgpu::Device, queue: &wgpu::Queue) {
    ATLAS.get_or_init(|| {
        GpuTexture::new_static_from_rgba_image(
            device,
            queue,
            Some("button prompts"),
            &paint_atlas(),
        )
    });
}

/// The device a button was last pressed on (every frame, from the raw input)
pub fn set_device(device: InputDevice) {
    let style = match device {
        InputDevice::KeyboardMouse => Style::Keyboard,
        InputDevice::Gamepad(family) => {
            let style = Style::of_family(family);
            LAST_PAD.store(style_index(style), Ordering::Relaxed);
            style
        }
    };
    DEVICE.store(style_index(style), Ordering::Relaxed);
}

/// The style the key help is drawn in
pub fn style() -> Style {
    match MODE.load(Ordering::Relaxed) {
        1 => Style::Xbox,
        2 => Style::PlayStation,
        3 => Style::Switch,
        4 => Style::Keyboard,
        _ => Style::ALL[DEVICE.load(Ordering::Relaxed) as usize],
    }
}

/// The pad style: the active one, or while the keyboard is in use the last pad's (Xbox before any)
pub fn pad_style() -> Style {
    match style() {
        Style::Keyboard => Style::ALL[LAST_PAD.load(Ordering::Relaxed) as usize],
        style => style,
    }
}

/// Once a frame: takes the PC tab's setting and lays out the key names the bindings now need
pub fn refresh(ctx: &PreRenderContext, font: &Arc<GpuFontLazy>) {
    let mode = match port_settings::get().button_prompts {
        ButtonPrompts::Auto => 0,
        ButtonPrompts::Xbox => 1,
        ButtonPrompts::PlayStation => 2,
        ButtonPrompts::Switch => 3,
        ButtonPrompts::Keyboard => 4,
    };
    MODE.store(mode, Ordering::Relaxed);
    let generation = bindings::generation();
    if TEXT_GENERATION.swap(generation, Ordering::Relaxed) == generation {
        return;
    }
    let table = bindings::table();
    KEY_TEXTS.with_borrow_mut(|texts| {
        texts.clear();
        for glyph in 0..16 {
            if let Drawn::Key(name) = resolve(glyph, Style::Keyboard, &table) {
                texts
                    .entry(name.clone())
                    .or_insert_with(|| key_text(ctx, font, &name));
            }
        }
    });
}

fn key_text(ctx: &PreRenderContext, font: &Arc<GpuFontLazy>, name: &str) -> TextBlock {
    TextBlock::new(ctx, font, name, text_layout(1000.0, 30.0), OUTLINE)
}

/// The dark outline of the game's glyphs
const INK: FloatColor4 = FloatColor4::from_rgba(0.16, 0.1, 0.09, 1.0);

/// Glyph `glyph` of the `key` atlas `keys` over `rect`, as the active device shows it
pub fn draw(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    transform: Mat4,
    keys: &GpuTexture,
    glyph: u8,
    rect: [f32; 4],
    color: FloatColor4,
) {
    let drawn = resolve(glyph, style(), &bindings::table());
    draw_drawn(pass, builder, transform, keys, drawn, rect, color);
}

/// A pad input in a pad style (the Controls screen); false when it has no glyph
pub fn draw_pad(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    transform: Mat4,
    keys: &GpuTexture,
    style: Style,
    input: Input,
    rect: [f32; 4],
    color: FloatColor4,
) -> bool {
    let Some(pad) = PadGlyph::of_input(input) else {
        return false;
    };
    let drawn = match style {
        Style::Switch => match pad.switch_index() {
            Some(index) => Drawn::Switch(index),
            None => Drawn::Painted(painted(Style::Xbox, pad)),
        },
        _ => Drawn::Painted(painted(style, pad)),
    };
    draw_drawn(pass, builder, transform, keys, drawn, rect, color);
    true
}

fn draw_drawn(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    transform: Mat4,
    keys: &GpuTexture,
    drawn: Drawn,
    rect: [f32; 4],
    color: FloatColor4,
) {
    let sprite = |texture, uv| {
        let mut sprite = Sprite::new(texture, rect).uv(uv);
        sprite.color = color;
        sprite
    };
    let cell_uv = |cell: usize| {
        let (column, row) = (
            (cell as u32 % COLUMNS) as f32,
            (cell as u32 / COLUMNS) as f32,
        );
        let (w, h) = (1.0 / COLUMNS as f32, 1.0 / ROWS as f32);
        [column * w, row * h, (column + 1.0) * w, (row + 1.0) * h]
    };
    match drawn {
        Drawn::Switch(glyph) => {
            let (u, v) = ((glyph & 7) as f32 * 0.125, (glyph >> 3) as f32 * 0.5);
            sprite(keys, [u, v, u + 0.125, v + 0.5]).draw(pass, builder, transform);
        }
        Drawn::Painted(cell) => {
            if let Some(atlas) = ATLAS.get() {
                sprite(atlas, cell_uv(cell)).draw(pass, builder, transform);
            }
        }
        Drawn::Key(name) => {
            let Some(atlas) = ATLAS.get() else { return };
            KEY_TEXTS.with_borrow(|texts| {
                let text = texts.get(&name);
                // a long name ("Space", "BkSp") widens its cap to the left instead of shrinking out of sight: the
                // text keeps at least half the cap's height
                let mut rect = rect;
                if let Some(text) = text {
                    let size = text.size();
                    let height = rect[3] - rect[1];
                    let needed = size.x * (height * 0.5 / size.y) / 0.84;
                    rect[0] = rect[0].min(rect[2] - needed);
                }
                let mut cap = Sprite::new(atlas, rect).uv(cell_uv(KEY_CAP));
                cap.color = color;
                cap.draw(pass, builder, transform);
                if let Some(text) = text {
                    draw_key_name(pass, builder, transform, text, rect, color);
                }
            });
        }
    }
}

/// A key's name centred on its cap, shrunk to fit
pub fn draw_key_name(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    transform: Mat4,
    text: &TextBlock,
    [x0, y0, x1, y1]: [f32; 4],
    color: FloatColor4,
) {
    let size = text.size();
    let (w, h) = (x1 - x0, y1 - y0);
    let scale = (w * 0.84 / size.x).min(h * 0.9 / size.y).min(h / 48.0);
    let center = vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let at = center - size * scale / 2.0;
    text.render(
        pass,
        builder,
        transform
            * Mat4::from_translation(at.extend(0.0))
            // the block's size includes its outline and it offsets itself by it when drawn
            * Mat4::from_scale(vec3(scale, scale, 1.0)),
        FloatColor4::from_rgba(color.r, color.g, color.b, color.a),
        INK.with_alpha(color.a),
    );
}

// ---- painting ----

type Rgba = [u8; 4];
const OUT: Rgba = [42, 26, 23, 255];
const GREY: Rgba = [93, 93, 93, 255];
const LIGHT: Rgba = [124, 124, 124, 255];
const DARK: Rgba = [56, 56, 56, 255];
const WHITE: Rgba = [255, 255, 255, 255];

/// One cell being painted: shapes are signed distances (negative inside) in the cell's 96 px, blended over
struct Cell<'a> {
    image: &'a mut RgbaImage,
    origin: (u32, u32),
}

impl Cell<'_> {
    fn paint(&mut self, color: Rgba, shape: impl Fn(Vec2) -> f32) {
        for y in 0..CELL {
            for x in 0..CELL {
                let p = vec2(x as f32 + 0.5, y as f32 + 0.5) * (96.0 / CELL as f32);
                let coverage = (0.5 - shape(p) * CELL as f32 / 96.0).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let pixel = self
                    .image
                    .get_pixel_mut(self.origin.0 + x, self.origin.1 + y);
                let a = coverage * color[3] as f32 / 255.0;
                let below = pixel.0[3] as f32 / 255.0;
                let out = a + below * (1.0 - a);
                for (channel, &value) in pixel.0.iter_mut().zip(&color).take(3) {
                    let mixed = (value as f32 * a + *channel as f32 * below * (1.0 - a)) / out;
                    *channel = mixed.round() as u8;
                }
                pixel.0[3] = (out * 255.0).round() as u8;
            }
        }
    }

    fn circle(&mut self, color: Rgba, r: f32) {
        self.paint(color, move |p| (p - vec2(48.0, 48.0)).length() - r);
    }

    fn rect(&mut self, color: Rgba, center: Vec2, half: Vec2, radius: f32) {
        self.paint(color, move |p| rounded_rect(p - center, half, radius));
    }

    fn strokes(&mut self, color: Rgba, lines: Vec<Vec<Vec2>>, width: f32) {
        self.paint(color, move |p| polyline_distance(&lines, p) - width / 2.0);
    }

    /// Letters as strokes, centred on `center`, `height` tall
    fn label(&mut self, color: Rgba, text: &str, center: Vec2, height: f32, width: f32) {
        self.strokes(color, letters(text, center, height), width);
    }

    fn polygon(&mut self, color: Rgba, points: Vec<Vec2>) {
        self.paint(color, move |p| convex_distance(&points, p));
    }
}

fn rounded_rect(p: Vec2, half: Vec2, radius: f32) -> f32 {
    let q = p.abs() - half + Vec2::splat(radius);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (p - a - ab * t).length()
}

fn polyline_distance(lines: &[Vec<Vec2>], p: Vec2) -> f32 {
    lines
        .iter()
        .flat_map(|line| line.windows(2))
        .map(|w| segment_distance(p, w[0], w[1]))
        .fold(f32::MAX, f32::min)
}

/// A convex polygon's signed distance, near enough at its edges for antialiasing
fn convex_distance(points: &[Vec2], p: Vec2) -> f32 {
    let centroid = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let edge = (b - a).normalize();
            let mut normal = vec2(-edge.y, edge.x);
            if (centroid - a).dot(normal) > 0.0 {
                normal = -normal;
            }
            (p - a).dot(normal)
        })
        .fold(f32::MIN, f32::max)
}

fn arc(center: Vec2, radius: Vec2, from: f32, to: f32) -> Vec<Vec2> {
    let steps = 16;
    (0..=steps)
        .map(|k| {
            let angle = (from + (to - from) * k as f32 / steps as f32).to_radians();
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// A letter's strokes in a unit box (y down)
fn letter(c: char) -> Vec<Vec<Vec2>> {
    let v = |x: f32, y: f32| vec2(x, y);
    let join = |mut a: Vec<Vec2>, b: Vec<Vec2>| {
        a.extend(b);
        a
    };
    match c {
        'A' => vec![
            vec![v(0.0, 1.0), v(0.5, 0.0), v(1.0, 1.0)],
            vec![v(0.22, 0.64), v(0.78, 0.64)],
        ],
        'B' => vec![
            vec![v(0.0, 0.0), v(0.0, 1.0)],
            join(
                join(
                    vec![v(0.0, 0.0)],
                    arc(v(0.55, 0.25), v(0.4, 0.25), -90.0, 90.0),
                ),
                vec![v(0.0, 0.5)],
            ),
            join(
                join(
                    vec![v(0.0, 0.5)],
                    arc(v(0.6, 0.75), v(0.4, 0.25), -90.0, 90.0),
                ),
                vec![v(0.0, 1.0)],
            ),
        ],
        'X' => vec![
            vec![v(0.0, 0.0), v(1.0, 1.0)],
            vec![v(1.0, 0.0), v(0.0, 1.0)],
        ],
        'Y' => vec![
            vec![v(0.0, 0.0), v(0.5, 0.5), v(1.0, 0.0)],
            vec![v(0.5, 0.5), v(0.5, 1.0)],
        ],
        'L' => vec![vec![v(0.05, 0.0), v(0.05, 1.0), v(0.95, 1.0)]],
        'R' => vec![
            join(
                join(
                    vec![v(0.0, 1.0), v(0.0, 0.0)],
                    arc(v(0.55, 0.26), v(0.4, 0.26), -90.0, 90.0),
                ),
                vec![v(0.0, 0.52)],
            ),
            vec![v(0.45, 0.52), v(1.0, 1.0)],
        ],
        'T' => vec![
            vec![v(0.0, 0.0), v(1.0, 0.0)],
            vec![v(0.5, 0.0), v(0.5, 1.0)],
        ],
        'S' => vec![join(
            arc(v(0.5, 0.25), v(0.45, 0.25), -15.0, -270.0),
            arc(v(0.5, 0.75), v(0.47, 0.25), -90.0, 165.0),
        )],
        '1' => vec![vec![v(0.2, 0.22), v(0.6, 0.0), v(0.6, 1.0)]],
        '2' => vec![join(
            arc(v(0.5, 0.3), v(0.45, 0.3), 200.0, 380.0),
            vec![v(0.0, 1.0), v(1.0, 1.0)],
        )],
        '3' => vec![join(
            arc(v(0.5, 0.26), v(0.42, 0.26), 205.0, 450.0),
            arc(v(0.5, 0.76), v(0.46, 0.24), -90.0, 155.0),
        )],
        _ => Vec::new(),
    }
}

/// A word's strokes, letters 0.62 of the height wide with 0.25 between, centred on `center`
fn letters(text: &str, center: Vec2, height: f32) -> Vec<Vec<Vec2>> {
    let (w, gap) = (height * 0.62, height * 0.25);
    let n = text.chars().count() as f32;
    let total = n * w + (n - 1.0) * gap;
    let mut lines = Vec::new();
    for (k, c) in text.chars().enumerate() {
        let origin = center + vec2(-total / 2.0 + k as f32 * (w + gap), -height / 2.0);
        for line in letter(c) {
            lines.push(
                line.into_iter()
                    .map(|p| origin + p * vec2(w, height))
                    .collect(),
            );
        }
    }
    lines
}

/// The game's glyphs are grey with a thick dark outline and white lettering; these follow them
fn paint_pad(cell: &mut Cell, style: Style, glyph: PadGlyph) {
    use PadGlyph::*;
    let xbox = style == Style::Xbox;
    let c = vec2(48.0, 48.0);
    match glyph {
        South | East | West | North => {
            cell.circle(OUT, 44.0);
            if xbox {
                let (fill, letter) = match glyph {
                    South => ([86, 160, 62, 255], "A"),
                    East => ([196, 64, 52, 255], "B"),
                    West => ([52, 112, 196, 255], "X"),
                    _ => ([214, 168, 40, 255], "Y"),
                };
                cell.circle(fill, 37.0);
                cell.label(WHITE, letter, c, 38.0, 8.5);
            } else {
                cell.circle(GREY, 37.0);
                match glyph {
                    // ✕ ○ □ △ in the shapes' customary colours
                    South => cell.strokes(
                        [130, 168, 236, 255],
                        vec![
                            vec![vec2(32.0, 32.0), vec2(64.0, 64.0)],
                            vec![vec2(64.0, 32.0), vec2(32.0, 64.0)],
                        ],
                        8.0,
                    ),
                    East => cell.paint([238, 98, 98, 255], |p| {
                        ((p - c).length() - 17.0).abs() - 4.0
                    }),
                    West => cell.paint([228, 132, 200, 255], |p| {
                        rounded_rect(p - c, Vec2::splat(15.0), 2.0).abs() - 4.0
                    }),
                    _ => cell.strokes(
                        [72, 200, 160, 255],
                        vec![vec![
                            vec2(48.0, 28.0),
                            vec2(68.0, 62.0),
                            vec2(28.0, 62.0),
                            vec2(48.0, 28.0),
                        ]],
                        8.0,
                    ),
                }
            }
        }
        L1 | R1 | L2 | R2 => {
            let text = match (xbox, glyph) {
                (true, L1) => "LB",
                (true, R1) => "RB",
                (true, L2) => "LT",
                (true, R2) => "RT",
                (false, L1) => "L1",
                (false, R1) => "R1",
                (false, L2) => "L2",
                _ => "R2",
            };
            cell.rect(OUT, c, vec2(44.0, 25.0), 14.0);
            cell.rect(GREY, c, vec2(38.0, 19.0), 9.0);
            cell.label(WHITE, text, c, 24.0, 6.0);
        }
        L1R1 => {
            let (top, bottom) = if xbox { ("LB", "RB") } else { ("L1", "R1") };
            cell.rect(OUT, c, vec2(36.0, 44.0), 10.0);
            cell.rect(GREY, c, vec2(30.0, 38.0), 5.0);
            cell.rect(OUT, c, vec2(32.0, 3.0), 0.0);
            cell.label(WHITE, top, vec2(48.0, 28.0), 20.0, 5.0);
            cell.label(WHITE, bottom, vec2(48.0, 68.0), 20.0, 5.0);
        }
        Start | Select => {
            if xbox {
                cell.circle(OUT, 32.0);
                cell.circle(GREY, 26.0);
                if glyph == Start {
                    // three bars
                    let bars = [38.0, 48.0, 58.0]
                        .map(|y| vec![vec2(36.0, y), vec2(60.0, y)])
                        .to_vec();
                    cell.strokes(WHITE, bars, 4.5);
                } else {
                    // two overlapping windows
                    cell.paint(WHITE, |p| {
                        rounded_rect(p - vec2(44.0, 44.0), vec2(9.0, 7.0), 1.5).abs() - 2.0
                    });
                    cell.paint(WHITE, |p| {
                        rounded_rect(p - vec2(53.0, 53.0), vec2(9.0, 7.0), 1.5)
                    });
                }
            } else {
                // the small pill buttons beside the touch pad
                cell.rect(OUT, c, vec2(22.0, 36.0), 22.0);
                cell.rect(GREY, c, vec2(16.0, 30.0), 16.0);
                if glyph == Start {
                    let bars = [38.0, 48.0, 58.0]
                        .map(|y| vec![vec2(40.0, y), vec2(56.0, y)])
                        .to_vec();
                    cell.strokes(WHITE, bars, 4.0);
                } else {
                    cell.strokes(
                        WHITE,
                        vec![
                            vec![vec2(48.0, 62.0), vec2(48.0, 34.0)],
                            vec![vec2(39.0, 43.0), vec2(48.0, 34.0), vec2(57.0, 43.0)],
                        ],
                        4.5,
                    );
                }
            }
        }
        DPad | DPadUpDown | DPadLeftRight | DPadUp | DPadDown => {
            cell.rect(OUT, c, vec2(16.0, 44.0), 7.0);
            cell.rect(OUT, c, vec2(44.0, 16.0), 7.0);
            let lit = |arm: usize| match glyph {
                DPad => GREY,
                DPadUpDown if arm < 2 => LIGHT,
                DPadLeftRight if arm >= 2 => LIGHT,
                DPadUp if arm == 0 => LIGHT,
                DPadDown if arm == 1 => LIGHT,
                _ => DARK,
            };
            // up, down, left, right
            cell.rect(lit(0), vec2(48.0, 28.0), vec2(10.0, 18.0), 3.0);
            cell.rect(lit(1), vec2(48.0, 68.0), vec2(10.0, 18.0), 3.0);
            cell.rect(lit(2), vec2(28.0, 48.0), vec2(18.0, 10.0), 3.0);
            cell.rect(lit(3), vec2(68.0, 48.0), vec2(18.0, 10.0), 3.0);
            cell.rect(
                if glyph == DPad { GREY } else { DARK },
                c,
                Vec2::splat(10.0),
                0.0,
            );
        }
        L3 | R3 => {
            let text = match (xbox, glyph) {
                (true, L3) => "LS",
                (true, _) => "RS",
                (false, L3) => "L3",
                _ => "R3",
            };
            cell.circle(OUT, 44.0);
            cell.circle(GREY, 37.0);
            cell.paint(LIGHT, |p| ((p - c).length() - 29.0).abs() - 2.0);
            cell.label(WHITE, text, c, 24.0, 6.0);
        }
    }
}

fn key_cap(cell: &mut Cell) {
    let c = vec2(48.0, 48.0);
    cell.rect(OUT, c, Vec2::splat(43.0), 12.0);
    cell.rect(GREY, c, Vec2::splat(37.0), 8.0);
}

/// An arrow key cap: `dirs` up, down, left, right
fn arrow_cap(cell: &mut Cell, dirs: [bool; 4]) {
    key_cap(cell);
    let all = dirs.iter().filter(|&&d| d).count();
    let (reach, size) = if all == 1 { (0.0, 16.0) } else { (17.0, 10.0) };
    for (k, &on) in dirs.iter().enumerate() {
        if !on {
            continue;
        }
        let dir = [
            vec2(0.0, -1.0),
            vec2(0.0, 1.0),
            vec2(-1.0, 0.0),
            vec2(1.0, 0.0),
        ][k];
        let side = vec2(-dir.y, dir.x);
        let tip = vec2(48.0, 48.0) + dir * (reach + size);
        let base = tip - dir * size * 1.6;
        cell.polygon(WHITE, vec![tip, base + side * size, base - side * size]);
    }
}

fn paint_atlas() -> RgbaImage {
    let mut image = RgbaImage::new(CELL * COLUMNS, CELL * ROWS);
    let mut cell_at = |index: usize, paint: &dyn Fn(&mut Cell)| {
        let origin = (
            (index as u32 % COLUMNS) * CELL,
            (index as u32 / COLUMNS) * CELL,
        );
        paint(&mut Cell {
            image: &mut image,
            origin,
        })
    };
    for style in [Style::Xbox, Style::PlayStation] {
        for glyph in ALL_PADS {
            cell_at(painted(style, glyph), &|cell| paint_pad(cell, style, glyph));
        }
    }
    cell_at(KEY_CAP, &key_cap);
    for (k, dirs) in [
        [true; 4],
        [true, true, false, false],
        [false, false, true, true],
        [true, false, false, false],
        [false, true, false, false],
    ]
    .into_iter()
    .enumerate()
    {
        cell_at(ARROW_CAPS + k, &|cell| arrow_cap(cell, dirs));
    }
    image
}

const ALL_PADS: [PadGlyph; PAD_GLYPHS] = {
    use PadGlyph::*;
    [
        South,
        East,
        West,
        North,
        L1,
        R1,
        L2,
        R2,
        Start,
        Select,
        DPad,
        DPadUpDown,
        DPadLeftRight,
        DPadUp,
        DPadDown,
        L1R1,
        L3,
        R3,
    ]
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_follow_the_style_and_the_bindings() {
        let table = bindings::defaults();
        // A (confirm, the east button): Switch A, Xbox B, PlayStation circle, the keyboard's Space
        assert_eq!(resolve(3, Style::Switch, &table), Drawn::Switch(3));
        assert_eq!(
            resolve(3, Style::Xbox, &table),
            Drawn::Painted(PadGlyph::East as usize)
        );
        assert_eq!(
            resolve(3, Style::PlayStation, &table),
            Drawn::Painted(PAD_GLYPHS + PadGlyph::East as usize)
        );
        assert_eq!(
            resolve(3, Style::Keyboard, &table),
            Drawn::Key("Space".into())
        );
        assert_eq!(
            resolve(7, Style::Keyboard, &table),
            Drawn::Key("A/S".into())
        );
        assert_eq!(
            resolve(7, Style::Xbox, &table),
            Drawn::Painted(PadGlyph::L1R1 as usize)
        );
        assert_eq!(
            resolve(2, Style::Keyboard, &table),
            Drawn::Painted(ARROW_CAPS + 2)
        );
        // every Switch glyph maps back to itself
        for glyph in 0..16 {
            assert_eq!(resolve(glyph, Style::Switch, &table), Drawn::Switch(glyph));
        }
        // the system menu (Y, west) moved to the south button and M
        let mut table = table;
        table[AppAction::ButtonY] = vec![
            Input::Key(shin_input::inputs::KeyCode::KeyM),
            Input::Pad(GamepadButton::B),
        ];
        assert_eq!(resolve(6, Style::Switch, &table), Drawn::Switch(4));
        assert_eq!(resolve(6, Style::Keyboard, &table), Drawn::Key("M".into()));
    }

    #[test]
    fn atlas_cells_are_painted() {
        let image = paint_atlas();
        let painted_cell = |index: usize| {
            let (x0, y0) = (
                (index as u32 % COLUMNS) * CELL,
                (index as u32 / COLUMNS) * CELL,
            );
            (0..CELL).any(|y| (0..CELL).any(|x| image.get_pixel(x0 + x, y0 + y).0[3] > 0))
        };
        for index in (0..2 * PAD_GLYPHS).chain(KEY_CAP..ARROW_CAPS + 5) {
            assert!(painted_cell(index), "cell {index} is empty");
        }
        if let Ok(path) = std::env::var("SHIN_PROMPTS_PNG") {
            image.save(path).unwrap();
        }
    }
}
