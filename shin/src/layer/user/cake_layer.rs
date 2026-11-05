//! The cake game ([`LayerType::Quiz`](shin_core::vm::command::types::LayerType::Quiz) kind 1): a pointing finger the
//! player moves over a whole cake to pick one of its 15 slices.
//!
//! Re-implemented from the behaviour of `shin::CakeLayer` in the Switch v1.0.3 executable (constructor
//! `0x7100043f40`, update `0x7100044a50`, draw `0x7100044af0`, hit test `0x7100044fe0`, result `0x7100045250`) and its
//! input handler `shin::CakeLayer::CakeOverlayLayer` (keys `0x7100045da0`). The slice table is filled by a static
//! initializer at `0x7100046c40`.
//!
//! The finger starts at (1440, 900) and moves 8 px per frame while a direction is held, kept within
//! x 28-1863, y 38-1015. The slice under its tip glows in (its sprite drawn over the cake); confirm on a slice flashes
//! it and fades the finger and the "choose a cake" banner, and the game answers with the slice's index; confirm
//! elsewhere plays `error`.

use std::{fmt::Debug, sync::Arc};

use anyhow::Context as _;
use glam::{Vec2, vec2};
use image::RgbaImage;
use shin_core::time::Tweener;
use shin_render::{
    PassKind, gpu_texture::GpuTexture, render_pass::RenderPass, shaders::types::RenderClone,
};

