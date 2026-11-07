//! The culprit game ([`LayerType::Quiz`](shin_core::vm::command::types::LayerType::Quiz) kind 2): a menu of screens
//! where the player re-reads the story, reads the rules and hints, browses the "purple" statements and finally names
//! the culprit(s).
//!
//! Re-implemented from the behaviour of `shin::MurdererLayer` in the Switch v1.0.3 executable (constructor
//! `0x710009a8f0`, update `0x710009b090`, offscreen render `0x710009b1b0`, draw `0x710009b400`, result `0x710009b6e0`)
//! and its screens (`shin::MurdererLayer::{TopMenu, ReplayMenu, Rule, HintMenu, Check, Select}`).
//!
//! Each screen draws into a texture in two passes (background, then foreground) in 1920 x 1080 top-left space. A
//! screen hands over by snapshotting itself (`beginTransition` `0x710008cac0`) and naming the next screen, which
//! replaces it at the start of the next update; the engine's wipers composite the snapshot with the live new screen
//! (a crossfade, or the glass shatter when the culprit is named). No input reaches a screen during a transition.
//!
//! The answer is set by the screen the player leaves the game from: a replayed episode (0-5), a hint (6-26) or the
//! culprit choice (27 and up). The script answers each with a scene and loads the game again.

mod check;
mod draw;
mod hint_menu;
mod replay_menu;
mod rule;
mod select;
mod top_menu;
mod tree;

use std::{fmt::Debug, sync::Arc};

use glam::{Mat4, Vec2, vec3};
use shin_core::{
    primitives::color::UnormColor,
    time::{Ticks, Tweener},
};
use shin_render::{
    PassKind, RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{
        RenderClone,
        texture::{DepthStencilTarget, TextureTarget},
    },
};
use shin_render::resize::CanvasSize;

use self::{
    check::Check, draw::Drawer, hint_menu::HintMenu, replay_menu::ReplayMenu, rule::Rule,
    select::Select, top_menu::TopMenu,
};
use crate::{
    adv::assets::AdvFonts,
    asset::texture_archive::NamedTextures,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
        user::minigame::{self, Minigame, MinigameInput, PointerTarget},
    },
    layer::text_block::TextBlock,
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::{AnyWiper, DefaultWiper, GlassWiper, Wiper},
};

/// `/murderer.txa`, the scroll bar texture of `/systex.txa`, and the Check screen's busts (`/chars/m_<name>.txa`,
/// which the original loads when a person is picked)
pub struct MurdererAssets {
    pub textures: Arc<NamedTextures>,
    pub fonts: AdvFonts,
    pub system: Arc<NamedTextures>,
    pub busts: Vec<(&'static str, Arc<NamedTextures>)>,
}

impl MurdererAssets {
    /// The bust archives to load: every living person's
    pub fn bust_names() -> impl Iterator<Item = &'static str> {
        tree::BUSTS.into_iter().flatten()
    }
}

/// What every screen can reach: the layer's parameter (the script's `$v36`), the answer, sounds and the blink phase
#[derive(Debug, Clone)]
struct Shared {
    param: i32,
    result: Option<i32>,
    sounds: Vec<&'static str>,
    /// Advances by 1/60 per frame and wraps (`+0x5c`)
    blink_phase: f32,
    /// The height of the text the screen shows (the Check screen's testimony or evidence), once laid out
    text_height: f32,
    transition_running: bool,
}

/// A screen asking to be replaced: the wiper that composites its snapshot with what comes next, and the next screen
/// (none when the same screen changes its own look, as the top menu does before the culprit choice)
struct Handover {
    wiper: AnyWiper,
    next: Option<Screen>,
    /// The screen as it was when the original took its snapshot, when the screen changed itself after that
    snapshot_of: Option<Screen>,
}

impl Handover {
    fn crossfade(frames: f32, next: Option<Screen>) -> Self {
        Self {
            wiper: DefaultWiper::new(Ticks::from_f32(frames)).into(),
            next,
            snapshot_of: None,
        }
    }

    fn glass(frames: f32, next: Screen) -> Self {
        Self {
            wiper: GlassWiper::new(Ticks::from_f32(frames)).into(),
            next: Some(next),
            snapshot_of: None,
        }
    }
}

