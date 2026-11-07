//! The window and the frame loop: a winit event loop, wgpu set up on the window's surface, and the [`ShinApp`] trait
//! the game implements (`shin/src/app.rs`).
//!
//! Each redraw (`RedrawRequested`): wait for the frame cap if one is set; turn the input gathered since the last
//! frame (winit events, gilrs gamepad events) into a [`RawInputState`], let the app filter it
//! ([`ShinApp::filter_input`]), lower it to the app's actions and map the mouse onto the 1920x1080 canvas; call
//! [`ShinApp::update`] with the elapsed time and an update command encoder (offscreen passes go there); apply new
//! [`DisplaySettings`] (window mode and size, V-Sync, frame cap, supersampling); call [`ShinApp::render`] into the
//! canvas, which is the window's largest 16:9 area, or with supersampling a larger texture filtered down into it by
//! four bilinear taps; draw [`ShinApp::render_overlay`] over the surface; submit the dynamic buffer, update and render
//! encoders together and present. Everything runs on the main thread. The architecture is described in
//! `research/docs/architecture.md`, the port-only display settings in `research/docs/engine-internals.md` §18.

use std::{
    cell::Cell,
    fmt::Debug,
    sync::Arc,
    time::{Duration, Instant},
};

use cfg_if::cfg_if;
use derive_where::derive_where;
use enum_map::EnumMap;
use shin_input::{Action, ActionState, ActionsState, Pointer, RawInputAccumulator, RawInputState};
use shin_render::{
    init::{RenderResources, WgpuInitResult, WgpuResources},
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{CanvasSize, ResizeHandle, SurfaceResizeSource, SurfaceSize, ViewportParams},
    shaders::types::texture::{DepthStencilTarget, TextureTarget, TextureTargetKind},
};
use shin_tasks::AsyncTask;
use tracing::{info, info_span, warn};
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopClosed, EventLoopProxy},
    window::{Window, WindowId},
};

pub struct AppContext<'a, A: ShinApp> {
    pub event_loop: &'a ActiveEventLoop,
    pub event_loop_proxy: &'a ShinEventLoopProxy<A::EventType>,
    pub winit: &'a mut WindowState,
    pub wgpu: &'a WgpuResources,
    pub render: &'a mut RenderResources,
}

pub struct RenderContext<'a> {
    pub winit: &'a WindowState,
    pub wgpu: &'a WgpuResources,
}

pub struct AppContextOwned<A: ShinApp> {
    pub event_loop_proxy: ShinEventLoopProxy<A::EventType>,
    pub winit: WindowState,
    pub wgpu: WgpuResources,
    pub render: RenderResources,
}

/// How the window is shown
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,
    /// A borderless window covering the monitor
    Borderless,
    /// Exclusive fullscreen at the monitor's resolution and highest refresh rate
    Exclusive,
}

/// The window and frame settings the app asks for (the port's PC settings)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplaySettings {
    pub mode: WindowMode,
    /// The inner size in logical pixels while windowed
    pub size: (u32, u32),
    pub vsync: bool,
    /// Frames per second at most; `None` for no cap
    pub frame_cap: Option<u32>,
    /// Supersampling: the canvas is rendered this many times larger and filtered down to the window
    pub render_scale: f32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            mode: WindowMode::Windowed,
            size: (1920, 1080),
            vsync: true,
            frame_cap: None,
            render_scale: 1.0,
        }
    }
}

pub trait ShinApp: Sized {
    type Parameters;
    type EventType: Send + Debug + 'static;
    type ActionType: Action;

    fn init(context: AppContext<Self>, parameters: Self::Parameters) -> anyhow::Result<Self>;

    /// The display settings the window is created with
    fn initial_display(_parameters: &Self::Parameters) -> DisplaySettings {
        DisplaySettings::default()
    }

    /// The display settings wanted now, applied whenever they change; `None` keeps the current ones
    fn display(&self) -> Option<DisplaySettings> {
        None
    }