use crate::{
    asset::system::{Asset, AssetDataAccessor, AssetLoadContext},
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::minigame::{
            self, Minigame, MinigameInput, PointerTarget, SOUND_DECIDE, SOUND_ERROR, Sprite,
            from_top_left, seconds,
        },
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

const SLICES: usize = 15;
/// Where each slice's sprite sits, relative to the cake's left edge at x 360 (`0x710022cbd8`, 15 x
/// (name, x, y, w, h)); the sprites are named "00" to "14"
const SLICE_RECTS: [[f32; 4]; SLICES] = [
    [384.0, 336.0, 256.0, 592.0],
    [240.0, 336.0, 384.0, 544.0],
    [160.0, 320.0, 464.0, 464.0],
    [144.0, 272.0, 480.0, 384.0],
    [176.0, 176.0, 448.0, 208.0],
    [256.0, 80.0, 368.0, 304.0],
    [368.0, 32.0, 256.0, 336.0],
    [496.0, 16.0, 208.0, 368.0],
    [576.0, 32.0, 256.0, 352.0],
    [576.0, 96.0, 368.0, 288.0],
    [592.0, 176.0, 432.0, 208.0],
    // the texture is 480 wide; the original squeezes it into the table's 432
    [576.0, 288.0, 432.0, 368.0],
    [576.0, 320.0, 464.0, 464.0],
    [576.0, 336.0, 384.0, 544.0],
    [576.0, 336.0, 240.0, 576.0],
];
const CAKE_LEFT: f32 = 360.0;
const CAKE_RECT: [f32; 4] = [360.0, 0.0, 1560.0, 1080.0];
const BANNER_RECT: [f32; 4] = [616.0, 920.0, 1304.0, 1016.0];
const FINGER_OFFSET: f32 = 16.0;
const FINGER_SIZE: f32 = 96.0;

const POINTER_START: Vec2 = vec2(1440.0, 900.0);
const POINTER_STEP: f32 = 8.0;
const POINTER_MIN: Vec2 = vec2(28.0, 38.0);
const POINTER_MAX: Vec2 = vec2(1863.0, 1015.0);

/// The hit map covers this square in 4 px cells
const HIT_ORIGIN: Vec2 = vec2(480.0, 0.0);
const HIT_SIZE: usize = 960;
const HIT_CELL: usize = 4;
const HIT_CELLS: usize = HIT_SIZE / HIT_CELL;

const HOVER_IN_SECONDS: f32 = 0.25;
const HOVER_OUT_SECONDS: f32 = 0.05;
const POINTER_FADE_SECONDS: f32 = 0.1;

/// `/cake.txa`, plus the slice hit map built from its sprites
pub struct CakeAssets {
    base: GpuTexture,
    banner: GpuTexture,
    finger: GpuTexture,
    slices: Vec<GpuTexture>,
    /// One cell per 4 x 4 px of [`HIT_ORIGIN`]'s square, row-major: the slice under it, or -1
    hit_map: Vec<i8>,
}

/// The slice under each 4 px cell, from the sprites' opaque pixels with later slices on top.
///
/// shortcut: the original reads a hand-made 4-bit map embedded in the executable (`0x71001675e9`, 240 x 240 cells)
/// that the port cannot ship; this reconstruction agrees with it on 98.5 % of the cells, differing only along slice
/// edges. Upgrade if the port ever reads tables from the player's own executable.
fn build_hit_map(slices: &[&RgbaImage]) -> Vec<i8> {
    let mut map = vec![-1i8; HIT_CELLS * HIT_CELLS];
    for (cell, hit) in map.iter_mut().enumerate() {
        let point = HIT_ORIGIN
            + vec2(
                ((cell % HIT_CELLS) * HIT_CELL) as f32,
                ((cell / HIT_CELLS) * HIT_CELL) as f32,
            )
            + HIT_CELL as f32 / 2.0;
        for (index, (image, [x, y, w, h])) in slices.iter().zip(SLICE_RECTS).enumerate() {
            let local = point - vec2(CAKE_LEFT + x, y);
            if local.x < 0.0 || local.y < 0.0 || local.x >= w || local.y >= h {
                continue;
            }
            // the sprite is stretched over the table's rect
            let px = (local.x / w * image.width() as f32) as u32;
            let py = (local.y / h * image.height() as f32) as u32;
            if image.get_pixel(px, py)[3] > 250 {
                *hit = index as i8;
            }
        }
    }
    map
}

impl Asset for CakeAssets {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        name: &str,
        data: AssetDataAccessor,
    ) -> anyhow::Result<Self> {
        let data = data.read_all().await;
        let context = context.clone();
        let label = format!("TXA[{name}]");
        shin_tasks::compute::spawn(move || {
            let archive = shin_core::format::texture_archive::read_texture_archive(&data)?;
            let image = |name: &str| {
                archive
                    .get_texture(name)
                    .with_context(|| format!("cake.txa has no {name}"))
            };
            let upload = |name: &str| -> anyhow::Result<GpuTexture> {
                Ok(GpuTexture::new_static_from_rgba_image(
                    &context.wgpu_device,
                    &context.wgpu_queue,
                    Some(&format!("{label}/{name}")),
                    image(name)?,
                ))
            };
            let names = (0..SLICES).map(|i| format!("{i:02}")).collect::<Vec<_>>();
            let slice_images = names
                .iter()
                .map(|name| image(name))
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(Self {
                base: upload("base")?,
                banner: upload("msg")?,
                finger: upload("finger")?,
                slices: names
                    .iter()
                    .map(|name| upload(name))
                    .collect::<anyhow::Result<_>>()?,
                hit_map: build_hit_map(&slice_images),
            })
        })
        .await
    }
}