/// Moves the highlight from item `old` to item `new`, as every list does: out in 0.1 s, in over 0.25 s
fn move_highlight<T>(
    items: &mut [T],
    old: usize,
    new: usize,
    highlight: impl Fn(&mut T) -> &mut Tweener,
) {
    // at 0.1 and 0.25 per frame (tween flag 0x40)
    minigame::tween_now(highlight(&mut items[old]), 0.0, 0.1);
    minigame::tween_now(highlight(&mut items[new]), 1.0, 0.25);
}

pub const SOUND_CANCEL: &str = "cancel";
/// The back button every sub-screen has at the top right (160 x 84)
const BACK_RECT: [f32; 4] = [1640.0, 32.0, 1800.0, 116.0];

/// What the pointer can be over in a screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Back,
    Item(usize),
    /// A chapter header of the Check screen's evidence list
    Header(usize),
}

#[derive(Clone)]
enum Screen {
    Top(TopMenu),
    Replay(ReplayMenu),
    Rule(Rule),
    Hint(HintMenu),
    Check(Check),
    Select(Select),
}

macro_rules! each_screen {
    ($screen:expr, $s:ident => $body:expr) => {
        match $screen {
            Screen::Top($s) => $body,
            Screen::Replay($s) => $body,
            Screen::Rule($s) => $body,
            Screen::Hint($s) => $body,
            Screen::Check($s) => $body,
            Screen::Select($s) => $body,
        }
    };
}

impl Screen {
    fn update(&mut self, delta: Ticks, shared: &mut Shared) -> Option<Handover> {
        each_screen!(self, s => s.update(delta, shared))
    }

    fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        each_screen!(self, s => s.handle_input(input, shared))
    }

    /// `background`: the first pass (pass flag 1); the foreground follows in a second call
    fn draw(&self, drawer: &mut Drawer, shared: &Shared, background: bool) {
        each_screen!(self, s => s.draw(drawer, shared, background))
    }

    /// Debug fast-forwarding cannot wait for a player: each screen heads for the culprit choice and picks there
    fn auto_pick(&mut self, shared: &mut Shared) {
        each_screen!(self, s => s.auto_pick(shared))
    }

    /// What is under `position`, only while the screen takes input (each screen's slot 10)
    fn pointer_hit(&self, position: Vec2) -> Option<Target> {
        each_screen!(self, s => s.pointer_hit(position))
    }

    /// Slot 12: lights the target and, for items, moves the cursor there
    fn pointer_press(&mut self, target: Target) {
        each_screen!(self, s => s.pointer_press(target))
    }

    /// Slot 13: activates the target, as confirm (or "back") does
    fn pointer_release(&mut self, target: Target, shared: &mut Shared) {
        each_screen!(self, s => s.pointer_release(target, shared))
    }

    /// Slots 14/15/17: drops the press's light
    fn pointer_cancel(&mut self, target: Target) {
        each_screen!(self, s => s.pointer_cancel(target))
    }

    /// The text the screen shows, if any (the Check screen's text object)
    fn text(&self) -> Option<&str> {
        match self {
            Screen::Check(screen) => Some(screen.text()),
            _ => None,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Screen::Top(_) => "TopMenu",
            Screen::Replay(_) => "ReplayMenu",
            Screen::Rule(_) => "Rule",
            Screen::Hint(_) => "HintMenu",
            Screen::Check(_) => "Check",
            Screen::Select(_) => "Select",
        }
    }
}

#[derive(RenderClone)]
pub struct MurdererLayerImpl {
    assets: Arc<MurdererAssets>,
    shared: Shared,
    screen: Screen,
    /// Replaces `screen` at the start of the next update
    pending: Option<Screen>,
    #[render_clone(needs_render)]
    transition: Option<AnyWiper>,
    /// The current screen (or `snapshot_of`) must be rendered into `snapshot` first: a transition has just begun
    snapshot_pending: bool,
    snapshot_of: Option<Screen>,
    #[render_clone(needs_render)]
    screen_texture: Option<RenderTexture>,
    #[render_clone(needs_render)]
    snapshot: Option<RenderTexture>,
    #[render_clone(needs_render)]
    text: TextCache,
    hovered: Option<Target>,
    pressed: Option<Target>,
}

