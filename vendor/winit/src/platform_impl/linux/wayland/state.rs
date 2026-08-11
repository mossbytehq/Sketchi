use std::cell::RefCell;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use ahash::AHashMap;

use sctk::reexports::calloop::{LoopHandle, PostAction, RegistrationToken};
use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::globals::GlobalList;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_output::WlOutput;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use sctk::compositor::{CompositorHandler, CompositorState};
use sctk::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::{DataDeviceManagerState, ReadPipe};
use sctk::output::{OutputHandler, OutputState};
use sctk::registry::{ProvidesRegistryState, RegistryState};
use sctk::seat::pointer::ThemedPointer;
use sctk::seat::SeatState;
use sctk::shell::xdg::window::{Window, WindowConfigure, WindowHandler};
use sctk::shell::xdg::XdgShell;
use sctk::shell::WaylandSurface;
use sctk::shm::slot::SlotPool;
use sctk::shm::{Shm, ShmHandler};
use sctk::subcompositor::SubcompositorState;

use crate::dpi::LogicalPosition;
use crate::event::WindowEvent;
use crate::platform_impl::wayland::event_loop::sink::EventSink;
use crate::platform_impl::wayland::output::MonitorHandle;
use crate::platform_impl::wayland::seat::{
    PointerConstraintsState, RelativePointerState, TextInputState, WinitPointerData,
    WinitPointerDataExt, WinitSeatState,
};
use crate::platform_impl::wayland::types::kwin_blur::KWinBlurManager;
use crate::platform_impl::wayland::types::wp_fractional_scaling::FractionalScalingManager;
use crate::platform_impl::wayland::types::wp_viewporter::ViewporterState;
use crate::platform_impl::wayland::types::xdg_activation::XdgActivationState;
use crate::platform_impl::wayland::window::{WindowRequests, WindowState};
use crate::platform_impl::wayland::{DeviceId, WaylandError, WindowId};
use crate::platform_impl::OsError;

/// Winit's Wayland state.
pub struct WinitState {
    /// The WlRegistry.
    pub registry_state: RegistryState,

    /// The state of the WlOutput handling.
    pub output_state: OutputState,

    /// The compositor state which is used to create new windows and regions.
    pub compositor_state: Arc<CompositorState>,

    /// The state of the subcompositor.
    pub subcompositor_state: Option<Arc<SubcompositorState>>,

    /// The seat state responsible for all sorts of input.
    pub seat_state: SeatState,

    /// The shm for software buffers, such as cursors.
    pub shm: Shm,

    /// The pool where custom cursors are allocated.
    pub custom_cursor_pool: Arc<Mutex<SlotPool>>,

    /// The XDG shell that is used for windows.
    pub xdg_shell: XdgShell,

    /// The currently present windows.
    pub windows: RefCell<AHashMap<WindowId, Arc<Mutex<WindowState>>>>,

    /// The requests from the `Window` to EventLoop, such as close operations and redraw requests.
    pub window_requests: RefCell<AHashMap<WindowId, Arc<WindowRequests>>>,

    /// The events that were generated directly from the window.
    pub window_events_sink: Arc<Mutex<EventSink>>,

    /// The update for the `windows` coming from the compositor.
    pub window_compositor_updates: Vec<WindowCompositorUpdate>,

    /// Currently handled seats.
    pub seats: AHashMap<ObjectId, WinitSeatState>,

    /// The data-device manager used to receive native Wayland file drops.
    pub data_device_manager_state: Option<DataDeviceManagerState>,

    /// Active native Wayland drag offers and their URI-list read pipes.
    dnd_offers: Vec<DndOfferState>,

    /// Currently present cursor surfaces.
    pub pointer_surfaces: AHashMap<ObjectId, Arc<ThemedPointer<WinitPointerData>>>,

    /// The state of the text input on the client.
    pub text_input_state: Option<TextInputState>,

    /// Observed monitors.
    pub monitors: Arc<Mutex<Vec<MonitorHandle>>>,

    /// Sink to accumulate window events from the compositor, which is latter dispatched in
    /// event loop run.
    pub events_sink: EventSink,

    /// Xdg activation.
    pub xdg_activation: Option<XdgActivationState>,

    /// Relative pointer.
    pub relative_pointer: Option<RelativePointerState>,