impl CakeAssets {
    fn slice_at(&self, point: Vec2) -> Option<usize> {
        let local = point - HIT_ORIGIN;
        if local.x < 0.0
            || local.y < 0.0
            || local.x >= HIT_SIZE as f32
            || local.y >= HIT_SIZE as f32
        {
            return None;
        }
        let cell = local.y as usize / HIT_CELL * HIT_CELLS + local.x as usize / HIT_CELL;
        usize::try_from(self.hit_map[cell]).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Picked,
    Done,
}

#[derive(Clone)]
struct Slice {
    hover: Tweener,
    flash: Tweener,
}

#[derive(Clone, RenderClone)]
pub struct CakeLayerImpl {
    assets: Arc<CakeAssets>,
    slices: Vec<Slice>,
    pointer: Vec2,
    /// The finger's and the banner's alpha
    pointer_alpha: Tweener,
    hovered: Option<usize>,
    state: State,
    sounds: Vec<&'static str>,
    /// Where the pointer last hit, and the slice there (`+0xa8`)
    pointer_at: Vec2,
    pointer_slice: Option<usize>,
}

impl CakeLayerImpl {
    pub fn new(assets: Arc<CakeAssets>) -> Self {
        Self {
            assets,
            slices: vec![
                Slice {
                    hover: Tweener::new(0.0),
                    flash: Tweener::new(0.0),
                };
                SLICES
            ],
            pointer: POINTER_START,
            pointer_alpha: Tweener::new(1.0),
            hovered: None,
            state: State::Waiting,
            sounds: Vec::new(),
            pointer_at: POINTER_START,
            pointer_slice: None,
        }
    }

    fn hover(&mut self, target: Option<usize>) {
        if target == self.hovered {
            return;
        }
        if let Some(old) = self.hovered {
            self.slices[old]
                .hover
                .enqueue_now(0.0, seconds(HOVER_OUT_SECONDS));
        }
        if let Some(new) = target {
            self.slices[new]
                .hover
                .enqueue_now(1.0, seconds(HOVER_IN_SECONDS));
        }
        self.hovered = target;
    }

    fn pick(&mut self, slice: usize) {
        self.sounds.push(SOUND_DECIDE);
        minigame::flash(&mut self.slices[slice].flash);
        self.pointer_alpha
            .enqueue_now(0.0, seconds(POINTER_FADE_SECONDS));
        self.state = State::Picked;
    }
}

/// `CakeOverlayLayer` slots 18-25 (`0x7100046010`-`0x7100046430`): the whole screen answers the pointer; a press
/// brings the finger there
impl PointerTarget for CakeLayerImpl {
    fn pointer_hit(&mut self, position: Vec2) -> bool {
        if self.state != State::Waiting {
            return false;
        }
        self.pointer_at = position;
        self.pointer_slice = self.assets.slice_at(position);
        true
    }

    fn pointer_leave(&mut self) {
        self.pointer_slice = None;
    }

    fn pointer_press(&mut self) {
        self.pointer = self.pointer_at.clamp(POINTER_MIN, POINTER_MAX);
        self.hover(self.pointer_slice);
        if let Some(slice) = self.pointer_slice {
            minigame::raise(&mut self.slices[slice].flash);
        }
    }

    fn pointer_release(&mut self) {
        self.pointer = self.pointer_at.clamp(POINTER_MIN, POINTER_MAX);
        self.hover(self.pointer_slice);
        match self.pointer_slice {
            Some(slice) => self.pick(slice),
            None => self.sounds.push(SOUND_ERROR),
        }
    }

    fn pointer_cancel(&mut self) {
        if let Some(slice) = self.hovered {
            minigame::lower(&mut self.slices[slice].flash);
        }
        self.pointer_slice = None;
    }
}

impl Minigame for CakeLayerImpl {
    fn handle_input(&mut self, input: &MinigameInput) {
        if self.state != State::Waiting {
            return;
        }
        if input.confirm {
            match self.hovered {
                Some(slice) => self.pick(slice),
                None => self.sounds.push(SOUND_ERROR),
            }
            return;
        }
        let held = input.held;
        let step = |minus: bool, plus: bool| match (minus, plus) {
            (true, _) => -POINTER_STEP * input.frames,
            (false, true) => POINTER_STEP * input.frames,
            _ => 0.0,
        };
        self.pointer = (self.pointer + vec2(step(held.left, held.right), step(held.up, held.down)))
            .clamp(POINTER_MIN, POINTER_MAX);
        self.hover(self.assets.slice_at(self.pointer));
    }

