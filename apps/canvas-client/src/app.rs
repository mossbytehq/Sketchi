//! Desktop application shell boundary.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use canvas_core::ClientId;
use canvas_renderer::Camera;
use thiserror::Error;
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{Icon as WindowIcon, Theme, Window, WindowButtons, WindowId},
};

use crate::editor::Editor;
use crate::gpu::GpuState;
use crate::remix_icons;
use crate::storage;
use crate::supervisor::LocalServer;
use crate::tools::{Tool, ToolController};
use crate::ui::WorkspaceUi;
use crate::window_state::WindowState;
use crate::{settings, window_state};

/// Application state shared by the winit event loop and renderer.
pub struct AppState {
    /// Operation-first local editor.
    pub editor: Editor,
}

/// Errors raised while creating the desktop shell.
#[derive(Debug, Error)]
pub enum AppError {
    /// The operating-system event loop could not be created.
    #[error("could not create event loop: {0}")]
    EventLoop(String),
    /// The application window could not be created.
    #[error("could not create window: {0}")]
    Window(String),
}

/// Native window and rendering-context foundation.
pub struct DesktopShell {
    event_loop: EventLoop<()>,
    /// Shared immediate-mode UI context for editor chrome and overlays.
    pub egui: egui::Context,
    /// GPU instance used by the eventual surface renderer.
    pub wgpu_instance: wgpu::Instance,
    local_server: Option<LocalServer>,
}

impl DesktopShell {
    /// Creates a native shell without requiring a GPU adapter yet.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the platform cannot create an event loop or
    /// window.
    pub fn new() -> Result<Self, AppError> {
        tracing::info!(
            wayland_display = ?std::env::var_os("WAYLAND_DISPLAY"),
            x11_display = ?std::env::var_os("DISPLAY"),
            unix_backend = ?std::env::var_os("WINIT_UNIX_BACKEND"),
            "desktop backend environment"
        );
        tracing::info!("creating native event loop");
        let event_loop =
            EventLoop::new().map_err(|error| AppError::EventLoop(error.to_string()))?;
        let egui = egui::Context::default();
        remix_icons::install(&egui);
        let local_server = match LocalServer::spawn_default() {
            Ok(server) => {
                tracing::info!(
                    process_id = server.id(),
                    endpoint = %server.readiness().endpoint,
                    "local collaboration server ready"
                );
                Some(server)
            }
            Err(error) => {
                tracing::warn!(error = %error, "Sketchi local server unavailable");
                None
            }
        };
        Ok(Self {
            event_loop,
            egui,
            wgpu_instance: wgpu::Instance::new(&wgpu::InstanceDescriptor::default()),
            local_server,
        })
    }

