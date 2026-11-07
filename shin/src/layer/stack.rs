//! What the layer containers share: [`LayerGroup`](super::LayerGroup) (a plane's layer banks),
//! [`PageLayer`](super::PageLayer) (the planes) and the [`ScreenLayer`](super::ScreenLayer). Each keeps its children
//! in its own order and with its own rules for which are drawn; this is how the chosen ones are drawn, as the original
//! draws a layer stack: every child gets a stencil value from 1 up (each child bumps it by its own amount), the opaque
//! pass goes top to bottom, then the pixels nothing covered are cleared, then the transparent pass goes bottom to top.

use glam::Vec3;
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    DepthStencilState, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    StencilFunction, StencilMask, StencilOperation, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosVertex},
};

use crate::layer::{
    properties::LayerProperties,
    render_params::{DrawableClipMode, TransformParams},
};

/// One triangle over the whole target, for the full-screen clears drawn through the stencil
pub const FULLSCREEN_TRIANGLE: [PosVertex; 3] = [
    PosVertex {
        position: Vec3::new(-1.0, 1.0, 0.0),
    },
    PosVertex {
        position: Vec3::new(3.0, 1.0, 0.0),
    },
    PosVertex {
        position: Vec3::new(-1.0, -3.0, 0.0),
    },
];

/// Whether a container is drawn offscreen first, for its effects to apply to it as a whole: clipping, a fragment
/// shader or a blend type other than the plain one (a layer group with a mask also is, see `LayerGroup`)
pub fn needs_offscreen_pass(props: &LayerProperties) -> bool {
    props.get_clip_mode() != DrawableClipMode::None
        || props.is_fragment_shader_nontrivial()
        || props.is_blending_nontrivial()
}

/// Fills with `color` the pixels whose stencil value is below `stencil_ref`: those no child drawn above has covered
pub fn fill_uncovered(pass: &mut RenderPass, stencil_ref: u8, color: FloatColor4) {
    pass.run(
        RenderRequestBuilder::new()
            .depth_stencil(DepthStencilState {
                depth: Default::default(),
                stencil: StencilState {
                    pipeline: StencilPipelineState {
                        function: StencilFunction::Greater,
                        stencil_fail_operation: StencilOperation::Keep,
                        depth_fail_operation: StencilOperation::Keep,
                        pass_operation: StencilOperation::Replace,
                        stencil_read_mask: StencilMask::All,
                        stencil_write_mask: StencilMask::All,
                    },
                    stencil_reference: stencil_ref,
                },
            })
            .build(
                RenderProgramWithArguments::Clear {
                    vertices: VertexSource::VertexData {
                        vertices: &FULLSCREEN_TRIANGLE,
                    },
                    color,
                },
                DrawPrimitive::Triangles,
            ),
    );
}

#[derive(Clone, Copy)]
struct StackItem {
    child: usize,
    stencil_ref_relative: u8,
}

/// The children a container draws this frame, bottom first, with their stencil values
#[derive(Clone)]
pub struct LayerStack {
    items: Vec<StackItem>,
    stencil_bump: u8,
}

impl LayerStack {
    /// `stencil_bump`: what the container reports before its first arrangement
    pub fn new(stencil_bump: u8) -> Self {
        Self {
            items: Vec::new(),
            stencil_bump,
        }
    }

    /// How far the container moves the stencil value of what is drawn after it
    pub fn stencil_bump(&self) -> u8 {
        self.stencil_bump
    }

    /// Takes `children` (indices into the container's own list, bottom first) for this frame; `bump` is each one's
    /// stencil bump
    pub fn arrange(
        &mut self,
        children: impl IntoIterator<Item = usize>,
        bump: impl Fn(usize) -> u8,
    ) {
        self.items.clear();
        let mut stencil_value = 1;
        for child in children {
            self.items.push(StackItem {
                child,
                stencil_ref_relative: stencil_value,
            });
            stencil_value += bump(child);
        }
        self.stencil_bump = stencil_value;
    }

    /// Draws one pass of the children over `stencil_ref` (the container drawn in place); the opaque pass ends by
    /// clearing what none of them covered
    pub fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
        mut child: impl FnMut(usize, &mut RenderPass, &TransformParams, u8, PassKind),
    ) {
        match pass_kind {
            PassKind::Opaque => {
                for item in self.items.iter().rev() {
                    child(
                        item.child,
                        pass,
                        transform,
                        stencil_ref + item.stencil_ref_relative,
                        pass_kind,
                    );
                }
                pass.push_debug("clear");
                fill_uncovered(pass, stencil_ref, FloatColor4::TRANSPARENT);
                pass.pop_debug();
            }
            PassKind::Transparent => {
                for item in &self.items {
                    child(
                        item.child,
                        pass,
                        transform,
                        stencil_ref + item.stencil_ref_relative,
                        pass_kind,
                    );
                }
            }
        }
    }

    /// Draws both passes of the children into an offscreen target cleared beforehand (the container drawn
    /// indirectly, for its effects), then forgets them: the container is shown from that texture this frame
    pub fn render_offscreen(
        &mut self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        mut child: impl FnMut(usize, &mut RenderPass, &TransformParams, u8, PassKind),
    ) {
        for item in self.items.iter().rev() {
            child(
                item.child,
                pass,
                transform,
                item.stencil_ref_relative,
                PassKind::Opaque,
            );
        }
        for item in &self.items {
            child(
                item.child,
                pass,
                transform,
                item.stencil_ref_relative,
                PassKind::Transparent,
            );
        }
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stencil_values_follow_each_childs_bump() {
        let mut stack = LayerStack::new(0);
        assert_eq!(stack.stencil_bump(), 0);
        // children 2, 0, 1 bottom first, bumping by 1, 3 and 2
        let bumps = [3, 2, 1];
        stack.arrange([2, 0, 1], |i| bumps[i]);
        let values: Vec<_> = stack
            .items
            .iter()
            .map(|i| (i.child, i.stencil_ref_relative))
            .collect();
        assert_eq!(values, [(2, 1), (0, 2), (1, 5)]);
        assert_eq!(stack.stencil_bump(), 7);
        stack.arrange([], |_| 1);
        assert_eq!(
            stack.stencil_bump(),
            1,
            "an empty stack still takes one value"
        );
    }
}