    /// Pointer constraints to handle pointer locking and confining.
    pub pointer_constraints: Option<Arc<PointerConstraintsState>>,

    /// Viewporter state on the given window.
    pub viewporter_state: Option<ViewporterState>,

    /// Fractional scaling manager.
    pub fractional_scaling_manager: Option<FractionalScalingManager>,

    /// KWin blur manager.
    pub kwin_blur_manager: Option<KWinBlurManager>,

    /// Loop handle to re-register event sources, such as keyboard repeat.
    pub loop_handle: LoopHandle<'static, Self>,

    /// Whether we have dispatched events to the user thus we want to
    /// send `AboutToWait` and normally wakeup the user.
    pub dispatched_events: bool,
}

impl WinitState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<Self>,
        loop_handle: LoopHandle<'static, WinitState>,
    ) -> Result<Self, OsError> {
        let registry_state = RegistryState::new(globals);
        let compositor_state =
            CompositorState::bind(globals, queue_handle).map_err(WaylandError::Bind)?;
        let subcompositor_state = match SubcompositorState::bind(
            compositor_state.wl_compositor().clone(),
            globals,
            queue_handle,
        ) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("Subcompositor protocol not available, ignoring CSD: {e:?}");
                None
            },
        };

        let output_state = OutputState::new(globals, queue_handle);
        let monitors = output_state.outputs().map(MonitorHandle::new).collect();

        let seat_state = SeatState::new(globals, queue_handle);

        let mut seats = AHashMap::default();
        for seat in seat_state.seats() {
            seats.insert(seat.id(), WinitSeatState::new());
        }

        let (viewporter_state, fractional_scaling_manager) =
            if let Ok(fsm) = FractionalScalingManager::new(globals, queue_handle) {
                (ViewporterState::new(globals, queue_handle).ok(), Some(fsm))
            } else {
                (None, None)
            };

        let shm = Shm::bind(globals, queue_handle).map_err(WaylandError::Bind)?;
        let custom_cursor_pool = Arc::new(Mutex::new(SlotPool::new(2, &shm).unwrap()));

        Ok(Self {
            registry_state,
            compositor_state: Arc::new(compositor_state),
            subcompositor_state: subcompositor_state.map(Arc::new),
            output_state,
            seat_state,
            shm,
            custom_cursor_pool,

            xdg_shell: XdgShell::bind(globals, queue_handle).map_err(WaylandError::Bind)?,
            xdg_activation: XdgActivationState::bind(globals, queue_handle).ok(),

            windows: Default::default(),
            window_requests: Default::default(),
            window_compositor_updates: Vec::new(),
            window_events_sink: Default::default(),
            viewporter_state,
            fractional_scaling_manager,
            kwin_blur_manager: KWinBlurManager::new(globals, queue_handle).ok(),

            seats,
            data_device_manager_state: DataDeviceManagerState::bind(globals, queue_handle).ok(),
            dnd_offers: Vec::new(),
            text_input_state: TextInputState::new(globals, queue_handle).ok(),

            relative_pointer: RelativePointerState::new(globals, queue_handle).ok(),
            pointer_constraints: PointerConstraintsState::new(globals, queue_handle)
                .map(Arc::new)
                .ok(),
            pointer_surfaces: Default::default(),

            monitors: Arc::new(Mutex::new(monitors)),
            events_sink: EventSink::new(),
            loop_handle,
            // Make it true by default.
            dispatched_events: true,
        })
    }

    pub fn scale_factor_changed(
        &mut self,
        surface: &WlSurface,
        scale_factor: f64,
        is_legacy: bool,
    ) {
        // Check if the cursor surface.
        let window_id = super::make_wid(surface);

        if let Some(window) = self.windows.get_mut().get(&window_id) {
            // Don't update the scaling factor, when legacy method is used.
            if is_legacy && self.fractional_scaling_manager.is_some() {
                return;
            }

            // The scale factor change is for the window.
            let pos = if let Some(pos) = self
                .window_compositor_updates
                .iter()
                .position(|update| update.window_id == window_id)
            {
                pos
            } else {
                self.window_compositor_updates.push(WindowCompositorUpdate::new(window_id));
                self.window_compositor_updates.len() - 1
            };

            // Update the scale factor right away.
            window.lock().unwrap().set_scale_factor(scale_factor);
            self.window_compositor_updates[pos].scale_changed = true;
        } else if let Some(pointer) = self.pointer_surfaces.get(&surface.id()) {
            // Get the window, where the pointer resides right now.
            let focused_window = match pointer.pointer().winit_data().focused_window() {
                Some(focused_window) => focused_window,
                None => return,
            };

            if let Some(window_state) = self.windows.get_mut().get(&focused_window) {
                window_state.lock().unwrap().reload_cursor_style()
            }
        }
    }

    pub fn queue_close(updates: &mut Vec<WindowCompositorUpdate>, window_id: WindowId) {
        let pos = if let Some(pos) = updates.iter().position(|update| update.window_id == window_id)
        {
            pos
        } else {
            updates.push(WindowCompositorUpdate::new(window_id));
            updates.len() - 1
        };

        updates[pos].close_window = true;
    }
}