    fn result(&self) -> Option<i32> {
        (self.state == State::Done).then(|| self.hovered.map_or(-1, |slice| slice as i32))
    }

    fn is_interactive(&self) -> bool {
        self.state == State::Waiting
    }

    fn auto_pick(&mut self) {
        if self.state == State::Waiting {
            self.hover(Some(0));
            self.pick(0);
        }
    }

    fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

pub type CakeLayer = NewDrawableLayerWrapper<CakeLayerImpl>;

impl CakeLayer {
    pub fn new(assets: Arc<CakeAssets>) -> Self {
        Self::from_inner(CakeLayerImpl::new(assets))
    }
}

impl NewDrawableLayerNeedsSeparatePass for CakeLayerImpl {}

impl NewDrawableLayer for CakeLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        &DrawableParams {
            color_multiplier, ..
        }: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent || color_multiplier.a <= 0.0 {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );
        let transform = transform.compute_final_transform();
        let builder = minigame::builder(stencil_ref);
        let alpha = color_multiplier.a;

        pass.push_debug("CakeLayer");
        Sprite::new(&self.assets.base, from_top_left(CAKE_RECT))
            .alpha(alpha)
            .draw(pass, builder, transform);
        for ((slice, texture), [x, y, w, h]) in
            self.slices.iter().zip(&self.assets.slices).zip(SLICE_RECTS)
        {
            let hover = slice.hover.value();
            if hover > 0.0 {
                let x = CAKE_LEFT + x;
                Sprite::new(texture, from_top_left([x, y, x + w, y + h]))
                    .alpha(hover * alpha)
                    .flash(slice.flash.value())
                    .draw(pass, builder, transform);
            }
        }
        let pointer_alpha = self.pointer_alpha.value();
        if pointer_alpha > 0.0 {
            Sprite::new(&self.assets.banner, from_top_left(BANNER_RECT))
                .alpha(pointer_alpha * alpha)
                .draw(pass, builder, transform);
            let finger = self.pointer - FINGER_OFFSET;
            Sprite::new(
                &self.assets.finger,
                from_top_left([
                    finger.x,
                    finger.y,
                    finger.x + FINGER_SIZE,
                    finger.y + FINGER_SIZE,
                ]),
            )
            .alpha(pointer_alpha * alpha)
            .draw(pass, builder, transform);
        }
        pass.pop_debug();
    }

    fn update_from_props(&mut self, _props: &LayerProperties) {}
}

impl NewDrawableLayerFastForward for CakeLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for CakeLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        if self.state == State::Picked
            && self
                .hovered
                .is_none_or(|slice| self.slices[slice].flash.is_idle())
        {
            self.state = State::Done;
        }
        for slice in &mut self.slices {
            slice.hover.update(ctx.delta_ticks);
            slice.flash.update(ctx.delta_ticks);
        }
        self.pointer_alpha.update(ctx.delta_ticks);
    }
}

impl Debug for CakeLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CakeLayer")
            .field("pointer", &self.pointer)
            .field("hovered", &self.hovered)
            .field("state", &self.state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_map_takes_the_last_opaque_slice_under_each_cell() {
        // fully opaque sprites for slices 0 (x 744..1000) and 1 (x 600..984), both from y 336
        let opaque = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]));
        let clear = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 0]));
        let mut images = vec![&clear; SLICES];
        images[0] = &opaque;
        images[1] = &opaque;
        let map = build_hit_map(&images);
        let at = |x: usize, y: usize| {
            map[(y - HIT_ORIGIN.y as usize) / HIT_CELL * HIT_CELLS + (x - 480) / HIT_CELL]
        };
        assert_eq!(at(620, 400), 1);
        assert_eq!(at(800, 400), 1);
        assert_eq!(at(990, 400), 0);
        assert_eq!(at(500, 100), -1);
    }
}