impl MurdererLayerImpl {
    pub fn new(assets: Arc<MurdererAssets>, param: i32) -> Self {
        Self {
            assets,
            shared: Shared {
                param,
                result: None,
                sounds: Vec::new(),
                blink_phase: 0.0,
                text_height: 0.0,
                transition_running: false,
            },
            screen: Screen::Top(TopMenu::new(0)),
            pending: None,
            transition: None,
            snapshot_pending: false,
            snapshot_of: None,
            screen_texture: None,
            snapshot: None,
            text: TextCache::default(),
            hovered: None,
            pressed: None,
        }
    }

    /// Clears `target` to opaque black and draws the screen's two passes into it (`0x710009b1b0`)
    fn render_screen(
        &self,
        screen: &Screen,
        context: &mut PreRenderContext,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: Mat4,
    ) {
        let target_size = context.resize_source.handle::<CanvasSize>().get();
        let target_size = (target_size.width, target_size.height);
        let text = self.text.0.as_ref().map(|(_, block)| block);
        let mut pass = context.begin_pass(target, Some(depth_stencil), "MurdererLayer/screen");
        pass.clear(Some(UnormColor::BLACK), Some(0), None);
        let mut drawer = Drawer::new(&mut pass, &self.assets, transform, target_size, text);
        screen.draw(&mut drawer, &self.shared, true);
        screen.draw(&mut drawer, &self.shared, false);
    }
}

/// `MurdererOverlayLayer` slots 18-25 (`0x710009ba20`-`0x710009bb10`): forwarded to the current screen; the hit test
/// is skipped while a transition runs
impl PointerTarget for MurdererLayerImpl {
    fn pointer_hit(&mut self, position: Vec2) -> bool {
        self.hovered = None;
        if self.transition.is_none() && self.pending.is_none() && self.shared.result.is_none() {
            self.hovered = self.screen.pointer_hit(position);
        }
        self.hovered.is_some()
    }

    fn pointer_press(&mut self) {
        if let Some(target) = self.hovered {
            self.screen.pointer_press(target);
            self.pressed = Some(target);
        }
    }

    fn pointer_release(&mut self) {
        self.pressed = None;
        if let Some(target) = self.hovered.take() {
            self.screen.pointer_release(target, &mut self.shared);
        }
    }

    fn pointer_cancel(&mut self) {
        if let Some(target) = self.pressed.take() {
            self.screen.pointer_cancel(target);
        }
    }
}

impl Minigame for MurdererLayerImpl {
    fn handle_input(&mut self, input: &MinigameInput) {
        // no input reaches a screen while a transition runs (`0x710009b9f0`)
        if self.transition.is_none() && self.shared.result.is_none() {
            self.screen.handle_input(input, &mut self.shared);
        }
    }

    fn result(&self) -> Option<i32> {
        self.shared.result
    }

    fn is_interactive(&self) -> bool {
        self.shared.result.is_none()
    }

    fn auto_pick(&mut self) {
        if self.transition.is_none() && self.pending.is_none() && self.shared.result.is_none() {
            self.screen.auto_pick(&mut self.shared);
        }
    }

    fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.shared.sounds)
    }
}

pub type MurdererLayer = NewDrawableLayerWrapper<MurdererLayerImpl>;

impl MurdererLayer {
    pub fn new(assets: Arc<MurdererAssets>, param: i32) -> Self {
        Self::from_inner(MurdererLayerImpl::new(assets, param))
    }
}

impl NewDrawableLayerNeedsSeparatePass for MurdererLayerImpl {
    /// The screens are always drawn through textures, as in the original
    fn needs_separate_pass(&self, _props: &LayerProperties) -> bool {
        true
    }
}