impl ShmHandler for WinitState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl WindowHandler for WinitState {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window) {
        let window_id = super::make_wid(window.wl_surface());
        Self::queue_close(&mut self.window_compositor_updates, window_id);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let window_id = super::make_wid(window.wl_surface());

        let pos = if let Some(pos) =
            self.window_compositor_updates.iter().position(|update| update.window_id == window_id)
        {
            pos
        } else {
            self.window_compositor_updates.push(WindowCompositorUpdate::new(window_id));
            self.window_compositor_updates.len() - 1
        };

        // Populate the configure to the window.
        self.window_compositor_updates[pos].resized |= self
            .windows
            .get_mut()
            .get_mut(&window_id)
            .expect("got configure for dead window.")
            .lock()
            .unwrap()
            .configure(configure, &self.shm, &self.subcompositor_state);

        // NOTE: configure demands wl_surface::commit, however winit doesn't commit on behalf of the
        // users, since it can break a lot of things, thus it'll ask users to redraw instead.
        self.window_requests
            .get_mut()
            .get(&window_id)
            .unwrap()
            .redraw_requested
            .store(true, Ordering::Relaxed);

        // Manually mark that we've got an event, since configure may not generate a resize.
        self.dispatched_events = true;
    }
}

impl OutputHandler for WinitState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        self.monitors.lock().unwrap().push(MonitorHandle::new(output));
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, updated: WlOutput) {
        let mut monitors = self.monitors.lock().unwrap();
        let updated = MonitorHandle::new(updated);
        if let Some(pos) = monitors.iter().position(|output| output == &updated) {
            monitors[pos] = updated
        } else {
            monitors.push(updated)
        }
    }

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, removed: WlOutput) {
        let mut monitors = self.monitors.lock().unwrap();
        let removed = MonitorHandle::new(removed);
        if let Some(pos) = monitors.iter().position(|output| output == &removed) {
            monitors.remove(pos);
        }
    }
}

impl CompositorHandler for WinitState {
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
        // TODO(kchibisov) we need to expose it somehow in winit.
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }

    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &WlSurface,
        scale_factor: i32,
    ) {
        self.scale_factor_changed(surface, scale_factor as f64, true)
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &WlSurface, _: u32) {
        let window_id = super::make_wid(surface);
        let window = match self.windows.get_mut().get(&window_id) {
            Some(window) => window,
            None => return,
        };

        // In case we have a redraw requested we must indicate the wake up.
        if self
            .window_requests
            .get_mut()
            .get(&window_id)
            .unwrap()
            .redraw_requested
            .load(Ordering::Relaxed)
        {
            self.dispatched_events = true;
        }

        window.lock().unwrap().frame_callback_received();
    }
}

const WAYLAND_DND_MAX_BYTES: usize = 8 * 1024 * 1024;
const WAYLAND_DND_MIME_TYPES: [&str; 2] = ["text/uri-list", "text/plain"];

struct DndOfferState {
    data_device: WlDataDevice,
    offer: DragOffer,
    window_id: WindowId,
    data: Vec<u8>,
    paths: Option<Vec<PathBuf>>,
    read_token: Option<RegistrationToken>,
    dropped: bool,
}

impl WinitState {
    fn data_device_for(&self, device: &WlDataDevice) -> Option<&DataDevice> {
        self.seats.values().find_map(|seat| {
            seat.data_device
                .as_ref()
                .filter(|data_device| data_device.inner() == device)
        })
    }

