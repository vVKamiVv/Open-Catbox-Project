//! Whirl transition (`WIPE` type 8), re-implemented from the behaviour of `shin::WhirlWiper` in the Switch v1.0.3
//! executable (constructor `0x710011b410`, render `0x710011b770`, shader program `0x28`); see
//! `docs/engine-internals.md` §7. Umineko uses it once.

use glam::vec4;
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};

/// Below this magnitude the scale parameter counts as unset (the original compares against FLT_EPSILON at 0x710015aa98)
const MIN_SCALE_PARAM: f32 = f32::EPSILON;

#[derive(Debug, Clone)]
pub struct WhirlWiperImpl {
    /// +1 or -1 turn over the whole transition
    direction: f32,
    /// vertical scale of the distance from the centre
    scale: f32,
}

impl WhirlWiperImpl {
    fn new(direction: i32, scale_param: f32) -> Self {
        Self {
            direction: if direction == 1 { 1.0 } else { -1.0 },
            scale: if scale_param.abs() >= MIN_SCALE_PARAM {
                1.0 / scale_param
            } else {
                1.0
            },
        }
    }
}

impl AdvUpdatable for WhirlWiperImpl {
    fn update(&mut self, _context: &AdvUpdateContext) {}
}

impl TimedWiper for WhirlWiperImpl {
    fn render(
        &self,
        pass: &mut RenderPass,
        render_request_builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    ) {
        let vertices = &build_quad_vertices(|t| PosTexVertex {
            position: t,
            texture_position: t,
        });

        pass.run(
            render_request_builder
                .color_blend_type(ColorBlendType::Opaque)
                .build(
                    RenderProgramWithArguments::WiperWhirl {
                        vertices: VertexSource::VertexData { vertices },
                        texture_source,
                        texture_target,
                        transform: normalized_projection_matrix(),
                        params: vec4(progress, self.direction, 1.0, self.scale),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

pub type WhirlWiper = TimedWiperWrapper<WhirlWiperImpl>;

impl WhirlWiper {
    /// `direction` and `scale_param` are the first two `WIPE` parameters
    pub fn new(duration: Ticks, direction: i32, scale_param: f32) -> Self {
        Self::from_inner(WhirlWiperImpl::new(direction, scale_param), duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_follow_the_original_constructor() {
        // Umineko's only use passes zeros: direction -1, unscaled
        let w = WhirlWiperImpl::new(0, 0.0);
        assert_eq!((w.direction, w.scale), (-1.0, 1.0));
        let w = WhirlWiperImpl::new(1, 2.0);
        assert_eq!((w.direction, w.scale), (1.0, 0.5));
    }
}