    fn map_canvas_size(window_size: PhysicalSize<u32>) -> ViewportParams {
        ViewportParams::with_aspect_ratio(window_size, 16.0 / 9.0)
    }

    fn custom_event(&mut self, context: AppContext<Self>, event: Self::EventType);

    fn update(
        &mut self,
        context: AppContext<Self>,
        input: EnumMap<Self::ActionType, ActionState>,
        pointer: Pointer,
        elapsed_time: Duration,
        command_encoder: &mut wgpu::CommandEncoder,
    );
    // can't pass context here because `RenderPass` borrows a bunch of stuff from there
    // let's hope it won't be an issue ;)
    fn render(&mut self, context: RenderContext, pass: &mut RenderPass);

    /// Sees the frame's raw input before it becomes actions, so debug tools can take the clicks they use
    fn filter_input(&mut self, _context: AppContext<Self>, _input: &mut RawInputState) {}

    /// Draws over the finished frame (debug overlays), after `render`'s pass has ended
    fn render_overlay(
        &mut self,
        _context: RenderContext,
        _encoder: &mut wgpu::CommandEncoder,
        _target: &wgpu::TextureView,
    ) {
    }

    /// The window is being closed
    fn exiting(&mut self) {}
}

#[derive_where(Clone)]
pub struct ShinEventLoopProxy<E: Send + Debug + 'static> {
    proxy: EventLoopProxy<ShinAppEventImpl<E>>,
}

impl<E: Send + Debug + 'static> ShinEventLoopProxy<E> {
    pub fn send_event(&self, event: E) -> Result<(), EventLoopClosed<E>> {
        self.proxy
            .send_event(ShinAppEventImpl::Custom(event))
            .map_err(|e| {
                let EventLoopClosed(ShinAppEventImpl::Custom(e)) = e else {
                    unreachable!()
                };
                EventLoopClosed(e)
            })
    }
}

pub struct WindowState {
    pub window: Arc<Window>,
    pub resize_source: SurfaceResizeSource,
    /// The display settings in effect
    display: DisplaySettings,
    /// The largest texture the device takes, which bounds the supersampled canvas
    max_texture_size: u32,
    /// Port-only: when the mouse last moved, clicked or scrolled, and whether the cursor is hidden for idling
    last_mouse: Cell<Instant>,
    cursor_hidden: Cell<bool>,
}

/// How long the mouse may sit still over the window before the cursor hides
const CURSOR_IDLE: Duration = Duration::from_secs(3);

impl WindowState {
    /// The mouse moved, clicked or scrolled: the cursor shows again and the idle time restarts
    fn mouse_used(&self) {
        self.last_mouse.set(Instant::now());
        if self.cursor_hidden.replace(false) {
            self.window.set_cursor_visible(true);
        }
    }

    /// Hides the cursor once the mouse has been still for `CURSOR_IDLE`
    fn hide_idle_cursor(&self) {
        if !self.cursor_hidden.get() && self.last_mouse.get().elapsed() >= CURSOR_IDLE {
            self.window.set_cursor_visible(false);
            self.cursor_hidden.set(true);
        }
    }

    pub fn new<A: ShinApp>(event_loop: &ActiveEventLoop, display: DisplaySettings) -> Self {
        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes();

        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            use winit::dpi::{LogicalPosition, LogicalSize};
            attributes = attributes
                .with_inner_size(LogicalSize::new(display.size.0, display.size.1))
                .with_maximized(false)
                .with_position(LogicalPosition::new(1080, 0));
        }

        #[cfg(target_arch = "wasm32")]
        {
            use anyhow::Context;
            use web_sys::wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes = web_sys::window()
                .context("Couldn't get window")
                .and_then(|win| win.document().context("Couldn't get document"))
                .and_then(|doc| {
                    let canvas = doc
                        .get_element_by_id("canvas")
                        .context("Couldn't find the canvas element element")?
                        .dyn_into::<web_sys::HtmlCanvasElement>()
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Couldn't cast the element to a canvas element: {:?}",
                                e
                            )
                        })
                        .context("Couldn't cast the element to a canvas element")?;

