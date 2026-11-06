use std::{cell::RefCell, time::Instant};

use bevy_utils::HashMap;
use egui::{
    ahash::HashMapExt, style::WidgetVisuals, ClippedPrimitive, CollapsingHeader, Color32, Context,
    CornerRadius, FontFamily, FontId, InnerResponse, Pos2, Rect, Stroke, TextureId, Ui, ViewportId,
    ViewportIdMap,
};
use egui_wgpu::{Renderer, ScreenDescriptor};
use shin_input::{inputs::MouseButton, RawInputState};

/// The debug overlays (`--dev`), drawn with egui over the finished frame; the app toggles the window with F3
pub struct OverlayManager {
    show_overlays_window: bool,
    start: Instant,
    /// The surface's size in pixels and the scale, as of the last update
    size_in_pixels: [u32; 2],
    pixels_per_point: f32,
    renderer: Renderer,
    context: Context,
    primitives: Vec<ClippedPrimitive>,
    free_textures: Vec<TextureId>,
    prev_input: RawInputState,
    storage: OverlayStateStorage,
}

impl OverlayManager {
    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let renderer = Renderer::new(device, texture_format, None, 1, false);
        let context = Context::default();

        let alpha = 128;

        let from_gray = |l: u8| Color32::from_rgba_unmultiplied(l, l, l, alpha);

        context.set_style(egui::Style {
            override_font_id: Some(FontId {
                size: 14.0,
                // TODO: use Fira Code
                family: FontFamily::Monospace,
            }),
            visuals: egui::Visuals {
                widgets: egui::style::Widgets {
                    noninteractive: WidgetVisuals {
                        bg_fill: from_gray(248),
                        weak_bg_fill: from_gray(248),
                        bg_stroke: Stroke::new(1.0, from_gray(190)), // separators, indentation lines
                        fg_stroke: Stroke::new(1.0, Color32::from_gray(180)), // normal text color
                        corner_radius: CornerRadius::same(2),
                        expansion: 0.0,
                    },
                    inactive: WidgetVisuals {
                        bg_fill: from_gray(230), // button background
                        weak_bg_fill: from_gray(230),
                        bg_stroke: Default::default(),
                        fg_stroke: Stroke::new(1.0, Color32::from_gray(180)), // button text
                        corner_radius: CornerRadius::same(2),
                        expansion: 0.0,
                    },
                    hovered: WidgetVisuals {
                        bg_fill: from_gray(220),
                        weak_bg_fill: from_gray(220),
                        bg_stroke: Stroke::new(1.0, from_gray(105)), // e.g. hover over window edge or button
                        fg_stroke: Stroke::new(1.5, Color32::BLACK),
                        corner_radius: CornerRadius::same(3),
                        expansion: 1.0,
                    },
                    active: WidgetVisuals {
                        bg_fill: from_gray(165),
                        weak_bg_fill: from_gray(165),
                        bg_stroke: Stroke::new(1.0, from_gray(255)),
                        fg_stroke: Stroke::new(2.0, Color32::BLACK),
                        corner_radius: CornerRadius::same(2),
                        expansion: 1.0,
                    },
                    open: WidgetVisuals {
                        bg_fill: from_gray(220),
                        weak_bg_fill: from_gray(220),
                        bg_stroke: Stroke::new(1.0, from_gray(160)),
                        fg_stroke: Stroke::new(1.0, Color32::BLACK),
                        corner_radius: CornerRadius::same(2),
                        expansion: 0.0,
                    },
                },
                window_fill: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                panel_fill: from_gray(248),
                ..egui::Visuals::light()
            },
            ..Default::default()
        });

