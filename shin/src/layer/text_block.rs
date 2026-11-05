//! A block of laid-out text drawn on its own, outside the message box: the original's text object (constructor
//! `0x7100100ea0`, layout `0x71001011d0`, draw `0x7100101a90`), which the choice menu uses for its options.

use glam::{Mat4, Vec2};
use itertools::{Either, Itertools};
use shin_core::{
    layout::{
        LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults,
        commands::{CharFontType, Command},
    },
    primitives::color::FloatColor4,
};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        buffer::{OwnedVertexBuffer, VertexSource},
        vertices::TextVertex,
    },
};

use crate::{
    adv::assets::AdvFonts,
    layer::message_layer::layout::{Char, LineInfo},
    render::PreRenderContext,
};

const VERTICES_PER_CHARACTER: usize = 4;

pub struct TextBlock {
    chars: Vec<Char>,
    /// Where the chars start horizontally; drawing shifts them to start at the outline's edge
    left: f32,
    outline: f32,
    size: Vec2,
    /// `None` for empty text: wgpu rejects empty buffers
    vertex_buffer: Option<OwnedVertexBuffer<TextVertex>>,
}

impl TextBlock {
    /// Lays out `text` (with the message markup) fully shown, with an outline `outline` pixels thick
    pub fn new(
        ctx: &PreRenderContext,
        fonts: &AdvFonts,
        text: &str,
        params: LayoutParams,
        outline: f32,
    ) -> Self {
        let defaults = MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 100,
            fade: 0,
        };
        // the plain layouter: the message box's one treats the first line as a speaker's name
        let (commands, lines, size) = MessageTextLayouter::new(
            fonts.medium_font.as_ref(),
            fonts.bold_font.as_ref(),
            params,
            defaults,
        )
        .parse(text);

        let layout_chars = commands
            .into_iter()
            .filter_map(|c| match c {
                Command::Char(c) => Some(c),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (regular, bold) = layout_chars
            .iter()
            .partition_map::<Vec<_>, Vec<_>, _, _, _>(|c| match c.font {
                CharFontType::Regular => Either::Left(c.codepoint),
                CharFontType::Bold => Either::Right(c.codepoint),
            });
        let mut regular = fonts
            .medium_font
            .clone()
            .load_glyphs(ctx.device.clone(), ctx.queue.clone(), &regular)
            .into_iter();
        let mut bold = fonts
            .bold_font
            .clone()
            .load_glyphs(ctx.device.clone(), ctx.queue.clone(), &bold)
            .into_iter();

        let lines = lines
            .iter()
            .map(|line| LineInfo {
                y_position: line.y_position,
                baseline_ascent: line.baseline_ascent,
                line_height: line.line_height,
                rubi_height: line.rubi_height,
                is_visible: 1.0,
            })
            .collect::<Vec<_>>();

        let chars = layout_chars
            .into_iter()
            .enumerate()
            .map(|(index, char)| {
                // rely on the order of the glyphs returned from `load_glyphs`
                let glyph = match char.font {
                    CharFontType::Regular => regular.next().unwrap(),
                    CharFontType::Bold => bold.next().unwrap(),
                };
                let border_distances = Char::border_distances(
                    glyph.info(),
                    char.horizontal_scale,
                    char.scale,
                    outline,
                );
                Char {
                    time: char.time,
                    line_index: char.line_index,
                    is_rubi: char.is_rubi,
                    position: char.position,
                    width: char.width,
                    height: char.height,
                    horizontal_scale: char.horizontal_scale,
                    vertical_scale: char.scale,
                    color_rgba: char.color,
                    progress_rate: 1.0,
                    current_progress: 1.0,
                    block_index: 0,
                    vertex_buffer_offset: index * VERTICES_PER_CHARACTER,
                    border_distances,
                    glyph,
                }
            })
            .collect::<Vec<_>>();

        let vertices = chars
            .iter()
            .flat_map(|char| char.quad(&lines[char.line_index]))
            .collect::<Vec<_>>();
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            OwnedVertexBuffer::allocate_vertex(ctx.device, &vertices, Some("TextBlock/vtxbuf"))
        });

        // the layouter does not start lines at x = 0, so measure what the chars cover
        let (left, right) = chars.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| {
            (lo.min(c.position.x), hi.max(c.position.x + c.width))
        });
        let left = if chars.is_empty() { 0.0 } else { left };
        // the outline pads the block on every side (`0x7100101360`)
        Self {
            chars,
            left,
            outline,
            size: Vec2::new((right - left).max(0.0), size.y) + 2.0 * outline,
            vertex_buffer,
        }
    }

    /// Width and height of the laid-out text with its outline, drawn from (0, 0)
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Draws the outline in `border`, then the text in its markup colours multiplied by `tint`
    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        tint: FloatColor4,
        border: FloatColor4,
    ) {
        let Some(vertex_buffer) = &self.vertex_buffer else {
            return;
        };
        let transform = transform
            * Mat4::from_translation(glam::vec3(self.outline - self.left, self.outline, 0.0));
        let vertices = |char: &Char| VertexSource::VertexBuffer {
            vertices: vertex_buffer
                .as_sliced_buffer_ref(char.vertex_buffer_offset, VERTICES_PER_CHARACTER),
        };

        for char in &self.chars {
            pass.run(builder.build(
                RenderProgramWithArguments::FontBorder {
                    vertices: vertices(char),
                    glyph: char.glyph.as_texture_source(),
                    transform,
                    distances: char.border_distances,
                    color: border,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
        for char in &self.chars {
            let color = FloatColor4::from_unorm(char.color_rgba) * tint;
            pass.run(builder.build(
                RenderProgramWithArguments::Font {
                    vertices: vertices(char),
                    glyph: char.glyph.as_texture_source(),
                    transform,
                    color1: color,
                    color2: color,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