    /// Enters the non-blocking native event loop.
    pub fn run(self) {
        let DesktopShell {
            event_loop,
            egui,
            wgpu_instance,
            local_server,
        } = self;
        let saved_window_state = match window_state::load() {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(error = %error, "Sketchi could not load native window state");
                None
            }
        };
        let saved_settings = match settings::load() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(error = %error, "Sketchi could not load settings");
                None
            }
        };
        let settings_state = saved_settings.clone().unwrap_or_default();
        let mut ui = WorkspaceUi::from_settings(&settings_state);
        let restore_session = saved_window_state
            .as_ref()
            .is_none_or(|state| state.restore_session);
        ui.set_restore_session(restore_session);
        let editor = if restore_session {
            match storage::load_document(&settings_state.autosave_directory) {
                Ok(Some(document)) => match Editor::from_document(ClientId::new(), &document) {
                    Ok(editor) => editor,
                    Err(error) => {
                        tracing::warn!(error = %error, "Sketchi could not restore autosave");
                        Editor::new(ClientId::new())
                    }
                },
                Ok(None) => Editor::new(ClientId::new()),
                Err(error) => {
                    tracing::warn!(error = %error, "Sketchi could not read autosave");
                    Editor::new(ClientId::new())
                }
            }
        } else {
            Editor::new(ClientId::new())
        };
        let settings_egui = egui::Context::default();
        let mut application = DesktopApplication {
            window: None,
            gpu: None,
            egui_state: None,
            egui,
            settings_window: None,
            settings_gpu: None,
            settings_egui_state: None,
            settings_egui,
            wgpu_instance,
            local_server,
            ui,
            editor,
            tools: ToolController::new(Tool::Select),
            camera: Camera::default(),
            first_frame_logged: false,
            settings_first_frame_logged: false,
            window_state: saved_window_state,
            window_state_dirty: false,
            settings_state,
            settings_dirty: false,
            last_autosave: Instant::now(),
            autosave_retry_at: None,
            modifiers: ModifiersState::default(),
        };
        let result = event_loop.run_app(&mut application);
        if let Err(error) = result {
            tracing::error!(error = %error, "Sketchi event loop stopped");
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
struct DesktopApplication {
    window: Option<Arc<Window>>,
    gpu: Option<GpuState>,
    egui_state: Option<egui_winit::State>,
    egui: egui::Context,
    settings_window: Option<Arc<Window>>,
    settings_gpu: Option<GpuState>,
    settings_egui_state: Option<egui_winit::State>,
    settings_egui: egui::Context,
    wgpu_instance: wgpu::Instance,
    local_server: Option<LocalServer>,
    ui: WorkspaceUi,
    editor: Editor,
    tools: ToolController,
    camera: Camera,
    first_frame_logged: bool,
    settings_first_frame_logged: bool,
    window_state: Option<WindowState>,
    window_state_dirty: bool,
    settings_state: settings::Settings,
    settings_dirty: bool,
    last_autosave: Instant,
    autosave_retry_at: Option<Instant>,
    modifiers: ModifiersState,
}

impl ApplicationHandler for DesktopApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let mut attributes = Window::default_attributes()
                .with_title("Sketchi")
                .with_window_icon(native_window_icon());
            #[cfg(target_os = "linux")]
            {
                attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
                    attributes, "sketchi", "sketchi",
                );
            }
            if self.ui.restore_session_enabled()
                && let Some(state) = self.window_state.as_ref()
            {
                if let Some([x, y]) = state.position {
                    attributes = attributes.with_position(PhysicalPosition::new(x, y));
                }
                attributes = attributes
                    .with_inner_size(PhysicalSize::new(state.inner_size[0], state.inner_size[1]))
                    .with_maximized(state.maximized);
            }
            match event_loop.create_window(attributes) {
                Ok(window) => {
                    let window = Arc::new(window);
                    let repaint_window = Arc::clone(&window);
                    self.egui.set_request_repaint_callback(move |_info| {
                        repaint_window.request_redraw();
                    });
                    let size = window.inner_size();
                    self.ui
                        .set_system_dark_mode(window.theme() == Some(Theme::Dark));
                    tracing::info!(
                        width = size.width,
                        height = size.height,
                        scale_factor = window.scale_factor(),
                        "native window created"
                    );
                    let egui_state = egui_winit::State::new(
                        self.egui.clone(),
                        egui::ViewportId::ROOT,
                        window.as_ref(),
                        Some(native_pixels_per_point(window.as_ref())),
                        None,
                        None,
                    );
                    match GpuState::new(window.clone(), &self.wgpu_instance) {
                        Ok(gpu) => {
                            let adapter = gpu.adapter_info();
                            tracing::info!(
                                adapter = %adapter.name,
                                backend = ?adapter.backend,
                                driver = %adapter.driver,
                                "GPU adapter initialized"
                            );
                            self.egui_state = Some(egui_state);
                            self.gpu = Some(gpu);
                            self.window = Some(window.clone());
                            window.request_redraw();
                        }
                        Err(error) => {
                            tracing::error!(error = %error, "GPU initialization failed");
                            event_loop.exit();
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(error = %error, "Sketchi could not create window");
                    event_loop.exit();
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::ModifiersChanged(modifiers) = &event {
            self.modifiers = modifiers.state();
        }
        if self
            .settings_window
            .as_ref()
            .is_some_and(|window| window.id() == window_id)
        {
            self.handle_settings_window_event(event_loop, &event);
            return;
        }
        let paste_requested = matches!(
            &event,
            WindowEvent::KeyboardInput {
                event: key_event,
                ..
            } if is_clipboard_paste(key_event, self.modifiers)
        );
        if paste_requested {
            self.ui.request_clipboard_image_paste();
        }
        let drop_event = self.handle_native_drop_event(&event);

        if let Some(window) = &self.window
            && let Some(egui_state) = &mut self.egui_state
        {
            let response = egui_state.on_window_event(window.as_ref(), &event);
            if should_request_redraw(&event, response.repaint, paste_requested, drop_event) {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("close requested");
                self.save_window_state(true);
                self.sync_settings_preferences();
                self.save_settings(true);
                self.save_document_if_enabled(true);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.window_state_dirty = true;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Moved(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window_state_dirty = true;
            }
            WindowEvent::ThemeChanged(theme) => {
                self.ui.set_system_dark_mode(theme == Theme::Dark);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                if let Some(window) = &self.settings_window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(window) = self.window.clone() else {
                    tracing::warn!("redraw requested before native window initialization");
                    return;
                };
                let Some(egui_state) = &mut self.egui_state else {
                    tracing::warn!("redraw requested before egui initialization");
                    return;
                };
                let raw_input = egui_state.take_egui_input(window.as_ref());
                let context = self.egui.clone();
                let full_output = context.run(raw_input, |context| {
                    self.ui
                        .show(context, &mut self.editor, &mut self.tools, &mut self.camera);
                });
                self.sync_window_state_preferences();
                self.sync_settings_preferences();
                self.maybe_autosave();
                self.sync_settings_window(event_loop);
                if let Some(egui_state) = &mut self.egui_state {
                    egui_state.handle_platform_output(
                        window.as_ref(),
                        full_output.platform_output.clone(),
                    );
                }
                let Some(gpu) = &mut self.gpu else {
                    tracing::warn!("redraw requested before GPU initialization");
                    return;
                };
                match gpu.render(
                    &context,
                    full_output,
                    wgpu::Color {
                        r: 0.04,
                        g: 0.07,
                        b: 0.11,
                        a: 1.0,
                    },
                ) {
                    Ok(()) => {
                        if !self.first_frame_logged {
                            self.first_frame_logged = true;
                            tracing::info!("first GPU frame presented");
                        }
                    }
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        tracing::warn!("GPU surface lost or outdated; reconfiguring");
                        gpu.reconfigure();
                        window.request_redraw();
                    }
                    Err(wgpu::SurfaceError::Timeout) => {
                        tracing::warn!("GPU surface frame timed out");
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        tracing::error!("GPU surface is out of memory");
                        event_loop.exit();
                    }
                    Err(wgpu::SurfaceError::Other) => {
                        tracing::error!("GPU surface returned an unspecified error");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.save_window_state(false);
        self.save_settings(false);
        self.maybe_autosave();
        if let Some(duration) = self.settings_state.autosave_interval.duration() {
            let next_autosave = self
                .autosave_retry_at
                .unwrap_or(self.last_autosave + duration);
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_autosave));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        let _ = &self.local_server;
    }
}

impl DesktopApplication {
    fn sync_settings_window(&mut self, event_loop: &ActiveEventLoop) {
        if self.ui.settings_open() {
            if self.settings_window.is_some() {
                return;
            }

            let attributes = Window::default_attributes()
                .with_title("Settings — Sketchi")
                .with_window_icon(native_window_icon())
                .with_inner_size(PhysicalSize::new(860, 560))
                .with_min_inner_size(PhysicalSize::new(720, 480))
                .with_decorations(true)
                .with_enabled_buttons(settings_window_buttons())
                .with_resizable(true);
            #[cfg(target_os = "linux")]
            let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
                attributes, "sketchi", "settings",
            );

            let window = match event_loop.create_window(attributes) {
                Ok(window) => Arc::new(window),
                Err(error) => {
                    tracing::error!(error = %error, "Sketchi could not create settings window");
                    self.ui.close_settings();
                    if let Some(main_window) = &self.window {
                        main_window.request_redraw();
                    }
                    return;
                }
            };
            let egui_state = egui_winit::State::new(
                self.settings_egui.clone(),
                egui::ViewportId::ROOT,
                window.as_ref(),
                Some(native_pixels_per_point(window.as_ref())),
                None,
                None,
            );
            match GpuState::new(window.clone(), &self.wgpu_instance) {
                Ok(gpu) => {
                    // The settings renderer is recreated when the window is
                    // reopened. Reinstalling the fonts invalidates the
                    // retained egui atlas so the next frame uploads it to
                    // this new renderer.
                    remix_icons::install(&self.settings_egui);
                    tracing::info!(
                        width = window.inner_size().width,
                        height = window.inner_size().height,
                        scale_factor = window.scale_factor(),
                        "settings window created"
                    );
                    self.settings_egui_state = Some(egui_state);
                    self.settings_gpu = Some(gpu);
                    self.settings_window = Some(window.clone());
                    self.settings_first_frame_logged = false;
                    window.request_redraw();
                }
                Err(error) => {
                    tracing::error!(error = %error, "Sketchi could not initialize settings window GPU");
                    self.ui.close_settings();
                    if let Some(main_window) = &self.window {
                        main_window.request_redraw();
                    }
                }
            }
        } else if self.settings_window.is_some() {
            self.close_settings_window();
        }
    }

    fn close_settings_window(&mut self) {
        self.settings_egui_state = None;
        self.settings_gpu = None;
        self.settings_window = None;
        self.settings_first_frame_logged = false;
        if let Some(main_window) = &self.window {
            main_window.request_redraw();
        }
    }

    fn handle_settings_window_event(&mut self, event_loop: &ActiveEventLoop, event: &WindowEvent) {
        let Some(window) = self.settings_window.clone() else {
            return;
        };
        let repaint = self
            .settings_egui_state
            .as_mut()
            .is_some_and(|egui_state| egui_state.on_window_event(window.as_ref(), event).repaint);
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("settings window close requested");
                self.ui.close_settings();
                self.close_settings_window();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.settings_gpu {
                    gpu.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                window.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => {
                self.ui.set_system_dark_mode(*theme == Theme::Dark);
                if let Some(main_window) = &self.window {
                    main_window.request_redraw();
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.render_settings_window(event_loop),
            _ if repaint => {
                window.request_redraw();
                if let Some(main_window) = &self.window {
                    main_window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn render_settings_window(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.settings_window.clone() else {
            return;
        };
        let raw_input = {
            let Some(egui_state) = &mut self.settings_egui_state else {
                tracing::warn!("settings redraw requested before egui initialization");
                return;
            };
            egui_state.take_egui_input(window.as_ref())
        };
        let context = self.settings_egui.clone();
        let full_output = context.run(raw_input, |context| {
            self.ui
                .show_settings_window(context, &mut self.editor, &mut self.tools);
        });
        self.sync_settings_preferences();
        if let Some(egui_state) = &mut self.settings_egui_state {
            egui_state.handle_platform_output(window.as_ref(), full_output.platform_output.clone());
        }
        let Some(gpu) = &mut self.settings_gpu else {
            tracing::warn!("settings redraw requested before GPU initialization");
            return;
        };
        match gpu.render(
            &context,
            full_output,
            GpuState::settings_clear_color(self.ui.settings_dark_mode()),
        ) {
            Ok(()) => {
                if !self.settings_first_frame_logged {
                    self.settings_first_frame_logged = true;
                    tracing::info!("first settings GPU frame presented");
                }
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                tracing::warn!("settings GPU surface lost or outdated; reconfiguring");
                gpu.reconfigure();
                window.request_redraw();
            }
            Err(wgpu::SurfaceError::Timeout) => {
                tracing::warn!("settings GPU surface frame timed out");
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                tracing::error!("settings GPU surface is out of memory");
                event_loop.exit();
            }
            Err(wgpu::SurfaceError::Other) => {
                tracing::error!("settings GPU surface returned an unspecified error");
            }
        }
        if !self.ui.settings_open() {
            self.close_settings_window();
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn handle_native_drop_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::HoveredFile(path) => {
                tracing::info!(
                    path = %path.display(),
                    "native file-hover event received"
                );
                self.ui.set_drop_preview(path.clone());
                true
            }
            WindowEvent::HoveredFileCancelled => {
                tracing::info!("native file-hover cancellation received");
                self.ui.set_drop_hovered(false);
                true
            }
            WindowEvent::DroppedFile(path) => {
                tracing::info!(
                    path = %path.display(),
                    "native file-drop event received"
                );
                self.ui.queue_dropped_file(path.clone());
                true
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale_factor = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor());
                self.ui.set_drop_position(egui::pos2(
                    position.x as f32 / scale_factor as f32,
                    position.y as f32 / scale_factor as f32,
                ))
            }
            _ => false,
        }
    }

    fn sync_window_state_preferences(&mut self) {
        let restore_session = self.ui.restore_session_enabled();
        match self.window_state.as_mut() {
            Some(state) if state.restore_session != restore_session => {
                state.restore_session = restore_session;
                self.window_state_dirty = true;
            }
            None if !restore_session => {
                self.window_state = Some(WindowState {
                    restore_session: false,
                    ..WindowState::default()
                });
                self.window_state_dirty = true;
            }
            _ => {}
        }
    }

    fn sync_settings_preferences(&mut self) {
        let settings = self.ui.settings_snapshot();
        if settings != self.settings_state {
            self.settings_state = settings;
            self.settings_dirty = true;
        }
    }

    fn save_settings(&mut self, force: bool) {
        if !force && !self.settings_dirty {
            return;
        }
        if let Err(error) = settings::save(&self.settings_state) {
            tracing::warn!(error = %error, "Sketchi could not save settings");
            return;
        }
        self.settings_dirty = false;
    }

    fn maybe_autosave(&mut self) {
        let Some(interval) = self.settings_state.autosave_interval.duration() else {
            return;
        };
        let due = self
            .autosave_retry_at
            .unwrap_or(self.last_autosave + interval);
        if Instant::now() >= due {
            self.save_document_if_enabled(false);
        }
    }

    fn save_document_if_enabled(&mut self, force: bool) {
        if !force && self.settings_state.autosave_interval.duration().is_none() {
            return;
        }
        if force && !self.ui.restore_session_enabled() {
            return;
        }
        match storage::save_document(
            &self.settings_state.autosave_directory,
            self.editor.document(),
        ) {
            Ok(path) => {
                self.last_autosave = Instant::now();
                self.autosave_retry_at = None;
                tracing::info!(path = %path.display(), "Sketchi saved local document");
            }
            Err(error) => {
                tracing::warn!(error = %error, "Sketchi could not save local document");
                self.autosave_retry_at = Some(Instant::now() + Duration::from_secs(5));
            }
        }
    }

    fn capture_window_state(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let maximized = window.is_maximized();
        let position = window
            .outer_position()
            .ok()
            .map(|position| [position.x, position.y]);
        let inner_size = window.inner_size();
        let restore_session = self.ui.restore_session_enabled();

        match self.window_state.as_mut() {
            Some(state) => {
                state.maximized = maximized;
                state.restore_session = restore_session;
                if !maximized {
                    state.position = position;
                    state.inner_size = [inner_size.width, inner_size.height];
                }
            }
            None => {
                self.window_state = Some(WindowState {
                    position,
                    inner_size: [inner_size.width, inner_size.height],
                    maximized,
                    restore_session,
                });
            }
        }
    }

    fn save_window_state(&mut self, force: bool) {
        if !force && !self.window_state_dirty {
            return;
        }
        self.capture_window_state();
        let Some(state) = self.window_state.as_ref() else {
            return;
        };
        if let Err(error) = window_state::save(state) {
            tracing::warn!(error = %error, "Sketchi could not save native window state");
        }
        self.window_state_dirty = false;
    }
}

impl Drop for DesktopApplication {
    fn drop(&mut self) {
        self.save_window_state(true);
        self.sync_settings_preferences();
        self.save_settings(true);
        self.save_document_if_enabled(true);
    }
}

impl AppState {
    /// Creates application state for a client identity.
    #[must_use]
    pub fn new(client_id: ClientId) -> Self {
        Self {
            editor: Editor::new(client_id),
        }
    }
}

/// Runs the current desktop foundation.
pub fn run() {
    match DesktopShell::new() {
        Ok(shell) => shell.run(),
        Err(error) => tracing::error!(error = %error, "Sketchi could not start"),
    }
}

const fn settings_window_buttons() -> WindowButtons {
    WindowButtons::CLOSE
}

#[allow(clippy::cast_possible_truncation)]
fn native_pixels_per_point(window: &Window) -> f32 {
    window.scale_factor() as f32
}

fn native_window_icon() -> Option<WindowIcon> {
    let decoded = match image::load_from_memory(include_bytes!("../assets/sketchi.png")) {
        Ok(decoded) => decoded,
        Err(error) => {
            tracing::warn!(error = %error, "Sketchi could not decode its window icon");
            return None;
        }
    };
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    match WindowIcon::from_rgba(rgba.into_raw(), width, height) {
        Ok(icon) => Some(icon),
        Err(error) => {
            tracing::warn!(error = %error, "Sketchi could not create its window icon");
            None
        }
    }
}

fn is_clipboard_paste(event: &KeyEvent, modifiers: ModifiersState) -> bool {
    if event.state != ElementState::Pressed || event.repeat {
        return false;
    }

    if matches!(event.logical_key, Key::Named(NamedKey::Paste)) {
        return true;
    }

    let is_v = matches!(&event.logical_key, Key::Character(key) if key.as_str().eq_ignore_ascii_case("v"))
        || matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyV));
    is_v && (modifiers.control_key() || modifiers.super_key())
}

fn should_request_redraw(
    event: &WindowEvent,
    egui_repaint: bool,
    paste_requested: bool,
    drop_event: bool,
) -> bool {
    (egui_repaint && !matches!(event, WindowEvent::RedrawRequested))
        || paste_requested
        || drop_event
}

#[cfg(test)]
mod tests {
    use super::{WindowButtons, remix_icons, settings_window_buttons, should_request_redraw};
    use winit::event::WindowEvent;

    #[test]
    fn settings_window_keeps_only_the_native_close_button() {
        assert_eq!(settings_window_buttons(), WindowButtons::CLOSE);
    }

    #[test]
    fn redraw_requested_does_not_schedule_another_redraw() {
        assert!(!should_request_redraw(
            &WindowEvent::RedrawRequested,
            true,
            false,
            false,
        ));
        assert!(should_request_redraw(
            &WindowEvent::RedrawRequested,
            false,
            true,
            false,
        ));
        assert!(should_request_redraw(
            &WindowEvent::RedrawRequested,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn settings_context_emits_the_first_font_atlas_upload() {
        let context = egui::Context::default();
        remix_icons::install(&context);

        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.label("Settings");
            });
        });

        assert!(
            output
                .textures_delta
                .set
                .iter()
                .any(|(id, _)| *id == egui::TextureId::Managed(0))
        );
        assert!(
            !output
                .textures_delta
                .free
                .contains(&egui::TextureId::Managed(0))
        );

        remix_icons::install(&context);
        let reopened_output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.label("Settings reopened");
            });
        });
        assert!(
            reopened_output
                .textures_delta
                .set
                .iter()
                .any(|(id, _)| *id == egui::TextureId::Managed(0))
        );
        assert!(
            !reopened_output
                .textures_delta
                .free
                .contains(&egui::TextureId::Managed(0))
        );
    }
}
