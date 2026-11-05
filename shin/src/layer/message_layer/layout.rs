use glam::{Vec2, vec2, vec4};
use shin_core::{format::font::GlyphInfo, primitives::color::UnormColor};
use shin_derive::RenderClone;
use shin_render::shaders::types::vertices::TextVertex;

use crate::asset::font::GpuFontGlyphHandle;

#[derive(Debug, Clone, RenderClone)]
pub struct LineInfo {
    pub y_position: f32,
    pub baseline_ascent: f32,
    pub line_height: f32,
    pub rubi_height: f32,
    /// Set to 1.0 when any of the chars on the line are visible
    ///
    /// This field is seemingly unused
    pub is_visible: f32,
}

#[derive(Debug, Clone, RenderClone)]
pub struct Char {
    pub time: f32,
    pub line_index: usize,
    pub is_rubi: bool,
    pub position: Vec2,
    pub width: f32,
    pub height: f32,
    pub horizontal_scale: f32,
    pub vertical_scale: f32,
    pub color_rgba: UnormColor,
    pub progress_rate: f32,
    pub current_progress: f32,
    pub block_index: usize,
    pub vertex_buffer_offset: usize,
    pub border_distances: [Vec2; 8],
    pub glyph: GpuFontGlyphHandle,
}

impl Char {
    pub fn scale(&self) -> Vec2 {
        Vec2::new(self.horizontal_scale, self.vertical_scale)
    }

    /// Offsets for the outline shader: `outline` pixels in 8 directions, in glyph texture units
    pub fn border_distances(
        info: &GlyphInfo,
        horizontal_scale: f32,
        vertical_scale: f32,
        outline: f32,
    ) -> [Vec2; 8] {
        // TODO: maybe support v8=1 setting, which changes the shape of the outline
        let v_distance =
            outline / vertical_scale / info.actual_height as f32 * info.actual_size_normalized().y;
        let h_distance =
            outline / horizontal_scale / info.actual_width as f32 * info.actual_size_normalized().x;

        // they go in a circle, just in a weird order:
        //
        //         |
        //         2
        //      1  |  3
        // ---4---------5---> x
        //      6  |  8
        //         7
        //         |
        //        \/  y
        #[rustfmt::skip]
        let directions = [
            /* 1 */ vec2(-1.0, -1.0),
            /* 2 */ vec2( 0.0, -1.0),
            /* 3 */ vec2( 1.0, -1.0),
            /* 4 */ vec2(-1.0,  0.0),
            /* 5 */ vec2( 1.0,  0.0),
            /* 6 */ vec2(-1.0,  1.0),
            /* 7 */ vec2( 0.0,  1.0),
            /* 8 */ vec2( 1.0,  1.0),
        ];
        directions.map(|v| v / v.length() * vec2(h_distance, v_distance))
    }

    /// The char's quad (with 2 pixels of overdraw on every side); `line` gives the tint gradient
    /// The glyph's quad, 2 px larger than the glyph on each side for its outline (the message box)
    pub fn quad(&self, line: &LineInfo) -> [TextVertex; 4] {
        self.quad_with_overdraw(line, 2.0)
    }

    /// The glyph's quad, `overdraw` px larger on each side (a text object uses its outline width, `0x7100101360`)
    pub fn quad_with_overdraw(&self, line: &LineInfo, overdraw: f32) -> [TextVertex; 4] {
        let glyph = self.glyph.info();
        let scaled_size = self.scale() * glyph.actual_size_f32();

        let pos_to_top_left = self.scale() * glyph.bearing_screenspace_f32() - overdraw;
        let pos_to_bottom_right = pos_to_top_left + scaled_size + 2.0 * overdraw;

        let [screen_left, screen_top] = (self.position + pos_to_top_left).to_array();
        let [screen_right, screen_bottom] = (self.position + pos_to_bottom_right).to_array();

        let tex_overdraw_ratio =
            ((pos_to_bottom_right - pos_to_top_left) / scaled_size - 1.0) / 2.0;

        let [tex_left, tex_top] = (-glyph.actual_size_normalized() * tex_overdraw_ratio).to_array();
        let [tex_right, tex_bottom] =
            (glyph.actual_size_normalized() * (tex_overdraw_ratio + 1.0)).to_array();

        let (color_top, color_bottom) = if self.is_rubi {
            (0.0, 0.0)
        } else {
            // I don't think the formulas in the original engine are right (it doesn't take scale into account),
            // but umineko doesn't rely on this feature (both tints are set to the same color),
            // so I ain't gonna fix it

            // NB: this bearing is NOT in screenspace, so we need to flip the Y
            let effective_ascent = line.baseline_ascent - glyph.bearing_y as f32;
            let effective_descent = effective_ascent + glyph.actual_height as f32;

            (
                effective_ascent / line.line_height,
                effective_descent / line.line_height,
            )
        };

        [
            TextVertex {
                position: vec4(screen_left, screen_top, tex_left, tex_top),
                color: color_top,
            },
            TextVertex {
                position: vec4(screen_right, screen_top, tex_right, tex_top),
                color: color_top,
            },
            TextVertex {
                position: vec4(screen_left, screen_bottom, tex_left, tex_bottom),
                color: color_bottom,
            },
            TextVertex {
                position: vec4(screen_right, screen_bottom, tex_right, tex_bottom),
                color: color_bottom,
            },
        ]
    }
}
