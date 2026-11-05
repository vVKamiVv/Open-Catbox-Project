//! How the culprit game's screens draw: textured quads (program 2/5), flat quads (program 1), nine-slice frames and
//! clipped list rows, all in 1920 x 1080 top-left space.

use glam::{Mat4, Vec2};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::{QuadVertices, build_quad_vertices},
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosColTexVertex},
};

use super::MurdererAssets;
use crate::{
    layer::user::minigame::{self, Sprite},
    render::normalized_projection_matrix,
};

pub type Rect = [f32; 4];

/// A rect from its top-left corner and size
pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    [x, y, x + w, y + h]
}

pub const SCREEN: Rect = [0.0, 0.0, 1920.0, 1080.0];

pub struct Drawer<'p, 'a, 'b, 'c, 'd, 'e> {
    pass: &'p mut RenderPass<'a, 'b, 'c, 'd, 'e>,
    assets: &'p MurdererAssets,
    transform: Mat4,
    builder: RenderRequestBuilder,
}

impl<'p, 'a, 'b, 'c, 'd, 'e> Drawer<'p, 'a, 'b, 'c, 'd, 'e> {
    pub fn new(
        pass: &'p mut RenderPass<'a, 'b, 'c, 'd, 'e>,
        assets: &'p MurdererAssets,
        transform: Mat4,
    ) -> Self {
        Self {
            pass,
            assets,
            transform,
            // the screens draw with `setLayerDepthStencil(1, 1, 1)` after clearing the stencil
            builder: minigame::builder(1),
        }
    }

    pub fn texture(&self, name: &str) -> &'p GpuTexture {
        self.assets.textures.get(name)
    }

    pub fn system_texture(&self, name: &str) -> &'p GpuTexture {
        self.assets.system.get(name)
    }

    /// The first texture of the bust archive `name`
    pub fn bust(&self, name: &str) -> Option<&'p GpuTexture> {
        self.assets
            .busts
            .iter()
            .find(|(bust, _)| *bust == name)
            .and_then(|(_, textures)| textures.first())
    }

    /// A textured quad, or nothing when it is fully transparent
    pub fn sprite(&mut self, sprite: Sprite) {
        sprite.draw(self.pass, self.builder, self.transform);
    }

    /// The whole texture `name` over `rect`, white
    pub fn image(&mut self, name: &str, rect: Rect) {
        let texture = self.texture(name);
        self.sprite(Sprite::new(texture, rect));
    }

    /// An untextured quad in `color` (program 1)
    pub fn flat(&mut self, [x0, y0, x1, y1]: Rect, color: FloatColor4) {
        if color.a <= 0.0 {
            return;
        }
        QuadVertices::new()
            .with_box(x0, y0, x1, y1)
            .with_color(color.into_unorm())
            .fill(self.pass, self.builder, self.transform);
    }

    /// A frame cut into 3 x 3 pieces: screen edges `xs`/`ys`, texture edges `us`/`vs`, drawn row by row
    pub fn nine_slice(
        &mut self,
        name: &str,
        xs: [f32; 4],
        ys: [f32; 4],
        us: [f32; 4],
        vs: [f32; 4],
    ) {
        let texture = self.texture(name);
        for row in 0..3 {
            for col in 0..3 {
                self.sprite(
                    Sprite::new(texture, [xs[col], ys[row], xs[col + 1], ys[row + 1]]).uv([
                        us[col],
                        vs[row],
                        us[col + 1],
                        vs[row + 1],
                    ]),
                );
            }
        }
    }
}

/// `sprite` cut to `clip`, or `None` when nothing is left (replaces the original's stencil mask, which is the same
/// for axis-aligned rects)
pub fn clip(sprite: Sprite<'_>, clip: Rect) -> Option<Sprite<'_>> {
    let (rect, uv) = clip_rect(sprite.rect, sprite.uv, clip)?;
    Some(Sprite { rect, uv, ..sprite })
}

/// `rect` cut to `clip`, with its uvs cut in proportion
fn clip_rect(rect: Rect, uv: Rect, [cx0, cy0, cx1, cy1]: Rect) -> Option<(Rect, Rect)> {
    let [x0, y0, x1, y1] = rect;
    let [u0, v0, u1, v1] = uv;
    let (nx0, ny0, nx1, ny1) = (x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1));
    if nx0 >= nx1 || ny0 >= ny1 {
        return None;
    }
    let u = |x: f32| u0 + (u1 - u0) * (x - x0) / (x1 - x0);
    let v = |y: f32| v0 + (v1 - v0) * (y - y0) / (y1 - y0);
    Some(([nx0, ny0, nx1, ny1], [u(nx0), v(ny0), u(nx1), v(ny1)]))
}

/// Draws `texture` over the whole target, opaque (the layer's plain draw `0x710009b400` when no transition runs)
pub fn blit(pass: &mut RenderPass, builder: RenderRequestBuilder, texture: TextureSource) {
    let vertices = build_quad_vertices(|t: Vec2| PosColTexVertex {
        position: t.extend(0.0),
        color: FloatColor4::WHITE.into_unorm(),
        texture_position: t,
    });
    pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
        RenderProgramWithArguments::Sprite {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            sprite: texture,
            transform: normalized_projection_matrix(),
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

/// The colour of a white quad with alpha `alpha`
pub fn white(alpha: f32) -> FloatColor4 {
    FloatColor4::WHITE.with_alpha(alpha)
}

/// Linear interpolation between two points
pub fn lerp2(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_cuts_the_uvs_in_proportion() {
        let clipped = clip_rect(
            [0.0, 0.0, 100.0, 100.0],
            [0.0, 0.0, 1.0, 0.5],
            [50.0, -10.0, 200.0, 25.0],
        );
        assert_eq!(
            clipped,
            Some(([50.0, 0.0, 100.0, 25.0], [0.5, 0.0, 1.0, 0.125]))
        );
        assert_eq!(
            clip_rect(
                [0.0, 0.0, 10.0, 10.0],
                [0.0, 0.0, 1.0, 1.0],
                [20.0, 0.0, 30.0, 10.0]
            ),
            None
        );
    }
}