    fn start_dnd_read(
        &mut self,
        data_device: &WlDataDevice,
        offer: DragOffer,
        window_id: WindowId,
        mime_type: String,
    ) {
        let read_pipe: ReadPipe = match offer.receive(mime_type) {
            Ok(read_pipe) => read_pipe,
            Err(error) => {
                tracing::debug!("Wayland drag offer could not be read: {error}");
                offer.finish();
                offer.destroy();
                return;
            }
        };

        let offer_key = offer.clone();
        let token = match self.loop_handle.insert_source(read_pipe, move |_, file, state| {
            let mut buffer = [0_u8; 16 * 1024];
            let mut source = file.as_ref();
            match source.read(&mut buffer) {
                Ok(0) => {
                    state.finish_dnd_read(&offer_key);
                    PostAction::Remove
                }
                Ok(bytes_read) => {
                    state.consume_dnd_bytes(&offer_key, &buffer[..bytes_read]);
                    PostAction::Continue
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    PostAction::Continue
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                    PostAction::Continue
                }
                Err(error) => {
                    tracing::debug!("Wayland drag offer read failed: {error}");
                    state.finish_dnd_read(&offer_key);
                    PostAction::Remove
                }
            }
        }) {
            Ok(token) => token,
            Err(error) => {
                tracing::debug!("Wayland drag offer pipe could not be registered: {error}");
                offer.finish();
                offer.destroy();
                return;
            }
        };

        self.dnd_offers.push(DndOfferState {
            data_device: data_device.clone(),
            offer,
            window_id,
            data: Vec::new(),
            paths: None,
            read_token: Some(token),
            dropped: false,
        });
    }

    fn consume_dnd_bytes(&mut self, offer: &DragOffer, bytes: &[u8]) {
        if let Some(active) = self.dnd_offers.iter_mut().find(|active| active.offer == *offer) {
            let remaining = WAYLAND_DND_MAX_BYTES.saturating_sub(active.data.len());
            active.data.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        }
    }

    fn finish_dnd_read(&mut self, offer: &DragOffer) {
        let Some(index) = self.dnd_offers.iter().position(|active| active.offer == *offer) else {
            return;
        };

        let (window_id, paths, dropped, offer) = {
            let active = &mut self.dnd_offers[index];
            let paths = parse_wayland_uri_list(&active.data);
            active.paths = Some(paths.clone());
            active.read_token = None;
            (active.window_id, paths, active.dropped, active.offer.clone())
        };

        for path in &paths {
            self.events_sink.push_window_event(WindowEvent::HoveredFile(path.clone()), window_id);
        }
        if !paths.is_empty() {
            self.dispatched_events = true;
        }

        if dropped {
            for path in paths {
                self.events_sink.push_window_event(WindowEvent::DroppedFile(path), window_id);
            }
            self.dispatched_events = true;
            offer.finish();
            offer.destroy();
            self.dnd_offers.remove(index);
        }
    }

    fn finish_dnd_drop(&mut self, data_device: &WlDataDevice) {
        let Some(index) = self
            .dnd_offers
            .iter()
            .position(|active| active.data_device == *data_device)
        else {
            return;
        };

        let active = &mut self.dnd_offers[index];
        active.dropped = true;
        let Some(paths) = active.paths.clone() else {
            return;
        };
        let window_id = active.window_id;
        let offer = active.offer.clone();
        let token = active.read_token.take();
        if let Some(token) = token {
            self.loop_handle.remove(token);
        }
        for path in paths {
            self.events_sink.push_window_event(WindowEvent::DroppedFile(path), window_id);
        }
        self.dispatched_events = true;
        offer.finish();
        offer.destroy();
        self.dnd_offers.remove(index);
    }

    fn cancel_dnd(&mut self, data_device: &WlDataDevice) {
        let mut index = self.dnd_offers.len();
        while index > 0 {
            index -= 1;
            if self.dnd_offers[index].data_device != *data_device {
                continue;
            }
            let active = self.dnd_offers.remove(index);
            if let Some(token) = active.read_token {
                self.loop_handle.remove(token);
            }
            if !active.dropped {
                self.events_sink
                    .push_window_event(WindowEvent::HoveredFileCancelled, active.window_id);
                self.dispatched_events = true;
            }
        }
    }

