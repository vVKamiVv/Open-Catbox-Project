//! The port's own texts in serif faces close to the game's lettering (the original draws its labels as art).
//!
//! Gelasio (Georgia's metrics, like the art's labels) and Cinzel (the tabs' Trajan-like capitals), both under the
//! SIL Open Font License (`shin/fonts`). A text is rasterised once into a texture at twice the virtual size, with
//! the art's soft dark shadow baked in, and drawn tinted.

use std::sync::{Arc, LazyLock};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, VariableFont};
use glam::{Mat4, Vec2, vec2};
use image::{Rgba, RgbaImage};
use shin_core::primitives::color::FloatColor4;
use shin_render::{gpu_texture::GpuTexture, render_pass::RenderPass};

use crate::{
    adv::hud::{OUTLINE, text_layout},
    asset::font::GpuFontLazy,
    layer::{text_block::TextBlock, user::minigame::Sprite},
    render::PreRenderContext,
    ui,
};

static GELASIO: &[u8] = include_bytes!("../../fonts/Gelasio.ttf");
static CINZEL: &[u8] = include_bytes!("../../fonts/Cinzel.ttf");

/// Rendered at this many texels per virtual pixel, so it stays sharp on 1440p and 4K windows
const OVERSAMPLE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Face {
    /// Gelasio, the labels and options (Georgia-like, as the art's)
    Serif,
    /// Cinzel, the tabs' engraved capitals
    Capitals,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub face: Face,
    /// The font's weight axis (400 regular, 700 bold)
    pub weight: f32,
    /// The em size in virtual pixels
    pub size: f32,
    /// Horizontal scale: the art's lettering is a little narrower than Gelasio
    pub squeeze: f32,
    /// The baked shadow: offset in virtual pixels and its opacity (0: none)
    pub shadow: (Vec2, f32),
}

impl Style {
    /// The config screen's row labels and option texts: bold, narrowed, with the art's soft shadow
    pub const LABEL: Style = Style {
        face: Face::Serif,
        weight: 650.0,
        size: 33.0,
        squeeze: 0.92,
        shadow: (Vec2::new(2.0, 2.0), 0.85),
    };
}

pub struct SerifText {
    texture: GpuTexture,
    /// The drawn size in virtual pixels
    pub size: Vec2,
    /// From the top to the middle of the capitals, in virtual pixels: what centring uses
    cap_middle: f32,
}

fn font(face: Face, weight: f32) -> FontRef<'static> {
    static FONTS: LazyLock<[FontRef<'static>; 2]> = LazyLock::new(|| {
        [
            FontRef::try_from_slice(GELASIO).expect("Gelasio.ttf"),
            FontRef::try_from_slice(CINZEL).expect("Cinzel.ttf"),
        ]
    });
    let mut font = FONTS[face as usize].clone();
    font.set_variation(b"wght", weight);
    font
}

/// Whether every character has a glyph (Japanese has none in these faces)
pub fn covers(face: Face, text: &str) -> bool {
    let font = font(face, 400.0);
    text.chars().all(|c| c == ' ' || font.glyph_id(c).0 != 0)
}

/// A box blur of an alpha mask, twice (close enough to the art's soft shadow)
fn blur(mask: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut a = mask.to_vec();
    for _ in 0..2 {
        let mut b = vec![0.0; a.len()];
        for y in 0..h {
            for x in 0..w {
                let (x0, x1) = (x.saturating_sub(r), (x + r).min(w - 1));
                b[y * w + x] = a[y * w + x0..=y * w + x1].iter().sum::<f32>() / (2 * r + 1) as f32;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (y0, y1) = (y.saturating_sub(r), (y + r).min(h - 1));
                a[y * w + x] = (y0..=y1).map(|y| b[y * w + x]).sum::<f32>() / (2 * r + 1) as f32;
            }
        }
    }
    a
}

