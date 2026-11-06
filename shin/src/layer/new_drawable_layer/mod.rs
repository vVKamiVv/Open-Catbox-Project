pub(crate) mod effect_passes;

use glam::{Vec2, Vec4};
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    vm::command::types::LayerProperty,
};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, LayerBlendType, LayerFragmentShader,
    LayerShaderOutputKind, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{
        RenderClone,
        buffer::VertexSource,
        texture::{DepthStencilTarget, TextureSource, TextureTarget},
        vertices::PosTexVertex,
    },
};

use crate::{
    layer::{
        DrawableLayer, Layer, LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::{
        PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, render_texture_holder::RenderTextureHolder,
        top_left_projection_matrix,
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

pub trait NewDrawableLayerNeedsSeparatePass {
    fn needs_separate_pass(&self, #[expect(unused)] props: &LayerProperties) -> bool {
        false
    }
}

pub trait NewDrawableLayer: NewDrawableLayerNeedsSeparatePass {
    /// Renders the layer into `target` so effects can be applied to it, as the original's slot-16 methods do (e.g.
    /// `shin::PictureLayer` at `0x71000c6040`): clear to transparent, then draw with neutral parameters. The layer's
    /// own colour, alpha, blend type and fragment shader are applied once, when the texture is composited
    /// ([`NewDrawableLayerState::try_finish_indirect_render`]).
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "NewDrawableLayer/indirect");
        pass.clear(Some(UnormColor(0)), Some(0), None);

        if props.is_visible() {
            let self_transform = props.get_composed_transform_params(transform);
            let neutral = DrawableParams {
                color_multiplier: FloatColor4::WHITE,
                blend_type: LayerBlendType::Type1,
                fragment_shader: LayerFragmentShader::Default,
                shader_param: Vec4::ONE,
            };
            let clip = props.get_clip_params();
            // the original draws opaque and transparent blocks in one go; two passes give the same image
            for pass_kind in [PassKind::Opaque, PassKind::Transparent] {
                self.render_drawable_direct(&mut pass, &self_transform, &neutral, &clip, 1, pass_kind);
            }
        }

        // shortcut: always composited in the transparent pass; the original uses the opaque pass when an opaque
        // picture covers the whole screen (0x7100072c80), which only saves overdraw
        PassKind::Transparent
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        // TODO: make a strong type for this?
        stencil_ref: u8,
        pass_kind: PassKind,
    );

    /// Called by [`NewDrawableLayerWrapper`] after its properties were updated, for layers driven by them (e.g. rain)
    fn update_from_props(&mut self, _props: &LayerProperties) {}
}

pub trait NewDrawableLayerFastForward {
    fn fast_forward(&mut self);
}

/// The second texture effects render into before swapping it with the layer's texture, made on first use
fn scratch_texture<'a>(slot: &'a mut Option<RenderTexture>, context: &PreRenderContext) -> &'a mut RenderTexture {
    slot.get_or_insert_with(|| {
        context.new_render_texture("NewDrawableLayerState/render_texture_target".to_string())
    })
}

pub struct PrerenderedDrawable<'a> {
    pub render_texture: TextureSource<'a>,
    pub target_pass: PassKind,
}

#[derive(Debug, RenderClone)]
pub struct NewDrawableLayerState {
    #[render_clone(needs_render)]
    render_texture_src: RenderTextureHolder,
    // it's needed for any kind of effect. we just don't have them implemented yet
    #[render_clone(needs_render)]
    render_texture_target: Option<RenderTexture>,
    #[render_clone(needs_render)]
    render_texture_prev_frame: Option<RenderTexture>,
    target_pass: PassKind,
    /// Seeds the dissolve pieces' random directions, fixed for the layer's lifetime so they don't flicker
    dissolve_seed: u32,
    /// Running phase of the ripple waves (the original's `+0x54`)
    ripple_phase: f32,
    /// Running phases of the horizontal and vertical raster waves (the original's `+0x4c` and `+0x50`)
    raster_phase: Vec2,
}