    /// Wayland keeps pointer motion for an external drag on the data-device
    /// protocol instead of the regular pointer protocol. Reuse winit's
    /// cursor event so clients such as egui can keep their drag preview under
    /// the pointer while the compositor owns the drag gesture.
    fn dispatch_dnd_motion(&mut self, data_device: &WlDataDevice, x: f64, y: f64) {
        let Some(window_id) = self
            .dnd_offers
            .iter()
            .find(|active| active.data_device == *data_device)
            .map(|active| active.window_id)
        else {
            return;
        };

        let scale_factor = self
            .windows
            .get_mut()
            .get(&window_id)
            .map(|window| window.lock().unwrap().scale_factor())
            .unwrap_or(1.0);
        let position = LogicalPosition::new(x, y).to_physical(scale_factor);
        let device_id = crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(
            DeviceId::dummy(),
        ));

        self.events_sink
            .push_window_event(WindowEvent::CursorMoved { device_id, position }, window_id);
        self.dispatched_events = true;
    }
}

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &WlSurface,
    ) {
        let Some(data_device_state) = self.data_device_for(data_device) else {
            return;
        };
        let Some(offer) = data_device_state.data().drag_offer() else {
            return;
        };
        let Some(mime_type) = offer.with_mime_types(|mime_types| {
            WAYLAND_DND_MIME_TYPES
                .iter()
                .find_map(|supported| mime_types.iter().find(|mime| mime == supported).cloned())
        }) else {
            return;
        };

        self.cancel_dnd(data_device);
        offer.accept_mime_type(offer.serial, Some(mime_type.clone()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        self.start_dnd_read(data_device, offer, super::make_wid(surface), mime_type);
        self.dispatch_dnd_motion(data_device, x, y);
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, data_device: &WlDataDevice) {
        self.cancel_dnd(data_device);
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
    ) {
        self.dispatch_dnd_motion(data_device, x, y);
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        data_device: &WlDataDevice,
    ) {
        self.finish_dnd_drop(data_device);
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        offer: &mut DragOffer,
        _actions: DndAction,
    ) {
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut DragOffer,
        _actions: DndAction,
    ) {
    }
}

fn parse_wayland_uri_list(data: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(parse_wayland_uri)
        .collect()
}

fn parse_wayland_uri(value: &str) -> Option<PathBuf> {
    let encoded_path = if let Some(value) = value.strip_prefix("file://") {
        if value.starts_with('/') {
            value
        } else {
            let (host, path) = value.split_once('/')?;
            if !host.is_empty() && host != "localhost" {
                return None;
            }
            path
        }
    } else if value.starts_with('/') {
        value
    } else {
        return None;
    };

    let mut decoded = Vec::with_capacity(encoded_path.len());
    let bytes = encoded_path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

impl ProvidesRegistryState for WinitState {
    sctk::registry_handlers![OutputState, SeatState];

    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
}

// The window update coming from the compositor.
#[derive(Debug, Clone, Copy)]
pub struct WindowCompositorUpdate {
    /// The id of the window this updates belongs to.
    pub window_id: WindowId,

    /// New window size.
    pub resized: bool,

    /// New scale factor.
    pub scale_changed: bool,

    /// Close the window.
    pub close_window: bool,
}

impl WindowCompositorUpdate {
    fn new(window_id: WindowId) -> Self {
        Self { window_id, resized: false, scale_changed: false, close_window: false }
    }
}

sctk::delegate_subcompositor!(WinitState);
sctk::delegate_compositor!(WinitState);
sctk::delegate_output!(WinitState);
sctk::delegate_registry!(WinitState);
sctk::delegate_shm!(WinitState);
sctk::delegate_xdg_shell!(WinitState);
sctk::delegate_xdg_window!(WinitState);
sctk::reexports::client::delegate_dispatch!(WinitState: [
    sctk::reexports::client::protocol::wl_data_device_manager::WlDataDeviceManager: sctk::globals::GlobalData
] => sctk::data_device_manager::DataDeviceManagerState);
sctk::reexports::client::delegate_dispatch!(WinitState: [
    sctk::reexports::client::protocol::wl_data_offer::WlDataOffer: sctk::data_device_manager::data_offer::DataOfferData
] => sctk::data_device_manager::DataDeviceManagerState);
sctk::reexports::client::delegate_dispatch!(WinitState: [
    sctk::reexports::client::protocol::wl_data_device::WlDataDevice: sctk::data_device_manager::data_device::DataDeviceData
] => sctk::data_device_manager::DataDeviceManagerState);