                    Ok(attributes.with_canvas(Some(canvas)))
                })
                .expect("Couldn't attach canvas to winit window");
        }

        let window = Arc::new(event_loop.create_window(attributes).unwrap());

        let state = Self {
            resize_source: SurfaceResizeSource::new(ViewportParams::default()),
            window,
            display,
            max_texture_size: 4096,
            last_mouse: Cell::new(Instant::now()),
            cursor_hidden: Cell::new(false),
        };
        if display.mode != WindowMode::Windowed {
            state.apply_mode();
        }
        let size = state.viewport::<A>(state.window.inner_size());
        info!("Created a window with size: {:?}", size);
        state.resize_source.resize(size);
        state
    }

    /// The display settings in effect
    pub fn display(&self) -> DisplaySettings {
        self.display
    }

    /// The canvas for a window of `size`, rendered at the supersampling scale as far as the device allows
    fn viewport<A: ShinApp>(&self, size: PhysicalSize<u32>) -> ViewportParams {
        let params = A::map_canvas_size(size);
        let max = self.max_texture_size as f32;
        let shown = params.canvas_display_size;
        let scale = self
            .display
            .render_scale
            .min(max / shown.width.max(1) as f32)
            .min(max / shown.height.max(1) as f32)
            .max(1.0);
        params.with_render_scale(scale)
    }

    /// Switches to new display settings; the caller applies the vsync to the surface
    fn apply_display<A: ShinApp>(&mut self, display: DisplaySettings) {
        let old = std::mem::replace(&mut self.display, display);
        info!("Display settings: {:?}", self.display);
        if old.mode != display.mode
            || (display.mode == WindowMode::Windowed && old.size != display.size)
        {
            self.apply_mode();
        }
        if old.render_scale != display.render_scale {
            self.resize_source
                .resize(self.viewport::<A>(self.window.inner_size()));
        }
    }

    /// Puts the window in the mode (and the size when windowed) of the display settings
    fn apply_mode(&self) {
        use winit::{dpi::LogicalSize, window::Fullscreen};
        let window = &self.window;
        match self.display.mode {
            WindowMode::Windowed => {
                window.set_fullscreen(None);
                let (width, height) = self.display.size;
                let _ = window.request_inner_size(LogicalSize::new(width, height));
            }
            WindowMode::Borderless => {
                window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
            }
            WindowMode::Exclusive => {
                // the monitor's own resolution at its highest refresh rate
                let mode = window.current_monitor().and_then(|monitor| {
                    let size = monitor.size();
                    monitor
                        .video_modes()
                        .filter(|mode| mode.size() == size)
                        .max_by_key(|mode| (mode.refresh_rate_millihertz(), mode.bit_depth()))
                });
                match mode {
                    Some(mode) => {
                        info!("Entering exclusive fullscreen mode {}", mode);
                        window.set_fullscreen(Some(Fullscreen::Exclusive(mode)));
                    }
                    None => {
                        warn!("No video mode for exclusive fullscreen; using a borderless window");
                        window
                            .set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
                    }
                }
            }
        }
    }

    pub fn toggle_fullscreen(&self) {
        let window = &self.window;

        if window.fullscreen().is_some() {
            info!("Exiting fullscreen mode");
            window.set_fullscreen(None);
        } else if let Some(monitor) = window.current_monitor() {
            if let Some(video_mode) = monitor.video_modes().next() {
                info!(
                    "Attempting to enter exclusive fullscreen mode {}",
                    video_mode
                );
                window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
            }
            if window.fullscreen().is_none() {
                info!(
                    "Attempting to enter non-exclusive fullscreen mode on {}",
                    monitor
                        .name()
                        .unwrap_or_else(|| "unknown monitor".to_string())
                );
                window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(Some(monitor))));
            }
        }
    }
}