impl NewDrawableLayerState {
    pub fn new() -> Self {
        Self {
            render_texture_src: RenderTextureHolder::new("NewDrawableLayerState/render_texture"),
            render_texture_target: None,
            render_texture_prev_frame: None,
            target_pass: PassKind::Transparent,
            // shortcut: the original reads its seed from the layer (+0x58), whose source is not identified yet;
            // any per-layer value gives the same kind of picture
            dissolve_seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.subsec_nanos()),
            ripple_phase: 0.0,
            raster_phase: Vec2::ZERO,
        }
    }

    pub fn get_prerendered_tex(&self) -> Option<PrerenderedDrawable> {
        let tex = self.render_texture_src.get()?;

        Some(PrerenderedDrawable {
            render_texture: tex.as_texture_source(),
            target_pass: self.target_pass,
        })
    }

    /// Advances the effects' running phases as the original's `0x7100078780` does: one unit per period, in ticks
    pub fn update(&mut self, context: &AdvUpdateContext, props: &LayerProperties) {
        let advance = |phase: &mut f32, period: LayerProperty| {
            let period = props.get_value(period);
            if period != 0.0 {
                *phase += context.delta_ticks.as_f32() / period;
            }
        };
        advance(&mut self.ripple_phase, LayerProperty::RippleTPeriod);
        advance(&mut self.raster_phase.x, LayerProperty::RasterHorizontalTPeriod);
        advance(&mut self.raster_phase.y, LayerProperty::RasterVerticalTPeriod);
    }

    pub fn is_rendered_opaquely<T: NewDrawableLayerNeedsSeparatePass>(
        &self,
        props: &LayerProperties,
        delegate: &T,
    ) -> bool {
        let Some(tex) = self.get_prerendered_tex() else {
            return true;
        };

        if delegate.needs_separate_pass(props) {
            // weird! I think this is too conservative
            return false;
        }

        tex.target_pass == PassKind::Opaque
    }

    pub fn pre_render<T: NewDrawableLayer>(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        delegate: &mut T,
        transform: &TransformParams,
    ) {
        if !props.is_visible() {
            return;
        }

        let blur_radius = props.get_value(LayerProperty::BlurRadius) * 0.001;
        let prop70 = props.get_value(LayerProperty::Prop70) * 0.001;
        let mosaic_size = props.get_value(LayerProperty::MosaicSize) as i32;
        let raster_horizontal_amplitude = props.get_value(LayerProperty::RasterHorizontalAmplitude);
        let raster_vertical_amplitude = props.get_value(LayerProperty::RasterVerticalAmplitude);
        let ripple_amplitude = props.get_value(LayerProperty::RippleAmplitude);
        let dissolve_intensity = props.get_value(LayerProperty::DissolveIntensity) * 0.001;
        let ghosting_alpha = props.get_value(LayerProperty::GhostingAlpha) * 0.001;

        if blur_radius.abs() < f32::EPSILON
            && prop70 < f32::EPSILON
            && mosaic_size <= 0
            && raster_horizontal_amplitude.abs() < f32::EPSILON
            && raster_vertical_amplitude.abs() < f32::EPSILON
            && ripple_amplitude.abs() < f32::EPSILON
            && dissolve_intensity <= 0.0
            && ghosting_alpha <= 0.0
            && !delegate.needs_separate_pass(props)
        {
            self.render_texture_src.clear();
            self.render_texture_target = None;
            self.render_texture_prev_frame = None;
            return;
        }

        if ghosting_alpha <= 0.0 {
            self.render_texture_prev_frame = None;
        } else {
            std::mem::swap(
                &mut self.render_texture_prev_frame,
                self.render_texture_src.as_inner_mut(),
            );
        }

        let render_texture_src = self.render_texture_src.get_or_init(context);
        self.target_pass = delegate.render_drawable_indirect(
            context,
            props,
            render_texture_src.as_texture_target(),
            context.depth_stencil,
            transform,
        );

        if blur_radius.abs() >= f32::EPSILON {
            let scratch = scratch_texture(&mut self.render_texture_target, context);
            effect_passes::apply_blur(context, render_texture_src, scratch, blur_radius);
        }
        if prop70 >= f32::EPSILON {
            todo!()
        }
        if mosaic_size > 0 {
            let target = scratch_texture(&mut self.render_texture_target, context);
            effect_passes::apply_mosaic(context, render_texture_src, target, mosaic_size);
            std::mem::swap(render_texture_src, target);
        }
        if raster_horizontal_amplitude.abs() >= f32::EPSILON
            || raster_vertical_amplitude.abs() >= f32::EPSILON
        {
            let target = scratch_texture(&mut self.render_texture_target, context);
            effect_passes::apply_raster(
                context,
                render_texture_src,
                target,
                Vec2::new(raster_horizontal_amplitude, raster_vertical_amplitude),
                Vec2::new(
                    props.get_value(LayerProperty::RasterHorizontalLPeriod),
                    props.get_value(LayerProperty::RasterVerticalLPeriod),
                ),
                self.raster_phase,
            );
            std::mem::swap(render_texture_src, target);
        }
        if ripple_amplitude.abs() >= f32::EPSILON {
            let target = scratch_texture(&mut self.render_texture_target, context);
            effect_passes::apply_ripple(
                context,
                render_texture_src,
                target,
                ripple_amplitude,
                props.get_value(LayerProperty::RippleLPeriod),
                self.ripple_phase,
            );
            std::mem::swap(render_texture_src, target);
        }
        if dissolve_intensity > 0.0 {
            let target = scratch_texture(&mut self.render_texture_target, context);
            effect_passes::apply_dissolve(
                context,
                render_texture_src,
                target,
                dissolve_intensity,
                props.get_value(LayerProperty::DissolveMode) as u32,
                self.dissolve_seed,
            );
            std::mem::swap(render_texture_src, target);
            // the pieces leave holes
            self.target_pass = PassKind::Transparent;
        }
        match (ghosting_alpha > 0.0, &mut self.render_texture_prev_frame) {
            (true, Some(render_texture_prev_frame)) => {
                effect_passes::apply_ghosting(
                    context,
                    props,
                    render_texture_src,
                    render_texture_prev_frame,
                    ghosting_alpha,
                );
            }
            _ => self.render_texture_prev_frame = None,
        }
    }

    pub fn try_finish_indirect_render(
        &self,
        props: &LayerProperties,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) -> bool {
        let Some(tex) = self.render_texture_src.get() else {
            return false;
        };

        if pass_kind != self.target_pass {
            return true;
        }

        let color_multiplier = props.get_color_multiplier().premultiply();
        let blend_type = props.get_blend_type();
        let fragment_shader = props.get_fragment_shader();
        let fragment_shader_param = props.get_fragment_shader_param();

        // NOTE: the transform is actually used just for clipping
        // we still compute it just in case
        let _self_transform = props.get_composed_transform_params(transform);

        let clip_params = props.get_clip_params();
        assert_eq!(clip_params.mode, DrawableClipMode::None);

        let transform = top_left_projection_matrix();

        let vertices = &build_quad_vertices(|t| PosTexVertex {
            position: t * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: t,
        });

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
                            ..Default::default()
                        },
                        stencil_reference: stencil_ref,
                    },
                })
                .color_blend_type(match pass_kind {
                    PassKind::Opaque => ColorBlendType::Opaque,
                    PassKind::Transparent => ColorBlendType::from_premultiplied_layer(blend_type),
                })
                .build(
                    RenderProgramWithArguments::Layer {
                        output_kind: match pass_kind {
                            PassKind::Opaque => LayerShaderOutputKind::Layer,
                            PassKind::Transparent => LayerShaderOutputKind::LayerDiscard,
                        },
                        fragment_shader,
                        vertices: VertexSource::VertexData { vertices },
                        texture: tex.as_texture_source(),
                        transform,
                        color_multiplier,
                        fragment_shader_param,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );

        true
    }

    #[tracing::instrument(skip_all)]
    pub fn render<T: NewDrawableLayer>(
        &self,
        props: &LayerProperties,
        delegate: &T,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if !props.is_visible() {
            return;
        }

        // an effect rendered the layer to a texture in pre_render: draw that instead
        if self.try_finish_indirect_render(props, pass, transform, stencil_ref, pass_kind) {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);

        let drawable = props.get_drawable_params();
        let clip = props.get_clip_params();

        delegate.render_drawable_direct(
            pass,
            &self_transform,
            &drawable,
            &clip,
            stencil_ref,
            pass_kind,
        );
    }
}