impl NewDrawableLayer for MurdererLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        // the layer's transform applies inside the screen texture, around 1920x1080 top-left space (`0x710009b240`)
        let screen_transform = props
            .get_composed_transform_params(transform)
            .compute_final_transform()
            * Mat4::from_translation(vec3(-960.0, -540.0, 0.0));

        // lay out the text the screen shows when it changes (`0x7100094480`)
        let wanted = self.screen.text().unwrap_or_default();
        if self.text.0.as_ref().is_none_or(|(text, _)| text != wanted) {
            let block = (!wanted.is_empty()).then(|| {
                TextBlock::new(
                    context,
                    &self.assets.fonts.system_font,
                    wanted,
                    check::text_layout(),
                    crate::layer::text_block::OUTLINE,
                )
            });
            self.shared.text_height = block.as_ref().map_or(0.0, |block| block.size().y);
            self.text.0 = block.map(|block| (wanted.to_string(), block));
        }

        if self.snapshot_pending {
            // the screen that started the transition is still current: the wiper starts from this picture
            self.snapshot_pending = false;
            let mut snapshot = self.snapshot.take().unwrap_or_else(|| {
                context.new_render_texture("MurdererLayer/snapshot".to_string())
            });
            let snapshot_of = self.snapshot_of.take();
            let screen = snapshot_of.as_ref().unwrap_or(&self.screen);
            self.render_screen(
                screen,
                context,
                snapshot.as_texture_target(),
                depth_stencil,
                screen_transform,
            );
            self.snapshot = Some(snapshot);
        }

        let mut screen_texture = self
            .screen_texture
            .take()
            .unwrap_or_else(|| context.new_render_texture("MurdererLayer/screen".to_string()));
        self.render_screen(
            &self.screen,
            context,
            screen_texture.as_texture_target(),
            depth_stencil,
            screen_transform,
        );

        let mut pass = context.begin_pass(target, Some(depth_stencil), "MurdererLayer/composite");
        pass.clear(Some(UnormColor(0)), Some(0), None);
        let builder = RenderRequestBuilder::new();
        match (&self.transition, &self.snapshot) {
            (Some(wiper), Some(snapshot)) => wiper.render(
                &mut pass,
                builder,
                snapshot.as_texture_source(),
                screen_texture.as_texture_source(),
            ),
            _ => draw::blit(&mut pass, builder, screen_texture.as_texture_source()),
        }
        drop(pass);
        self.screen_texture = Some(screen_texture);
        PassKind::Transparent
    }

    fn render_drawable_direct(
        &self,
        _pass: &mut RenderPass,
        _transform: &TransformParams,
        _drawable: &DrawableParams,
        _clip: &DrawableClipParams,
        _stencil_ref: u8,
        _pass_kind: PassKind,
    ) {
        // always drawn through `render_drawable_indirect`
    }

    fn update_from_props(&mut self, _props: &LayerProperties) {}
}

impl NewDrawableLayerFastForward for MurdererLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for MurdererLayerImpl {
    /// `0x710009b090`: blink phase, pending screen swap, screen update (also during a transition), transition
    fn update(&mut self, context: &AdvUpdateContext) {
        let delta = context.delta_ticks;
        self.shared.blink_phase = (self.shared.blink_phase + delta.as_f32() / 60.0).fract();
        if let Some(next) = self.pending.take() {
            self.screen = next;
            self.hovered = None;
            self.pressed = None;
        }
        self.shared.transition_running = self.transition.is_some();
        if let Some(handover) = self.screen.update(delta, &mut self.shared) {
            self.transition = Some(handover.wiper);
            self.snapshot_pending = true;
            self.snapshot_of = handover.snapshot_of;
            if handover.next.is_some() {
                self.pending = handover.next;
            }
        }
        if let Some(wiper) = &mut self.transition {
            wiper.update(context);
            if !wiper.is_running() {
                self.transition = None;
            }
        }
    }
}

impl Debug for MurdererLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MurdererLayer")
            .field("param", &self.shared.param)
            .field("screen", &self.screen.name())
            .field("result", &self.shared.result)
            .finish()
    }
}

/// The laid-out text of the current screen and the string it was laid out from; not cloned with the layer (it is
/// laid out again)
#[derive(Default)]
struct TextCache(Option<(String, TextBlock)>);

impl RenderClone for TextCache {
    fn render_clone(&self, _ctx: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        Self(None)
    }
}