#[derive(Debug)]
enum ShinAppEventImpl<E> {
    #[allow(dead_code)]
    WgpuInitDone(anyhow::Result<WgpuInitResult<'static>>),
    Custom(E),
}

#[inline]
fn start_wgpu_init<A: ShinApp>(
    #[allow(unused)] event_loop: &ActiveEventLoop,
    proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
    raw_input_state: RawInputAccumulator,
    params: A::Parameters,
    winit: WindowState,
    // window: Arc<Window>,
    // surface_resize_handle: ResizeHandle<SurfaceSize>,
) -> WinitAppState<A> {
    let window = winit.window.clone();
    let surface_resize_handle = winit.resize_source.handle();

    cfg_if! {
        if #[cfg(not(windows))] {
            // async init
            // this is not necessarily required anywhere besides wasm (we can't do sync init there)
            // however if we have to choose async init is a bit nicer because we can continue handling events
            let proxy_clone = proxy.clone();
            let task = shin_tasks::async_io::spawn(async move {
                let result = shin_render::init::init_wgpu(window, surface_resize_handle, None).await;

                proxy_clone
                    .send_event(ShinAppEventImpl::WgpuInitDone(result))
                    .unwrap();
            });

            WinitAppState::WaitingForWgpuInit {
                proxy,
                raw_input_state,
                params,
                winit,
                task,
            }
        } else {
            // sync init
            // windows want accesses to window handles to be on the main thread, so we have to do this synchronously
            let result = shin_tasks::block_on(shin_render::init::init_wgpu(
                window,
                surface_resize_handle,
                None,
            ));
            finish_wgpu_init(event_loop, proxy, raw_input_state, params, winit, result)
        }
    }
}

#[inline]
fn finish_wgpu_init<A: ShinApp>(
    event_loop: &ActiveEventLoop,
    proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
    raw_input_state: RawInputAccumulator,
    params: A::Parameters,
    mut winit: WindowState,
    result: anyhow::Result<WgpuInitResult<'static>>,
) -> WinitAppState<A> {
    let wgpu = result.expect("wgpu init failed");

    // finish render init
    // the depth stencil should be the same size as the surface, even though it's a bit wasteful
    let (wgpu, mut render) = wgpu.into_resources(
        winit.resize_source.handle::<SurfaceSize>(),
        winit.resize_source.handle::<CanvasSize>(),
    );
    // the supersampled canvas is bounded by the device's texture size, known only now
    winit.max_texture_size = wgpu.device.limits().max_texture_dimension_2d;
    winit
        .resize_source
        .resize(winit.viewport::<A>(winit.window.inner_size()));
    if !winit.display.vsync {
        render.surface.set_vsync(false);
    }

    let shin_proxy = ShinEventLoopProxy {
        proxy: proxy.clone(),
    };

    let context = AppContext {
        event_loop,
        event_loop_proxy: &shin_proxy,
        winit: &mut winit,
        wgpu: &wgpu,
        render: &mut render,
    };

    let app = A::init(context, params)
        // TODO: report this error to the user better than just panicking
        .expect("App initialization failed");

    let context = AppContextOwned {
        event_loop_proxy: shin_proxy,
        winit,
        wgpu,
        render,
    };

    WinitAppState::Operational {
        proxy,
        last_update: Instant::now(),
        next_frame: Instant::now(),
        supersample: None,
        raw_input_state,
        input_state: ActionsState::new(),
        context,
        app,
    }
}

/// Waits until the frame cap's next frame is due, sleeping most of the way and spinning the rest for accuracy
fn wait_for_frame(next_frame: &mut Instant, fps: u32) {
    let period = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
    let margin = Duration::from_millis(1);
    let left = next_frame.saturating_duration_since(Instant::now());
    if left > margin {
        std::thread::sleep(left - margin);
    }
    while Instant::now() < *next_frame {
        std::thread::yield_now();
    }
    // on schedule, the next frame is one period on; a late frame does not make the following ones hurry
    *next_frame = (*next_frame + period).max(Instant::now());
}