// packages NewDrawableLayerState and LayerProperties to implement simple NewDrawable-based layers
#[derive(Debug, RenderClone)]
pub struct NewDrawableLayerWrapper<T> {
    #[render_clone(needs_render)]
    inner_layer: T,
    #[render_clone(needs_render)]
    state: NewDrawableLayerState,
    props: LayerProperties,
}

impl<T: NewDrawableLayer> NewDrawableLayerWrapper<T> {
    pub fn from_inner(inner_layer: T) -> Self {
        Self {
            inner_layer,
            state: NewDrawableLayerState::new(),
            props: LayerProperties::new(),
        }
    }

    pub fn inner_ref(&self) -> &T {
        &self.inner_layer
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner_layer
    }
}

impl<T: AdvUpdatable + NewDrawableLayer> AdvUpdatable for NewDrawableLayerWrapper<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.inner_layer.update(context);
        self.state.update(context, &self.props);
        self.props.update(context);
        self.inner_layer.update_from_props(&self.props);
    }
}

impl<T: NewDrawableLayer + AdvUpdatable + NewDrawableLayerFastForward> Layer
    for NewDrawableLayerWrapper<T>
{
    fn fast_forward(&mut self) {
        self.props.fast_forward();
        self.inner_layer.fast_forward();
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.state
            .pre_render(context, &self.props, &mut self.inner_layer, transform);
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.state.render(
            &self.props,
            &self.inner_layer,
            pass,
            transform,
            stencil_ref,
            pass_kind,
        );
    }
}

impl<T: NewDrawableLayer + AdvUpdatable + NewDrawableLayerFastForward> DrawableLayer
    for NewDrawableLayerWrapper<T>
{
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}