/// Lays `text` out and rasterises it: white letters over a black shadow, straight alpha. Returns the image, its
/// size in virtual pixels and the capitals' middle from its top.
fn rasterise(text: &str, style: Style) -> (RgbaImage, Vec2, f32) {
    let font = font(style.face, style.weight);
    // ab_glyph scales by the line's height (ascent to descent); `size` is the em
    let em = style.size * OVERSAMPLE * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0);
    let scale = PxScale {
        x: em * style.squeeze,
        y: em,
    };
    let scaled = font.as_scaled(scale);
    let pad = (4.0 * OVERSAMPLE).ceil();
    let shadow = style.shadow.0 * OVERSAMPLE;
    let mut x = 0.0;
    let mut last = None;
    let mut glyphs = Vec::new();
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(last) = last {
            x += scaled.kern(last, id);
        }
        glyphs.push(id.with_scale_and_position(scale, ab_glyph::point(pad + x, pad + scaled.ascent())));
        x += scaled.h_advance(id);
        last = Some(id);
    }
    let w = (x + 2.0 * pad + shadow.x.max(0.0)).ceil() as usize;
    let h = (scaled.ascent() - scaled.descent() + 2.0 * pad + shadow.y.max(0.0)).ceil() as usize;
    let mut mask = vec![0.0f32; w * h];
    for glyph in glyphs {
        if let Some(outline) = scaled.outline_glyph(glyph) {
            let b = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let (x, y) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                    let m = &mut mask[y as usize * w + x as usize];
                    *m = (*m + c).min(1.0);
                }
            });
        }
    }
    let soft = blur(&mask, w, h, OVERSAMPLE as usize);
    let (dx, dy) = (shadow.x.round() as i32, shadow.y.round() as i32);
    let image = RgbaImage::from_fn(w as u32, h as u32, |x, y| {
        let fill = mask[y as usize * w + x as usize];
        let (sx, sy) = (x as i32 - dx, y as i32 - dy);
        let under = if (0..w as i32).contains(&sx) && (0..h as i32).contains(&sy) {
            soft[sy as usize * w + sx as usize] * style.shadow.1
        } else {
            0.0
        };
        // the white letter over the black shadow ("over" in straight alpha)
        let alpha = fill + under * (1.0 - fill);
        let white = if alpha > 0.0 { fill / alpha } else { 0.0 };
        let v = (white * 255.0) as u8;
        Rgba([v, v, v, (alpha * 255.0) as u8])
    });
    // the capital H's ink height, for centring on the capitals rather than on the line with its descenders
    let cap = scaled
        .outline_glyph(scaled.glyph_id('H').with_scale(scale))
        .map_or(scaled.ascent() * 0.7, |o| o.px_bounds().height());
    let cap_middle = (pad + scaled.ascent() - cap / 2.0) / OVERSAMPLE;
    (image, vec2(w as f32, h as f32) / OVERSAMPLE, cap_middle)
}

impl SerifText {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, text: &str, style: Style) -> Self {
        let (image, size, cap_middle) = rasterise(text, style);
        let texture = GpuTexture::new_static_from_rgba_image(device, queue, Some(text), &image);
        Self {
            texture,
            size,
            cap_middle,
        }
    }

    /// Drawn with its left edge at `x` and its capitals centred on `y`
    pub fn draw(&self, pass: &mut RenderPass, transform: Mat4, x: f32, y: f32, color: FloatColor4) {
        let top = y - self.cap_middle;
        // the button program, as the menus' sprites (the sprite program swaps the tint's red and blue)
        let mut sprite = Sprite::new(&self.texture, [x, top, x + self.size.x, top + self.size.y]);
        sprite.color = color;
        sprite.draw(pass, ui::builder(), transform);
    }
}

/// A port text: in a serif face when it covers the text, else (Japanese) in the game's own font at `game_size`
pub enum UiText {
    Serif(SerifText),
    Game(TextBlock),
}

impl UiText {
    pub fn new(
        ctx: &PreRenderContext,
        game_font: &Arc<GpuFontLazy>,
        text: &str,
        style: Style,
        game_size: f32,
    ) -> Self {
        if covers(style.face, text) {
            Self::Serif(SerifText::new(ctx.device, ctx.queue, text, style))
        } else {
            Self::Game(TextBlock::new(ctx, game_font, text, text_layout(1000.0, game_size), OUTLINE))
        }
    }

    pub fn width(&self) -> f32 {
        match self {
            Self::Serif(text) => text.size.x,
            Self::Game(text) => text.size().x,
        }
    }

    /// Drawn with its left edge at `x`, its middle on `y`; `border` is the game font's outline
    pub fn draw(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        x: f32,
        y: f32,
        tint: FloatColor4,
        border: FloatColor4,
    ) {
        match self {
            Self::Serif(text) => text.draw(pass, transform, x, y, tint),
            Self::Game(text) => {
                let at = vec2(x, y - text.size().y / 2.0);
                ui::draw_text(pass, text, transform * Mat4::from_translation(at.extend(0.0)), tint, border);
            }
        }
    }

    /// Drawn centred on `center`
    pub fn draw_centered(
        &self,
        pass: &mut RenderPass,
        transform: Mat4,
        center: Vec2,
        tint: FloatColor4,
        border: FloatColor4,
    ) {
        self.draw(pass, transform, center.x - self.width() / 2.0, center.y, tint, border);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_rasterised_with_its_capitals_inside() {
        let (image, size, cap_middle) = rasterise("Window size", Style::LABEL);
        assert!(image.pixels().any(|p| p.0 == [255, 255, 255, 255]));
        assert!(size.x > 100.0 && size.y > 20.0);
        assert!(cap_middle > 0.0 && cap_middle < size.y);
        assert!(covers(Face::Serif, "1920 x 1080") && !covers(Face::Serif, "ウィンドウ"));
    }
}