/// Filters the supersampled canvas down to the window: four bilinear taps a quarter of a window pixel from its
/// centre, which average the canvas texels the pixel covers (exactly at 2x)
fn downsample(pass: &mut RenderPass, source: &RenderTexture, shown: CanvasSize) {
    use shin_render::{
        ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
        shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
    };
    // in uv units, where a window pixel is 1 / the shown size
    let (dx, dy) = (0.25 / shown.width as f32, 0.25 / shown.height as f32);
    let vertex = |x: f32, y: f32| PosTexVertex {
        position: glam::vec2(x * 2.0 - 1.0, 1.0 - y * 2.0),
        // render textures hold the canvas bottom row first
        texture_position: glam::vec2(x, 1.0 - y),
    };
    let vertices = [
        vertex(0.0, 0.0),
        vertex(1.0, 0.0),
        vertex(0.0, 1.0),
        vertex(1.0, 1.0),
    ];
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Opaque)
            .build(
                RenderProgramWithArguments::Blur {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: source.as_texture_source(),
                    transform: glam::Mat4::IDENTITY,
                    offsets: [glam::vec4(dx, dy, dx, -dy), glam::Vec4::ZERO],
                    weights: glam::vec4(0.5, 0.5, 0.0, 0.0),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

#[derive(Default)]
enum WinitAppState<A: ShinApp> {
    WaitingForInitialResume {
        // unfortunately we have to weave in the proxy from outside of the loop until winit 0.31: https://github.com/rust-windowing/winit/pull/3764
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
    },
    /// On web platform the initial winit canvas size is zero, so we have to wait for a non-zero size
    WaitingForNonzeroSize {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
        winit: WindowState,
        surface_resize_handle: ResizeHandle<SurfaceSize>,
    },
    #[allow(dead_code)]
    WaitingForWgpuInit {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        raw_input_state: RawInputAccumulator,
        params: A::Parameters,
        winit: WindowState,
        #[allow(unused)] // this is just to keep the task alive
        task: AsyncTask<()>,
    },
    Operational {
        proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>,
        last_update: Instant,
        /// When the frame cap lets the next frame start
        next_frame: Instant,
        /// The canvas rendered larger, while supersampling
        supersample: Option<RenderTexture>,
        raw_input_state: RawInputAccumulator,
        input_state: ActionsState<A::ActionType>,
        context: AppContextOwned<A>,
        app: A,
    },
    #[default]
    Poison,
}

impl<A: ShinApp> WinitAppState<A> {
    fn new(proxy: EventLoopProxy<ShinAppEventImpl<A::EventType>>, params: A::Parameters) -> Self {
        Self::WaitingForInitialResume {
            proxy,
            params,
            raw_input_state: RawInputAccumulator::new(),
        }
    }

    pub fn input_mut(&mut self) -> &mut RawInputAccumulator {
        match self {
            WinitAppState::WaitingForInitialResume {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::WaitingForNonzeroSize {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::WaitingForWgpuInit {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::Operational {
                raw_input_state: input_state,
                ..
            } => input_state,
            WinitAppState::Poison => {
                unreachable!()
            }
        }
    }

    pub fn winit(&self) -> Option<&WindowState> {
        match self {
            WinitAppState::WaitingForInitialResume { .. } => None,
            WinitAppState::WaitingForNonzeroSize { winit, .. } => Some(winit),
            WinitAppState::WaitingForWgpuInit { winit, .. } => Some(winit),
            WinitAppState::Operational { context, .. } => Some(&context.winit),
            WinitAppState::Poison => {
                unreachable!()
            }
        }
    }
}

impl<A: ShinApp> ApplicationHandler<ShinAppEventImpl<A::EventType>> for WinitAppState<A> {
    /// The app (and with it the window's surface) goes while the event loop still holds its connection to the display:
    /// `run_app` drops the event loop before returning, and on Wayland a swapchain destroyed after that crashes in
    /// libwayland
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        drop(std::mem::take(self));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match std::mem::take(self) {
            WinitAppState::WaitingForInitialResume {
                proxy,
                raw_input_state: input_state,
                params,
            } => {
                let winit = WindowState::new::<A>(event_loop, A::initial_display(&params));

                let mut surface_resize_handle = winit.resize_source.surface_handle();
                let current_size = surface_resize_handle.get();

                if current_size != SurfaceSize::default() {
                    *self = start_wgpu_init::<A>(event_loop, proxy, input_state, params, winit);
                } else {
                    *self = WinitAppState::WaitingForNonzeroSize {
                        proxy,
                        raw_input_state: input_state,
                        params,
                        winit,
                        surface_resize_handle,
                    };
                }
            }
            WinitAppState::WaitingForNonzeroSize { .. } => {
                // this probably shouldn't happen
                todo!()
            }
            WinitAppState::WaitingForWgpuInit { .. } => {
                // this shouldn't happen.. I think
                // TODO: figure out this out better when porting to Android or smth
                todo!()
            }
            WinitAppState::Operational {
                proxy,
                last_update,
                next_frame,
                supersample,
                raw_input_state,
                input_state,
                mut context,
                app,
            } => {
                context.render.surface = shin_render::init::surface_reinit(
                    &context.wgpu.instance,
                    context.wgpu.device.clone(),
                    context.winit.window.clone(),
                    context.winit.resize_source.handle::<SurfaceSize>(),
                    context.render.surface_texture_format,
                )
                .expect("surface reinit failed");
                if !context.winit.display.vsync {
                    context.render.surface.set_vsync(false);
                }

                *self = WinitAppState::Operational {
                    proxy,
                    last_update,
                    next_frame,
                    supersample,
                    raw_input_state,
                    input_state,
                    context,
                    app,
                };
            }
            WinitAppState::Poison => unreachable!(),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ShinAppEventImpl<A::EventType>) {
        match event {
            ShinAppEventImpl::WgpuInitDone(result) => {
                let WinitAppState::WaitingForWgpuInit {
                    proxy,
                    raw_input_state,
                    params,
                    winit,
                    task: _,
                } = std::mem::take(self)
                else {
                    unreachable!()
                };
                *self = finish_wgpu_init::<A>(
                    event_loop,
                    proxy,
                    raw_input_state,
                    params,
                    winit,
                    result,
                );
            }
            ShinAppEventImpl::Custom(e) => {
                let WinitAppState::Operational {
                    proxy: _,
                    last_update: _,
                    next_frame: _,
                    supersample: _,
                    raw_input_state: _,
                    input_state: _,
                    context:
                        AppContextOwned {
                            event_loop_proxy,
                            winit,
                            wgpu,
                            render,
                        },
                    app,
                } = self
                else {
                    warn!("Received custom event before app was initialized");
                    return;
                };

                app.custom_event(
                    AppContext {
                        event_loop,
                        event_loop_proxy,
                        wgpu,
                        winit,
                        render,
                    },
                    e,
                );
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.input_mut().on_winit_event(&event);

        let Some(winit) = self.winit() else {
            return;
        };

        if window_id != winit.window.id() {
            return;
        }
        if matches!(
            event,
            WindowEvent::CursorMoved { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::MouseWheel { .. }
        ) {
            winit.mouse_used();
        }

        match event {
            WindowEvent::CloseRequested => {
                if let WinitAppState::Operational { app, .. } = self {
                    app.exiting();
                }
                event_loop.exit()
            }
            WindowEvent::Resized(physical_size) => {
                winit
                    .resize_source
                    .resize(winit.viewport::<A>(physical_size));
                winit.window.request_redraw();

                // this is a mouthful...
                match std::mem::take(self) {
                    WinitAppState::WaitingForNonzeroSize {
                        proxy,
                        raw_input_state: input_state,
                        params,
                        mut surface_resize_handle,
                        winit,
                    } => {
                        if surface_resize_handle.get() != SurfaceSize::default() {
                            *self =
                                start_wgpu_init::<A>(event_loop, proxy, input_state, params, winit);
                        } else {
                            *self = WinitAppState::WaitingForNonzeroSize {
                                proxy,
                                raw_input_state: input_state,
                                params,
                                winit,
                                surface_resize_handle,
                            };
                        }
                    }
                    state => *self = state,
                }
            }
            WindowEvent::RedrawRequested => {
                let _span = info_span!("redraw").entered();

                let WinitAppState::Operational {
                    proxy: _,
                    last_update,
                    next_frame,
                    supersample,
                    raw_input_state,
                    input_state,
                    context:
                        AppContextOwned {
                            event_loop_proxy,
                            winit,
                            wgpu,
                            render,
                        },
                    app,
                } = self
                else {
                    return;
                };

                winit.hide_idle_cursor();
                if let Some(fps) = winit.display.frame_cap {
                    info_span!("frame_cap").in_scope(|| wait_for_frame(next_frame, fps));
                }
                let now_update = Instant::now();
                let elapsed = now_update.duration_since(*last_update);
                *last_update = now_update;

                let mut raw_instantaneous_input_state = raw_input_state.start_frame();
                app.filter_input(
                    AppContext {
                        event_loop,
                        event_loop_proxy,
                        winit,
                        wgpu,
                        render,
                    },
                    &mut raw_instantaneous_input_state,
                );
                let instantaneous_input_state =
                    A::ActionType::lower(&raw_instantaneous_input_state);
                // NOTE: this interface does not expose analog stick positions, as well as mouse wheel (and buttons...)
                // these should probably be passed directly or some other abstraction should be devised
                let input_state = input_state.update(instantaneous_input_state, elapsed);
                let pointer = {
                    let viewport = A::map_canvas_size(winit.window.inner_size());
                    let mouse = &raw_instantaneous_input_state.mouse;
                    let offset = glam::vec2(
                        viewport.canvas_offset.x as f32,
                        viewport.canvas_offset.y as f32,
                    );
                    let size = glam::vec2(
                        viewport.canvas_size.width as f32,
                        viewport.canvas_size.height as f32,
                    );
                    let canvas = (mouse.position - offset) / size;
                    Pointer {
                        position: (canvas.cmpge(glam::Vec2::ZERO).all()
                            && canvas.cmplt(glam::Vec2::ONE).all())
                        .then(|| canvas * glam::vec2(1920.0, 1080.0)),
                        held: mouse.buttons[shin_input::inputs::MouseButton::Left],
                    }
                };

                let mut update_encoder =
                    wgpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Update"),
                        });

                info_span!("update").in_scope(|| {
                    app.update(
                        AppContext {
                            event_loop,
                            event_loop_proxy,
                            winit,
                            wgpu,
                            render,
                        },
                        input_state,
                        pointer,
                        elapsed,
                        &mut update_encoder,
                    );
                });
                raw_input_state.finish_frame();

                if let Some(display) = app.display().filter(|display| *display != winit.display) {
                    if display.vsync != winit.display.vsync {
                        render.surface.set_vsync(display.vsync);
                    }
                    winit.apply_display::<A>(display);
                }
                let viewport_params = winit.viewport::<A>(winit.window.inner_size());
                if viewport_params.canvas_size == viewport_params.canvas_display_size {
                    *supersample = None;
                } else if supersample.is_none() {
                    *supersample = Some(RenderTexture::new(
                        wgpu.device.clone(),
                        winit.resize_source.handle::<CanvasSize>(),
                        "supersample".to_string(),
                    ));
                }

                let mut render_encoder =
                    wgpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("Render"),
                        });

                // the frame's updates are still submitted when the surface has no texture to draw on (it is being
                // resized or switched in or out of fullscreen); the next frame draws
                let surface = match render.surface.get_current_texture() {
                    Ok(surface) => Some(surface),
                    Err(e) => {
                        warn!("Skipping a frame's drawing: {e}");
                        None
                    }
                };

                info_span!("render").in_scope(|| {
                    let Some((viewport, surface_texture)) = &surface else {
                        return;
                    };
                    let viewport = *viewport;
                    if let Some(canvas) = supersample {
                        // the canvas at its rendered size, then filtered down into its area of the window
                        let mut pass = RenderPass::new(
                            &mut render.pipelines,
                            &mut render.dynamic_buffer,
                            &render.sampler_store,
                            &wgpu.device,
                            &mut render_encoder,
                            canvas.as_texture_target(),
                            Some(render.canvas_depth_stencil_buffer.get_target_view()),
                            None,
                            "app",
                        );
                        app.render(RenderContext { winit, wgpu }, &mut pass);
                        drop(pass);

                        let mut pass = RenderPass::new(
                            &mut render.pipelines,
                            &mut render.dynamic_buffer,
                            &render.sampler_store,
                            &wgpu.device,
                            &mut render_encoder,
                            TextureTarget {
                                kind: TextureTargetKind::Screen,
                                view: &surface_texture.view,
                            },
                            None,
                            Some(viewport),
                            "supersample",
                        );
                        downsample(&mut pass, canvas, viewport_params.canvas_display_size);
                        drop(pass);
                    } else {
                        let mut pass = RenderPass::new(
                            &mut render.pipelines,
                            &mut render.dynamic_buffer,
                            &render.sampler_store,
                            &wgpu.device,
                            &mut render_encoder,
                            TextureTarget {
                                kind: TextureTargetKind::Screen,
                                view: &surface_texture.view,
                            },
                            Some(DepthStencilTarget {
                                view: render.surface_depth_stencil_buffer.resize_and_get_view(),
                            }),
                            Some(viewport),
                            "app",
                        );
                        app.render(RenderContext { winit, wgpu }, &mut pass);
                        drop(pass);
                    }
                    app.render_overlay(
                        RenderContext { winit, wgpu },
                        &mut render_encoder,
                        &surface_texture.view,
                    );
                });

                info_span!("submit").in_scope(|| {
                    let mut dynamic_buffer_encoder =
                        wgpu.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("DynamicBuffer"),
                            });
                    render.dynamic_buffer.finish(&mut dynamic_buffer_encoder);

                    wgpu.queue.submit([
                        dynamic_buffer_encoder.finish(),
                        update_encoder.finish(),
                        render_encoder.finish(),
                    ]);
                    render.dynamic_buffer.recall();
                });

                if let Some((_, surface_texture)) = surface {
                    info_span!("present").in_scope(|| {
                        winit.window.pre_present_notify();
                        surface_texture.texture.present();
                    });
                }

                tracy_client::frame_mark();

                winit.window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(winit) = self.winit() else {
            return;
        };

        winit.window.request_redraw();
    }
}

pub fn init_tracing() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            console_error_panic_hook::set_once();
            tracing_wasm::set_as_global_default();
        } else {
            // tracy_client::Client::start();

            tracing_subscriber::registry()
                .with(tracing_tracy::TracyLayer::default()
                    .with_filter(tracing_subscriber::filter::filter_fn(|m| m.module_path().is_some_and(|v| v.starts_with("shin")))))
                .with(tracing_subscriber::fmt::layer()
                    .with_filter(tracing_subscriber::EnvFilter::builder().from_env_lossy())
                )
            .init();
        }
    }
}

pub fn run_window<A: ShinApp>(
    parameters: A::Parameters,
    #[cfg(target_os = "android")] android_app: winit::platform::android::activity::AndroidApp,
) {
    #[allow(unused_mut)]
    let mut event_loop_builder = EventLoop::<ShinAppEventImpl<A::EventType>>::with_user_event();

    #[cfg(target_os = "android")]
    {
        event_loop_builder = event_loop_builder.android_app(android_app);
    }

    let event_loop = event_loop_builder.build().unwrap();
    let proxy = event_loop.create_proxy();

    event_loop
        .run_app(&mut WinitAppState::<A>::new(proxy, parameters))
        .unwrap();
}
