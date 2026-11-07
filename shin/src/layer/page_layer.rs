use shin_core::{
    primitives::color::UnormColor,
    vm::command::types::{PLANES_COUNT, PlaneId},
};
use shin_derive::RenderClone;
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::texture::{DepthStencilTarget, TextureTarget},
};

use crate::{
    layer::{
        DrawableLayer, Layer, LayerGroup, NewDrawableLayer,
        new_drawable_layer::{NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState},
        properties::LayerProperties,
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
        stack::{LayerStack, needs_offscreen_pass},
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

/// The page: the four planes, each a [`LayerGroup`], drawn from the lowest one that covers the screen up
#[derive(RenderClone)]
pub struct PageLayer {
    #[render_clone(needs_render)]
    planes: Vec<LayerGroup>,
    stack: LayerStack,

    #[render_clone(needs_render)]
    new_drawable_state: NewDrawableLayerState,
    props: LayerProperties,
}

impl PageLayer {
    pub fn new(plane_count: usize) -> Self {
        // can't support more, the `PlaneId` type is specialized to 4 planes
        // ideally I would like to only support 4 planes, but `BustupMode` at least uses 1
        assert!(plane_count <= PLANES_COUNT);

        Self {
            planes: (0..plane_count)
                .map(|i| LayerGroup::new(Some(format!("PageLayer/Plane{}", i))))
                .collect(),
            stack: LayerStack::new(1),
            new_drawable_state: NewDrawableLayerState::new(),
            props: LayerProperties::new(),
        }
    }

    pub fn get_plane(&self, index: PlaneId) -> &LayerGroup {
        &self.planes[index.raw() as usize]
    }

    pub fn get_plane_mut(&mut self, index: PlaneId) -> &mut LayerGroup {
        &mut self.planes[index.raw() as usize]
    }
}

struct PageLayerNewDrawableDelegate<'a> {
    planes: &'a [LayerGroup],
    stack: &'a mut LayerStack,
}

impl NewDrawableLayerNeedsSeparatePass for PageLayerNewDrawableDelegate<'_> {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        needs_offscreen_pass(props)
    }
}

impl NewDrawableLayer for PageLayerNewDrawableDelegate<'_> {
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "PageLayer/indirect");
        let planes = self.planes;

        if !props.is_visible() {
            pass.clear(Some(UnormColor::TRANSPARENT), None, None);
        } else {
            let self_transform = props.get_composed_transform_params(transform);
            pass.clear(Some(UnormColor::TRANSPARENT), Some(0), None);
            self.stack
                .render_offscreen(&mut pass, &self_transform, |i, pass, t, s, k| {
                    planes[i].render(pass, t, s, k)
                });
        }

        PassKind::Opaque
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
        // direct rendering is always done by the PageLayer without relying on NewDrawableLayer
        unreachable!()
    }
}

impl AdvUpdatable for PageLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.props.update(context);
        self.new_drawable_state.update(context, &self.props);

        for plane in self.planes.iter_mut() {
            plane.update(context);
        }
    }
}

impl Layer for PageLayer {
    fn fast_forward(&mut self) {
        self.props.fast_forward();

        for plane in &mut self.planes {
            plane.fast_forward();
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        self.stack.stencil_bump()
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        let self_transform = self.props.get_composed_transform_params(transform);

        // the planes with something to draw, from the topmost one drawn opaquely up: nothing below it shows
        let mut shown = Vec::new();
        for (index, plane) in self.planes.iter_mut().enumerate() {
            plane.pre_render(context, &self_transform);
            if plane.needs_rendering() {
                if plane.is_rendered_opaquely() {
                    shown.clear();
                }
                shown.push(index);
            }
        }

        let planes = &self.planes;
        self.stack.arrange(shown, |i| planes[i].get_stencil_bump());

        let mut delegate = PageLayerNewDrawableDelegate {
            planes: &self.planes,
            stack: &mut self.stack,
        };

        // the parent's transform: the indirect render composes this layer's own (`0x710009f360`)
        self.new_drawable_state
            .pre_render(context, &self.props, &mut delegate, transform);
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let props = self.properties();
        if self.new_drawable_state.try_finish_indirect_render(
            props,
            pass,
            transform,
            stencil_ref,
            pass_kind,
        ) {
            return;
        }

        if !props.is_visible() {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);

        pass.push_debug(&format!(
            "PageLayer/{}",
            match pass_kind {
                PassKind::Opaque => "opaque",
                PassKind::Transparent => "transparent",
            }
        ));
        self.stack.render(
            pass,
            &self_transform,
            stencil_ref,
            pass_kind,
            |i, pass, t, s, k| self.planes[i].render(pass, t, s, k),
        );
        pass.pop_debug();
    }
}

impl DrawableLayer for PageLayer {
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}
