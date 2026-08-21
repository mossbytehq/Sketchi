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

use crate::connection::{
    CollaborationClient, CollaborationIntent, CollaborationView, ConnectionError,
    parse_room_invite, resolve_invite_readiness,
};
use crate::editor::Editor;
use crate::gpu::GpuState;
use crate::lucide_icons;
use crate::storage;
use crate::supervisor::LocalServer;
use crate::tools::{Tool, ToolController};
use crate::ui::WorkspaceUi;
use crate::window_state::WindowState;
use crate::{settings, update, window_state};

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
    local_server_error: Option<String>,
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
        lucide_icons::install(&egui);
        let (local_server, local_server_error) = match LocalServer::spawn_default() {
            Ok(server) => {
                tracing::info!(
                    process_id = server.id(),
                    endpoint = %server.readiness().endpoint,
                    "local collaboration server ready"
                );
                (Some(server), None)
            }
            Err(error) => {
                tracing::warn!(error = %error, "Sketchi local server unavailable");
                (None, Some(error.to_string()))
            }
        };
        Ok(Self {
            event_loop,
            egui,
            wgpu_instance: wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle(),
            ),
            local_server,
            local_server_error,
        })
    }

    /// Enters the non-blocking native event loop.
    pub fn run(self) {
        let DesktopShell {
            event_loop,
            egui,
            wgpu_instance,
            local_server,
            local_server_error,
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
        if let Some(update_result) = update::take_update_result() {
            ui.set_update_message(update_result);
        }
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
            collaboration: None,
            collaboration_error: local_server_error,
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
    collaboration: Option<CollaborationClient>,
    collaboration_error: Option<String>,
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
            let egui_repaint = if paste_requested {
                queue_text_clipboard_paste(egui_state);
                false
            } else {
                egui_state.on_window_event(window.as_ref(), &event).repaint
            };
            if should_request_redraw(&event, egui_repaint, paste_requested, drop_event) {
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
                let full_output = context.run_ui(raw_input, |ui| {
                    let collaboration = self.collaboration_view();
                    let action = self.ui.show(
                        ui,
                        &mut self.editor,
                        &mut self.tools,
                        &mut self.camera,
                        &collaboration,
                    );
                    self.handle_collaboration_action(action);
                    if let Some(collaboration) = self.collaboration.as_mut()
                        && let Some(room_id) = collaboration.room_id()
                    {
                        let presence =
                            self.ui
                                .local_presence(ui.ctx(), self.camera, self.editor.client_id());
                        if let Err(error) = collaboration.offer_presence(room_id, presence)
                            && !matches!(error, ConnectionError::QueueFull)
                        {
                            self.collaboration_error = Some(error.to_string());
                        }
                    }
                    self.flush_collaboration_operations();
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
                    Err(
                        crate::gpu::GpuSurfaceError::Lost | crate::gpu::GpuSurfaceError::Outdated,
                    ) => {
                        tracing::warn!("GPU surface lost or outdated; reconfiguring");
                        gpu.reconfigure();
                        window.request_redraw();
                    }
                    Err(crate::gpu::GpuSurfaceError::Timeout) => {
                        tracing::warn!("GPU surface frame timed out");
                    }
                    Err(crate::gpu::GpuSurfaceError::Other) => {
                        tracing::error!("GPU surface returned an unspecified error");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let collaboration_changed = self.poll_collaboration();
        if collaboration_changed && let Some(window) = &self.window {
            window.request_redraw();
        }
        if self.ui.poll_update_check() {
            if let Some(window) = &self.settings_window {
                window.request_redraw();
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if self.ui.poll_update_install() {
            if let Some(window) = &self.settings_window {
                window.request_redraw();
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if self.ui.take_update_restart_request() {
            event_loop.exit();
            return;
        }
        self.save_window_state(false);
        self.save_settings(false);
        self.maybe_autosave();
        let next_autosave = self
            .settings_state
            .autosave_interval
            .duration()
            .map(|duration| {
                self.autosave_retry_at
                    .unwrap_or(self.last_autosave + duration)
            });
        let next_update_poll = self
            .ui
            .update_checking()
            .then_some(())
            .or_else(|| self.ui.update_installing().then_some(()))
            .map(|()| Instant::now() + Duration::from_millis(100));
        let next_collaboration_poll = self
            .collaboration
            .is_some()
            .then(|| Instant::now() + Duration::from_millis(50));
        let next_wakeup = [next_autosave, next_update_poll, next_collaboration_poll]
            .into_iter()
            .flatten()
            .min();
        if let Some(next_wakeup) = next_wakeup {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_wakeup));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        let _ = &self.local_server;
    }
}

impl DesktopApplication {
    fn collaboration_view(&self) -> CollaborationView {
        let mut view = self.collaboration.as_ref().map_or_else(
            || CollaborationView::disconnected(self.local_server.is_some()),
            CollaborationClient::view,
        );
        if let Some(error) = &self.collaboration_error {
            view.status.clone_from(error);
        }
        view
    }

    fn handle_collaboration_action(&mut self, action: crate::ui::CollaborationAction) {
        use crate::ui::CollaborationAction;

        let local_readiness = self
            .local_server
            .as_ref()
            .map(|server| server.readiness().clone());
        let intent = match action {
            CollaborationAction::None => return,
            CollaborationAction::Create { display_name } => {
                let Some(display_name) = normalized_display_name(&display_name) else {
                    self.collaboration_error =
                        Some(String::from("A display name is required to create a room."));
                    return;
                };
                let Some(readiness) = local_readiness.clone() else {
                    self.collaboration_error = Some(String::from(
                        "The local collaboration server is unavailable.",
                    ));
                    return;
                };
                CollaborationIntent::Create {
                    readiness,
                    display_name,
                }
            }
            CollaborationAction::Join {
                invite_token,
                display_name,
                endpoint,
                certificate_sha256,
            } => {
                let Some(display_name) = normalized_display_name(&display_name) else {
                    self.collaboration_error =
                        Some(String::from("A display name is required to join a room."));
                    return;
                };
                let invite = match parse_room_invite(&invite_token) {
                    Ok(invite) => invite,
                    Err(error) => {
                        self.collaboration_error = Some(error.to_string());
                        return;
                    }
                };
                let endpoint = invite
                    .endpoint
                    .as_deref()
                    .unwrap_or(endpoint.as_str())
                    .to_owned();
                let certificate_sha256 = invite
                    .certificate_sha256
                    .as_deref()
                    .unwrap_or(certificate_sha256.as_str())
                    .to_owned();
                let readiness = match resolve_invite_readiness(
                    local_readiness.as_ref(),
                    &endpoint,
                    &certificate_sha256,
                ) {
                    Ok(readiness) => readiness,
                    Err(error) => {
                        self.collaboration_error = Some(error.to_string());
                        return;
                    }
                };
                CollaborationIntent::Join {
                    room_id: invite.room_id,
                    capability_token: invite.capability_token,
                    readiness,
                    display_name,
                }
            }
        };

        let journal = match storage::open_journal(&self.settings_state.autosave_directory) {
            Ok(journal) => journal,
            Err(error) => {
                self.collaboration_error = Some(format!("Could not open sync journal: {error}"));
                return;
            }
        };
        match CollaborationClient::start(self.editor.client_id(), journal, intent) {
            Ok(collaboration) => {
                self.collaboration = Some(collaboration);
                self.collaboration_error = None;
            }
            Err(error) => self.collaboration_error = Some(error.to_string()),
        }
    }

    fn poll_collaboration(&mut self) -> bool {
        let messages = {
            let Some(collaboration) = self.collaboration.as_mut() else {
                return false;
            };
            match collaboration.poll() {
                Ok(messages) => messages,
                Err(error) => {
                    self.collaboration_error = Some(error.to_string());
                    return true;
                }
            }
        };
        let changed = !messages.is_empty();
        for message in messages {
            let result = {
                let Some(collaboration) = self.collaboration.as_mut() else {
                    return changed;
                };
                collaboration
                    .observe(&message)
                    .map_err(|error| error.to_string())
                    .and_then(|accepted| {
                        if !accepted {
                            return Ok(());
                        }
                        self.editor
                            .apply_server_message(
                                collaboration.synchronization_mut(),
                                &message.message,
                            )
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    })
            };
            if let Err(error) = result {
                self.collaboration_error = Some(error);
            }
        }
        self.flush_collaboration_operations();
        changed
    }

    fn flush_collaboration_operations(&mut self) {
        let result = {
            let Some(collaboration) = self.collaboration.as_mut() else {
                return;
            };
            self.editor
                .persist_pending(collaboration.synchronization_mut())
                .and_then(|_| collaboration.queue_pending())
        };
        if let Err(error) = result
            && !matches!(error, ConnectionError::QueueFull)
        {
            if let Some(collaboration) = self.collaboration.as_mut() {
                collaboration.set_error(error.to_string());
            }
            self.collaboration_error = Some(error.to_string());
        }
    }

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
                    lucide_icons::install(&self.settings_egui);
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
        let paste_requested = matches!(
            event,
            WindowEvent::KeyboardInput {
                event: key_event,
                ..
            } if is_clipboard_paste(key_event, self.modifiers)
        );
        let repaint = self.settings_egui_state.as_mut().is_some_and(|egui_state| {
            if paste_requested {
                queue_text_clipboard_paste(egui_state);
                false
            } else {
                egui_state.on_window_event(window.as_ref(), event).repaint
            }
        });
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
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
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

    fn render_settings_window(&mut self, _event_loop: &ActiveEventLoop) {
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
        let full_output = context.run_ui(raw_input, |ui| {
            self.ui
                .show_settings_window(ui, &mut self.editor, &mut self.tools);
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
            Err(crate::gpu::GpuSurfaceError::Lost | crate::gpu::GpuSurfaceError::Outdated) => {
                tracing::warn!("settings GPU surface lost or outdated; reconfiguring");
                gpu.reconfigure();
                window.request_redraw();
            }
            Err(crate::gpu::GpuSurfaceError::Timeout) => {
                tracing::warn!("settings GPU surface frame timed out");
            }
            Err(crate::gpu::GpuSurfaceError::Other) => {
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

fn normalized_display_name(value: &str) -> Option<String> {
    let name = value.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
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

fn queue_text_clipboard_paste(egui_state: &mut egui_winit::State) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return;
    };
    let Ok(contents) = clipboard.get_text() else {
        return;
    };
    let contents = contents.replace("\r\n", "\n");
    if !contents.is_empty() {
        egui_state
            .egui_input_mut()
            .events
            .push(egui::Event::Paste(contents));
    }
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
    use super::{
        WindowButtons, lucide_icons, normalized_display_name, settings_window_buttons,
        should_request_redraw,
    };
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
    fn normalized_display_name_rejects_blank_and_trims_valid_names() {
        assert_eq!(normalized_display_name(""), None);
        assert_eq!(normalized_display_name(" \t\n"), None);
        assert_eq!(
            normalized_display_name("  Santanu Datta  "),
            Some(String::from("Santanu Datta"))
        );
    }

    #[test]
    fn settings_context_emits_the_first_font_atlas_upload() {
        let context = egui::Context::default();
        lucide_icons::install(&context);

        let output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
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

        lucide_icons::install(&context);
        let reopened_output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
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
