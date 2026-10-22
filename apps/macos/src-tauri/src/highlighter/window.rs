use std::num::NonZeroU32;
use std::sync::Arc;

use egui_wgpu::wgpu::PresentMode;
use egui_wgpu::winit::Painter;
use egui_wgpu::{RendererOptions, WgpuConfiguration, WgpuSetup};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window as WinitWindow, WindowButtons, WindowId, WindowLevel};

use super::Error;
use super::render_state::RenderState;

/// A transparent click-through overlay window for one monitor.
///
/// `Window` owns the native winit window plus egui/wgpu integration required to render into it. It
/// deliberately does not own highlighter drawing decisions; those are supplied by `RenderState`
/// during each redraw.
///
/// Each window owns its own egui context. A painter only receives the texture updates (such as
/// font atlas changes) from passes run on its own window, so windows that share one context end
/// up drawing with incomplete textures.
#[cfg(target_os = "macos")]
fn join_all_spaces(window: &WinitWindow) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::AppKit(handle) = handle.as_raw()
    {
        // SAFETY: the handle points at the winit-owned NSView for this window,
        // which outlives this call.
        let view = unsafe { &*(handle.ns_view.as_ptr() as *const objc2_app_kit::NSView) };
        if let Some(ns_window) = view.window() {
            let behavior = ns_window.collectionBehavior()
                | objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllSpaces
                | objc2_app_kit::NSWindowCollectionBehavior::Stationary
                | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary
                | objc2_app_kit::NSWindowCollectionBehavior::IgnoresCycle;
            ns_window.setCollectionBehavior(behavior);
            ns_window.setHasShadow(false);
            ns_window.setOpaque(false);
            // Raise above normal floating utility windows (NSStatusWindowLevel = 25).
            ns_window.setLevel(25);
            ns_window.orderFrontRegardless();
        }
    }
}

pub struct Window {
    inner: Arc<WinitWindow>,
    egui_state: egui_winit::State,
    painter: Painter,
    viewport_id: egui::ViewportId,
}

impl Window {
    pub async fn new(event_loop: &ActiveEventLoop, monitor: MonitorHandle) -> Result<Self, Error> {
        let context = egui::Context::default();
        let position = monitor.position();
        let size = monitor.size();
        let window = Arc::new(
            event_loop.create_window(
                WinitWindow::default_attributes()
                    .with_title("Elocutio")
                    .with_inner_size(size)
                    .with_position(position)
                    .with_resizable(false)
                    .with_enabled_buttons(WindowButtons::empty())
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_window_level(WindowLevel::AlwaysOnTop)
                    .with_active(false),
            )?,
        );

        #[cfg(target_os = "macos")]
        join_all_spaces(&window);

        window.set_outer_position(PhysicalPosition::new(position.x, position.y));
        let _ = window.request_inner_size(PhysicalSize::new(size.width, size.height));
        window.set_cursor_hittest(false)?;
        let viewport_id = egui::ViewportId::from_hash_of(window.id());

        let egui_state = egui_winit::State::new(
            context.clone(),
            viewport_id,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );

        let mut painter = Painter::new(
            context,
            WgpuConfiguration {
                present_mode: PresentMode::AutoNoVsync,
                wgpu_setup: WgpuSetup::from_display_handle(event_loop.owned_display_handle()),
                ..Default::default()
            },
            true,
            RendererOptions::default(),
        )
        .await;
        painter
            .set_window(viewport_id, Some(window.clone()))
            .await?;
        let inner_size = window.inner_size();
        if let (Some(width), Some(height)) = (
            NonZeroU32::new(inner_size.width),
            NonZeroU32::new(inner_size.height),
        ) {
            painter.on_window_resized(viewport_id, width, height);
        }
        window.request_redraw();

        Ok(Self {
            inner: window,
            egui_state,
            painter,
            viewport_id,
        })
    }

    pub fn id(&self) -> WindowId {
        self.inner.id()
    }

    pub fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    /// Controls whether the transparent overlay can receive pointer events.
    ///
    /// Highlight windows are click-through by default so the underlying app remains usable. The
    /// window manager temporarily enables hit-testing when global cursor polling shows the pointer
    /// is over an interactive highlight or popup.
    pub fn set_cursor_hittest(&self, enabled: bool) -> Result<(), Error> {
        self.inner.set_cursor_hittest(enabled)?;

        Ok(())
    }

    pub fn handle_event(&mut self, event: &WindowEvent) {
        let response = self.egui_state.on_window_event(&self.inner, event);

        if response.repaint {
            self.inner.request_redraw();
        }

        if let WindowEvent::Resized(size) = event
            && let (Some(width), Some(height)) =
                (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            self.painter
                .on_window_resized(self.viewport_id, width, height);
            self.inner.request_redraw();
        }
    }

    pub fn render(&mut self, render_state: &mut RenderState) {
        let context = self.egui_state.egui_ctx().clone();
        let scale = self.inner.scale_factor() as f32;
        let origin = self
            .inner
            .inner_position()
            .map(|pos| egui::vec2(pos.x as f32 / scale, pos.y as f32 / scale))
            .unwrap_or(egui::Vec2::ZERO);

        let mut input = self.egui_state.take_egui_input(&self.inner);
        if origin != egui::Vec2::ZERO {
            if let Some(rect) = &mut input.screen_rect {
                *rect = rect.translate(origin);
            }
            for event in &mut input.events {
                match event {
                    egui::Event::PointerMoved(pos) => *pos += origin,
                    egui::Event::PointerButton { pos, .. } => *pos += origin,
                    egui::Event::Touch { pos, .. } => *pos += origin,
                    _ => {}
                }
            }
        }

        let output = context.run_ui(input, |ui| {
            render_state.render(ui);
        });

        self.egui_state
            .handle_platform_output(&self.inner, output.platform_output);

        let mut shapes = output.shapes;
        if origin != egui::Vec2::ZERO {
            let to_local = egui::emath::TSTransform::from_translation(-origin);
            for clipped in &mut shapes {
                clipped.clip_rect = clipped.clip_rect.translate(-origin);
                clipped.shape.transform(to_local);
            }
        }

        let clipped_primitives = context.tessellate(shapes, output.pixels_per_point);
        self.painter.paint_and_update_textures(
            self.viewport_id,
            output.pixels_per_point,
            [0.0, 0.0, 0.0, 0.0],
            &clipped_primitives,
            &output.textures_delta,
            Vec::new(),
        );
    }
}