        Self {
            show_overlays_window: false,
            start: Instant::now(),
            size_in_pixels: [1, 1],
            pixels_per_point: 1.0,
            renderer,
            context,
            primitives: Vec::new(),
            free_textures: Vec::new(),
            prev_input: RawInputState::new(),
            storage: OverlayStateStorage::new(),
        }
    }

    pub fn toggle_window(&mut self) {
        self.show_overlays_window = !self.show_overlays_window;
    }

    fn screen_descriptor(&self) -> ScreenDescriptor {
        ScreenDescriptor {
            size_in_pixels: self.size_in_pixels,
            pixels_per_point: self.pixels_per_point,
        }
    }

    /// `window_size` in physical pixels, `pixels_per_point` the window's scale factor
    pub fn start_update(
        &mut self,
        raw_input_state: &RawInputState,
        window_size: (u32, u32),
        pixels_per_point: f32,
    ) {
        for id in self.free_textures.drain(..) {
            self.renderer.free_texture(&id);
        }

        self.size_in_pixels = [window_size.0.max(1), window_size.1.max(1)];
        self.pixels_per_point = pixels_per_point;

        let mut events = Vec::new();

        // winit gives the cursor in physical pixels, egui wants points
        let mouse_pos = Pos2::new(
            raw_input_state.mouse.position.x / pixels_per_point,
            raw_input_state.mouse.position.y / pixels_per_point,
        );

        events.push(egui::Event::PointerMoved(mouse_pos));
        events.extend(
            self.prev_input
                .mouse
                .buttons
                .iter()
                .zip(raw_input_state.mouse.buttons.values())
                .filter_map(|((button, &prev), &new)| {
                    if prev != new {
                        Some(egui::Event::PointerButton {
                            pos: mouse_pos,
                            button: match button {
                                MouseButton::Left => egui::PointerButton::Primary,
                                MouseButton::Right => egui::PointerButton::Secondary,
                                MouseButton::Middle => egui::PointerButton::Middle,
                                _ => return None,
                            },
                            pressed: new,
                            // TODO: modifiers support
                            modifiers: Default::default(),
                        })
                    } else {
                        None
                    }
                }),
        );

        let viewport = egui::ViewportInfo {
            parent: None,
            title: None,
            events: vec![],
            native_pixels_per_point: Some(pixels_per_point),
            monitor_size: None,
            inner_rect: None,
            outer_rect: None,
            minimized: None,
            maximized: None,
            fullscreen: None,
            focused: None,
        };

        let mut viewports = ViewportIdMap::new();
        viewports.insert(ViewportId::ROOT, viewport);

        let raw_input = egui::RawInput {
            viewport_id: ViewportId::ROOT,
            viewports,
            screen_rect: Some(Rect::from_min_max(
                Pos2::default(),
                Pos2::new(
                    window_size.0 as f32 / pixels_per_point,
                    window_size.1 as f32 / pixels_per_point,
                ),
            )),
            max_texture_side: None,
            time: Some(self.start.elapsed().as_secs_f64()),
            predicted_dt: 0.0,
            modifiers: Default::default(),
            events,
            hovered_files: vec![],
            dropped_files: vec![],
            focused: false,
            system_theme: None,
        };

        self.prev_input = raw_input_state.clone();

        self.context.begin_pass(raw_input);
    }

    /// Visit overlays and show them
    /// This method should be called exactly once after `start_update` and before `end_update`
    pub fn visit_overlays(&mut self, visit_fn: impl FnOnce(&mut OverlayCollector)) {
        let ctx = &self.context;

        egui::Area::new(egui::Id::new("top-left")).show(ctx, |top_left| {
            // the area is as narrow as its content, so wrapping would break every label at its first space
            top_left.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let mut visit_fn = Some(visit_fn);

            let window_shown = if self.show_overlays_window {
                let result = egui::Window::new("Overlays")
                    .resizable(false)
                    // out of the way of the labels in the top left, which grow with the overlays shown
                    .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
                    .show(ctx, |ui| {
                        let mut collector = OverlayCollector {
                            ctx,
                            top_left,
                            ui: Some(ui),
                            storage: &mut self.storage,
                        };

                        visit_fn.take().unwrap()(&mut collector);
                    });
                !matches!(result, Some(InnerResponse { inner: None, .. }))
            } else {
                false
            };
            if !window_shown {
                // if the window is closed, we still want to visit the visitable, just without a ui
                let mut collector = OverlayCollector {
                    ctx,
                    top_left,
                    ui: None,
                    storage: &mut self.storage,
                };

                visit_fn.take().unwrap()(&mut collector);
            }
        });
    }

    pub fn finish_update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        // yes, we can mutate the input state
        // this is needed to consume the mouse events
        raw_input_state: &mut RawInputState,
    ) {
        let ctx = &self.context;
        let full_output = self.context.end_pass();

        // consume mouse events if egui wants them
        if ctx.wants_pointer_input() {
            raw_input_state
                .mouse
                .buttons
                .values_mut()
                .for_each(|v| *v = false);
            raw_input_state.mouse.scroll_amount = 0.0;
        }

        // TODO: handle platform outputs or smth

        self.primitives = ctx.tessellate(full_output.shapes, self.pixels_per_point);

        // update the textures as requested
        for (id, tex) in full_output.textures_delta.set {
            self.renderer.update_texture(device, queue, id, &tex);
        }
        // freed after this frame has drawn with them
        self.free_textures.extend(full_output.textures_delta.free);
    }

    /// Draws the overlays over `target` (the whole surface, outside the game's letterbox too)
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let screen = self.screen_descriptor();
        // the returned command buffers are for paint callbacks, which the overlays do not use
        let _ = self
            .renderer
            .update_buffers(device, queue, encoder, &self.primitives, &screen);
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        self.renderer.render(&mut pass, &self.primitives, &screen);
    }
}

struct OverlayStateStorage {
    overlays: HashMap<String, bool>,
    subgroups: HashMap<String, RefCell<OverlayStateStorage>>,
}

// TODO: we should learn to persist the overlay states
impl OverlayStateStorage {
    pub fn new() -> Self {
        Self {
            overlays: HashMap::new(),
            subgroups: HashMap::new(),
        }
    }
}

pub struct OverlayCollector<'a, 'top_left, 'ctx> {
    ctx: &'ctx Context,
    top_left: &'top_left mut Ui,
    ui: Option<&'a mut Ui>,
    storage: &'a mut OverlayStateStorage,
}

impl OverlayCollector<'_, '_, '_> {
    pub fn overlay(&mut self, name: &str, content: impl FnOnce(&Context, &mut Ui), default: bool) {
        let state = self
            .storage
            .overlays
            .entry(name.to_string())
            .or_insert(default);
        if let Some(ui) = self.ui.as_mut() {
            ui.checkbox(state, name);
        }
        if *state {
            content(self.ctx, self.top_left);
        }
    }

    pub fn subgroup(
        &mut self,
        name: &str,
        content: impl FnOnce(&mut OverlayCollector),
        default_open: bool,
    ) {
        let state = self
            .storage
            .subgroups
            .entry(name.to_string())
            .or_insert_with(|| RefCell::new(OverlayStateStorage::new()));

        if let Some(ui) = self.ui.as_mut() {
            CollapsingHeader::new(name)
                .default_open(default_open)
                .show(ui, |ui| {
                    content(&mut OverlayCollector {
                        ctx: self.ctx,
                        top_left: self.top_left,
                        ui: Some(ui),
                        storage: &mut state.borrow_mut(),
                    });
                });
        } else {
            content(&mut OverlayCollector {
                ctx: self.ctx,
                top_left: self.top_left,
                ui: None,
                storage: &mut state.borrow_mut(),
            });
        }
    }
}

pub trait OverlayVisitable {
    fn visit_overlay(&self, collector: &mut OverlayCollector);
}
