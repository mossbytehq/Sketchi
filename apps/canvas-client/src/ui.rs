use std::fmt::Write as _;
use std::{
    collections::{BTreeSet, HashMap, hash_map::Entry},
    env, fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use canvas_core::{
    Color, Document, EdgeStyle, EditorCommand, Element, ElementId, ElementKind, EmbeddedImage,
    Point, Size, Sloppiness, StrokeStyle, Style, StylePatch, TextAlign, TextFontFamily, Transform,
};
use canvas_renderer::{Camera, hit_test};
use egui::epaint::{TextShape, Vertex};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Key, Margin, Mesh, Modifiers, Painter,
    PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::{
    components::{
        STANDARD_CONTROL_SIZE, button, color_picker_editor, color_picker_trigger, color_swatch,
        dropdown_field, numeric_field, numeric_field_with_decimals, range_slider, text_field,
    },
    editor::Editor,
    images::{
        ImageImportError, embedded_image_from_bytes, embedded_image_from_rgba,
        embedded_image_with_rgba,
    },
    remix_icons::{self, RemixIcon as Icon},
    selection::{
        SelectionHandle, angle_delta, element_bounds, group_resized_element, marquee_intersects,
        padded_selection_corners, padded_selection_handle_at, padded_selection_handle_position,
        padded_selection_over_rotation_handle, padded_selection_rotation_handle_position,
        pointer_angle, rotate_around, selection_bounds, selection_handle_at_bounds,
        selection_handle_position, translated_element,
    },
    tools::{Tool, ToolController, ToolOutput},
};

const DARK_CANVAS: Color32 = Color32::from_rgb(26, 27, 30);
const LIGHT_CANVAS: Color32 = Color32::from_rgb(252, 252, 253);
const ACCENT: Color32 = Color32::from_rgb(91, 87, 214);
const LIGHT_PANEL: Color32 = Color32::from_rgb(255, 255, 255);
const DARK_PANEL: Color32 = Color32::from_rgb(37, 38, 43);
const LIGHT_BORDER: Color32 = Color32::from_rgb(229, 231, 235);
const DARK_BORDER: Color32 = Color32::from_rgb(62, 64, 72);
const LIGHT_TEXT: Color32 = Color32::from_rgb(38, 39, 43);
const DARK_TEXT: Color32 = Color32::from_rgb(232, 233, 237);
const LIGHT_MUTED: Color32 = Color32::from_rgb(107, 111, 122);
const DARK_MUTED: Color32 = Color32::from_rgb(160, 163, 174);
const SELECTION_STROKE_WIDTH: f32 = 2.0;

const STROKE_COLORS: [Color32; 7] = [
    Color32::from_rgb(31, 31, 31),
    Color32::from_rgb(224, 49, 49),
    Color32::from_rgb(47, 158, 68),
    Color32::from_rgb(25, 113, 194),
    Color32::from_rgb(240, 140, 0),
    Color32::from_rgb(121, 80, 242),
    Color32::from_rgb(255, 255, 255),
];

const FILL_COLORS: [Color32; 6] = [
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(255, 201, 201),
    Color32::from_rgb(190, 242, 200),
    Color32::from_rgb(190, 224, 255),
    Color32::from_rgb(255, 236, 153),
    Color32::from_rgb(221, 214, 254),
];

const COLOR_PICKER_COLORS: [Color32; 15] = [
    Color32::from_rgb(31, 31, 31),
    Color32::from_rgb(82, 82, 91),
    Color32::from_rgb(145, 145, 155),
    Color32::from_rgb(224, 224, 230),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(224, 49, 49),
    Color32::from_rgb(245, 159, 0),
    Color32::from_rgb(47, 158, 68),
    Color32::from_rgb(25, 113, 194),
    Color32::from_rgb(121, 80, 242),
    Color32::from_rgb(190, 242, 200),
    Color32::from_rgb(190, 224, 255),
    Color32::from_rgb(255, 236, 153),
    Color32::from_rgb(255, 201, 201),
    Color32::from_rgb(221, 214, 254),
];

/// Immediate-mode UI state for the desktop whiteboard workspace.
pub(crate) struct WorkspaceUi {
    active_tool: Tool,
    dark_mode: bool,
    system_dark_mode: Option<bool>,
    appearance: AppearanceMode,
    settings_open: bool,
    settings_page: SettingsPage,
    restore_session: bool,
    autosave_interval: AutosaveInterval,
    autosave_directory: String,
    light_canvas_color: Color32,
    dark_canvas_color: Color32,
    light_palette: [Color32; 7],
    dark_palette: [Color32; 7],
    stabilization: f32,
    pressure_sensitivity: f32,
    keybinds: Keybinds,
    capturing_keybind: Option<KeybindAction>,
    status: String,
    selected: BTreeSet<ElementId>,
    selection_gesture: Option<SelectionGesture>,
    draft_style: Style,
    custom_font_size: CustomFontSizeState,
    text_edit: Option<TextEditState>,
    color_picker: Option<ColorPickerTarget>,
    color_picker_detail: Option<()>,
    clipboard: Option<arboard::Clipboard>,
    clipboard_paste_requested: Option<()>,
    drop_hovered: Option<()>,
    egui_hovered_path: Option<PathBuf>,
    egui_hovered_file_count: usize,
    egui_dropped_file_count: usize,
    pending_dropped_files: Vec<PathBuf>,
    drop_preview: Option<DropPreview>,
    drop_preview_decode: Option<Receiver<DropPreviewDecode>>,
    drop_screen_position: Option<Pos2>,
    image_textures: HashMap<ElementId, egui::TextureHandle>,
}

#[derive(Clone, Debug)]
enum SelectionGesture {
    Marquee {
        start: Point,
        current: Point,
        additive: bool,
    },
    Move {
        elements: Vec<Element>,
        pointer_start: Point,
        pointer_current: Point,
    },
    Resize {
        elements: Vec<Element>,
        bounds: canvas_core::Rect,
        handle: SelectionHandle,
        pointer_current: Point,
        pointer_offset: Point,
    },
    Rotate {
        element: Element,
        center: Point,
        start_angle: f32,
        current_angle: f32,
    },
}

#[derive(Clone, Debug)]
struct TextEditState {
    element_id: Option<ElementId>,
    position: Point,
    rotation: f32,
    text: String,
    style: Style,
    just_started: bool,
}

struct DropPreview {
    path: PathBuf,
    image: Option<EmbeddedImage>,
    texture: Option<egui::TextureHandle>,
}

struct DropPreviewDecode {
    path: PathBuf,
    result: Result<(EmbeddedImage, Vec<u8>), DropPreviewDecodeError>,
}

enum DropPreviewDecodeError {
    Read(String),
    Decode(ImageImportError),
}

fn start_drop_preview_decode(path: PathBuf) -> Receiver<DropPreviewDecode> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        tracing::info!(
            path = %path.display(),
            "preparing hovered image preview"
        );
        let result = match fs::read(&path) {
            Ok(bytes) => {
                tracing::info!(
                    path = %path.display(),
                    bytes = bytes.len(),
                    "hovered image preview bytes read"
                );
                embedded_image_with_rgba(bytes).map_err(DropPreviewDecodeError::Decode)
            }
            Err(error) => Err(DropPreviewDecodeError::Read(error.to_string())),
        };
        let _ = sender.send(DropPreviewDecode { path, result });
    });
    receiver
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorPickerTarget {
    Stroke,
    Fill,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SettingsPage {
    General,
    Keybinds,
    Input,
    About,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppearanceMode {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CustomFontSizeState {
    #[default]
    Closed,
    Open,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutosaveInterval {
    ThirtySeconds,
    OneMinute,
    FiveMinutes,
    TenMinutes,
    Never,
}

impl AutosaveInterval {
    const ALL: [Self; 5] = [
        Self::ThirtySeconds,
        Self::OneMinute,
        Self::FiveMinutes,
        Self::TenMinutes,
        Self::Never,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::ThirtySeconds => "30 seconds",
            Self::OneMinute => "1 minute",
            Self::FiveMinutes => "5 minutes",
            Self::TenMinutes => "10 minutes",
            Self::Never => "Never",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeyBinding {
    key: Key,
    modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Keybinds {
    select: KeyBinding,
    text: KeyBinding,
    freehand: KeyBinding,
    rectangle: KeyBinding,
    diamond: KeyBinding,
    triangle: KeyBinding,
    ellipse: KeyBinding,
    line: KeyBinding,
    arrow: KeyBinding,
    pan: KeyBinding,
    select_all: KeyBinding,
    delete: KeyBinding,
    undo: KeyBinding,
    redo: KeyBinding,
    new_document: KeyBinding,
    save: KeyBinding,
    settings: KeyBinding,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            select: KeyBinding {
                key: Key::V,
                modifiers: Modifiers::NONE,
            },
            text: KeyBinding {
                key: Key::T,
                modifiers: Modifiers::NONE,
            },
            freehand: KeyBinding {
                key: Key::P,
                modifiers: Modifiers::NONE,
            },
            rectangle: KeyBinding {
                key: Key::R,
                modifiers: Modifiers::NONE,
            },
            diamond: KeyBinding {
                key: Key::D,
                modifiers: Modifiers::NONE,
            },
            triangle: KeyBinding {
                key: Key::Y,
                modifiers: Modifiers::NONE,
            },
            ellipse: KeyBinding {
                key: Key::O,
                modifiers: Modifiers::NONE,
            },
            line: KeyBinding {
                key: Key::L,
                modifiers: Modifiers::NONE,
            },
            arrow: KeyBinding {
                key: Key::A,
                modifiers: Modifiers::NONE,
            },
            pan: KeyBinding {
                key: Key::H,
                modifiers: Modifiers::NONE,
            },
            select_all: KeyBinding {
                key: Key::A,
                modifiers: Modifiers::CTRL,
            },
            delete: KeyBinding {
                key: Key::Backspace,
                modifiers: Modifiers::NONE,
            },
            undo: KeyBinding {
                key: Key::Z,
                modifiers: Modifiers::CTRL,
            },
            redo: KeyBinding {
                key: Key::Y,
                modifiers: Modifiers::CTRL,
            },
            new_document: KeyBinding {
                key: Key::N,
                modifiers: Modifiers::CTRL,
            },
            save: KeyBinding {
                key: Key::S,
                modifiers: Modifiers::CTRL,
            },
            settings: KeyBinding {
                key: Key::Comma,
                modifiers: Modifiers::CTRL,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeybindAction {
    Select,
    Text,
    Freehand,
    Rectangle,
    Diamond,
    Triangle,
    Ellipse,
    Line,
    Arrow,
    Pan,
    SelectAll,
    Delete,
    Undo,
    Redo,
    NewDocument,
    Save,
    Settings,
}

impl KeybindAction {
    const ALL: [Self; 17] = [
        Self::Select,
        Self::Text,
        Self::Freehand,
        Self::Rectangle,
        Self::Diamond,
        Self::Triangle,
        Self::Ellipse,
        Self::Line,
        Self::Arrow,
        Self::Pan,
        Self::SelectAll,
        Self::Delete,
        Self::Undo,
        Self::Redo,
        Self::NewDocument,
        Self::Save,
        Self::Settings,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Select => "Select tool",
            Self::Text => "Text tool",
            Self::Freehand => "Freehand tool",
            Self::Rectangle => "Rectangle tool",
            Self::Diamond => "Diamond tool",
            Self::Triangle => "Triangle tool",
            Self::Ellipse => "Ellipse tool",
            Self::Line => "Line tool",
            Self::Arrow => "Arrow tool",
            Self::Pan => "Pan canvas",
            Self::SelectAll => "Select all",
            Self::Delete => "Delete selection",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::NewDocument => "New whiteboard",
            Self::Save => "Save locally",
            Self::Settings => "Settings",
        }
    }

    const fn tool(self) -> Option<Tool> {
        match self {
            Self::Select => Some(Tool::Select),
            Self::Text => Some(Tool::Text),
            Self::Freehand => Some(Tool::Freehand),
            Self::Rectangle => Some(Tool::Rectangle),
            Self::Diamond => Some(Tool::Diamond),
            Self::Triangle => Some(Tool::Triangle),
            Self::Ellipse => Some(Tool::Ellipse),
            Self::Line => Some(Tool::Line),
            Self::Arrow => Some(Tool::Arrow),
            Self::Pan => Some(Tool::Pan),
            Self::SelectAll
            | Self::Delete
            | Self::Undo
            | Self::Redo
            | Self::NewDocument
            | Self::Save
            | Self::Settings => None,
        }
    }
}

impl Keybinds {
    fn binding(self, action: KeybindAction) -> KeyBinding {
        match action {
            KeybindAction::Select => self.select,
            KeybindAction::Text => self.text,
            KeybindAction::Freehand => self.freehand,
            KeybindAction::Rectangle => self.rectangle,
            KeybindAction::Diamond => self.diamond,
            KeybindAction::Triangle => self.triangle,
            KeybindAction::Ellipse => self.ellipse,
            KeybindAction::Line => self.line,
            KeybindAction::Arrow => self.arrow,
            KeybindAction::Pan => self.pan,
            KeybindAction::SelectAll => self.select_all,
            KeybindAction::Delete => self.delete,
            KeybindAction::Undo => self.undo,
            KeybindAction::Redo => self.redo,
            KeybindAction::NewDocument => self.new_document,
            KeybindAction::Save => self.save,
            KeybindAction::Settings => self.settings,
        }
    }

    fn set_binding(&mut self, action: KeybindAction, binding: KeyBinding) {
        match action {
            KeybindAction::Select => self.select = binding,
            KeybindAction::Text => self.text = binding,
            KeybindAction::Freehand => self.freehand = binding,
            KeybindAction::Rectangle => self.rectangle = binding,
            KeybindAction::Diamond => self.diamond = binding,
            KeybindAction::Triangle => self.triangle = binding,
            KeybindAction::Ellipse => self.ellipse = binding,
            KeybindAction::Line => self.line = binding,
            KeybindAction::Arrow => self.arrow = binding,
            KeybindAction::Pan => self.pan = binding,
            KeybindAction::SelectAll => self.select_all = binding,
            KeybindAction::Delete => self.delete = binding,
            KeybindAction::Undo => self.undo = binding,
            KeybindAction::Redo => self.redo = binding,
            KeybindAction::NewDocument => self.new_document = binding,
            KeybindAction::Save => self.save = binding,
            KeybindAction::Settings => self.settings = binding,
        }
    }

    fn action_for(self, binding: KeyBinding) -> Option<KeybindAction> {
        KeybindAction::ALL
            .into_iter()
            .find(|action| self.binding(*action) == binding)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LayerAction {
    SendBackward,
    BringForward,
    SendToBack,
    BringToFront,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlignAction {
    Left,
    CenterHorizontal,
    Right,
    Top,
    CenterVertical,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ElementAction {
    Duplicate,
    Delete,
    CopyLink,
}

fn reordered_layer_ids(
    ordered: &[ElementId],
    selected: &BTreeSet<ElementId>,
    action: LayerAction,
) -> Vec<ElementId> {
    let mut next = ordered.to_vec();
    match action {
        LayerAction::SendToBack | LayerAction::BringToFront => {
            let selected_ids = ordered
                .iter()
                .copied()
                .filter(|element_id| selected.contains(element_id))
                .collect::<Vec<_>>();
            let other_ids = ordered
                .iter()
                .copied()
                .filter(|element_id| !selected.contains(element_id))
                .collect::<Vec<_>>();
            next.clear();
            if action == LayerAction::SendToBack {
                next.extend(selected_ids);
                next.extend(other_ids);
            } else {
                next.extend(other_ids);
                next.extend(selected_ids);
            }
        }
        LayerAction::SendBackward => {
            for index in 1..next.len() {
                let should_swap = next
                    .get(index)
                    .is_some_and(|element_id| selected.contains(element_id))
                    && next
                        .get(index - 1)
                        .is_some_and(|element_id| !selected.contains(element_id));
                if should_swap {
                    next.swap(index - 1, index);
                }
            }
        }
        LayerAction::BringForward => {
            for index in (0..next.len().saturating_sub(1)).rev() {
                let should_swap = next
                    .get(index)
                    .is_some_and(|element_id| selected.contains(element_id))
                    && next
                        .get(index + 1)
                        .is_some_and(|element_id| !selected.contains(element_id));
                if should_swap {
                    next.swap(index, index + 1);
                }
            }
        }
    }
    next
}

impl SelectionGesture {
    fn preview_elements(&self) -> Vec<Element> {
        match self {
            Self::Marquee { .. } => Vec::new(),
            Self::Move {
                elements,
                pointer_start,
                pointer_current,
            } => {
                let delta = Point::new(
                    pointer_current.x - pointer_start.x,
                    pointer_current.y - pointer_start.y,
                );
                elements
                    .iter()
                    .map(|element| translated_element(element, delta))
                    .collect()
            }
            Self::Resize {
                elements,
                bounds,
                handle,
                pointer_current,
                ..
            } => elements
                .iter()
                .map(|element| {
                    if elements.len() == 1 {
                        let mut preview = element.clone();
                        preview.transform =
                            crate::selection::resize_transform(element, *handle, *pointer_current);
                        preview
                    } else {
                        group_resized_element(element, *bounds, *handle, *pointer_current)
                    }
                })
                .collect(),
            Self::Rotate {
                element,
                center,
                start_angle,
                current_angle,
            } => {
                let mut preview = element.clone();
                preview.transform.rotation =
                    element.transform.rotation + angle_delta(*start_angle, *current_angle);
                let _ = center;
                vec![preview]
            }
        }
    }

    fn is_marquee(&self) -> bool {
        matches!(self, Self::Marquee { .. })
    }
}

impl Default for WorkspaceUi {
    fn default() -> Self {
        Self {
            active_tool: Tool::Select,
            dark_mode: false,
            system_dark_mode: None,
            appearance: AppearanceMode::System,
            settings_open: false,
            settings_page: SettingsPage::General,
            restore_session: true,
            autosave_interval: AutosaveInterval::OneMinute,
            autosave_directory: String::from("~/.cache/sketchi/sketchi.autosave"),
            light_canvas_color: LIGHT_CANVAS,
            dark_canvas_color: DARK_CANVAS,
            light_palette: STROKE_COLORS,
            dark_palette: [
                Color32::from_rgb(245, 245, 245),
                Color32::from_rgb(224, 49, 49),
                Color32::from_rgb(82, 196, 104),
                Color32::from_rgb(66, 153, 225),
                Color32::from_rgb(245, 159, 0),
                Color32::from_rgb(145, 120, 242),
                Color32::from_rgb(31, 35, 45),
            ],
            stabilization: 0.5,
            pressure_sensitivity: 0.5,
            keybinds: Keybinds::default(),
            capturing_keybind: None,
            status: String::from("Select a tool to start drawing"),
            selected: BTreeSet::new(),
            selection_gesture: None,
            draft_style: Style::default(),
            custom_font_size: CustomFontSizeState::Closed,
            text_edit: None,
            color_picker: None,
            color_picker_detail: None,
            clipboard: None,
            clipboard_paste_requested: None,
            drop_hovered: None,
            egui_hovered_path: None,
            egui_hovered_file_count: 0,
            egui_dropped_file_count: 0,
            pending_dropped_files: Vec::new(),
            drop_preview: None,
            drop_preview_decode: None,
            drop_screen_position: None,
            image_textures: HashMap::new(),
        }
    }
}

impl WorkspaceUi {
    /// Sets whether the last native-window geometry should be restored.
    pub(crate) fn set_restore_session(&mut self, restore_session: bool) {
        self.restore_session = restore_session;
    }

    /// Returns whether the last native-window geometry should be restored.
    pub(crate) const fn restore_session_enabled(&self) -> bool {
        self.restore_session
    }

    /// Returns whether the native settings window should be open.
    pub(crate) const fn settings_open(&self) -> bool {
        self.settings_open
    }

    /// Closes the native settings window and abandons an in-progress shortcut capture.
    pub(crate) fn close_settings(&mut self) {
        self.settings_open = false;
        self.capturing_keybind = None;
    }

    /// Requests a native clipboard image paste on the next canvas frame.
    pub(crate) fn request_clipboard_image_paste(&mut self) {
        self.clipboard_paste_requested = Some(());
    }

    /// Updates the visual drop target state from native window events.
    pub(crate) fn set_drop_hovered(&mut self, hovered: bool) {
        self.drop_hovered = hovered.then_some(());
        if !hovered {
            self.egui_hovered_path = None;
            self.drop_preview = None;
            self.drop_preview_decode = None;
            self.drop_screen_position = None;
        }
    }

    /// Stores the latest native drag position in egui points. Wayland sends
    /// external-drag motion separately from regular pointer motion, so this
    /// position is kept independently of egui's pointer state.
    pub(crate) fn set_drop_position(&mut self, position: Pos2) -> bool {
        self.drop_screen_position = Some(position);
        self.drop_hovered.is_some()
    }

    /// Stores the file currently being dragged over the canvas so it can be
    /// decoded and previewed before the final drop event arrives.
    pub(crate) fn set_drop_preview(&mut self, path: PathBuf) {
        self.drop_hovered = Some(());
        let path_changed = self
            .drop_preview
            .as_ref()
            .is_none_or(|preview| preview.path != path);
        if path_changed {
            self.drop_preview_decode = Some(start_drop_preview_decode(path.clone()));
            self.drop_preview = Some(DropPreview {
                path,
                image: None,
                texture: None,
            });
        }
    }

    /// Queues a native file drop for import on the next canvas frame.
    pub(crate) fn queue_dropped_file(&mut self, path: PathBuf) {
        self.pending_dropped_files.push(path);
        self.egui_hovered_path = None;
        self.drop_hovered = None;
        self.drop_preview = None;
        self.drop_preview_decode = None;
        self.drop_screen_position = None;
    }

    /// Updates the UI to follow the desktop theme until the user overrides it.
    pub(crate) fn set_system_dark_mode(&mut self, dark_mode: bool) {
        self.system_dark_mode = Some(dark_mode || kde_system_dark_mode().unwrap_or(false));
        if self.appearance == AppearanceMode::System {
            self.dark_mode = self.system_dark_mode.unwrap_or(false);
            self.sync_draft_palette();
        }
    }

    /// Draws the canvas, toolbars, properties, and local editor controls.
    pub(crate) fn show(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        tools: &mut ToolController,
        camera: &mut Camera,
    ) {
        if self.appearance == AppearanceMode::System {
            self.dark_mode = self.system_dark_mode.unwrap_or(false);
        }
        context.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        self.handle_keybind_input(context, editor, tools);
        self.show_canvas(context, editor, tools, camera);
        self.show_text_editor(context, editor, camera);
        self.show_file_actions(context, editor);
        self.show_tool_palette(context, tools);
        self.show_history(context, editor);
        self.show_properties(context, editor);
        self.show_color_picker(context, editor);
        self.show_zoom_controls(context, camera);
        self.show_status(context, editor, camera);
    }

    #[allow(clippy::too_many_lines)]
    fn show_canvas(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        tools: &mut ToolController,
        camera: &mut Camera,
    ) {
        let canvas_color = if self.dark_mode {
            self.dark_canvas_color
        } else {
            self.light_canvas_color
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(canvas_color))
            .show(context, |ui| {
                let canvas_rect = ui.max_rect();
                camera.set_viewport(Size::new(canvas_rect.width(), canvas_rect.height()));
                let response = ui.interact(
                    canvas_rect,
                    Id::new("sketchi.canvas"),
                    Sense::click_and_drag(),
                );
                let navigation_handled =
                    self.handle_canvas_navigation(ui, &response, tools, camera, canvas_rect);
                let dragging = response.dragged() || ui.input(|input| input.pointer.middle_down());
                self.sync_egui_drop_input(context);
                self.handle_dropped_images(context, editor, camera, canvas_rect);
                self.handle_clipboard_image(context, editor, camera, canvas_rect);
                self.prepare_drop_preview(context);
                let input_document = editor.document();
                if !navigation_handled {
                    self.handle_canvas_input(ui, &response, &input_document, editor, tools, camera);
                }
                let cursor = if navigation_handled && ui.input(|input| input.pointer.middle_down())
                {
                    CursorIcon::Grabbing
                } else {
                    self.canvas_cursor(&input_document, &response, *camera, dragging)
                };
                let response = response.on_hover_cursor(cursor);
                let painter = ui.painter_at(canvas_rect);
                paint_dot_grid(&painter, canvas_rect, *camera, self.dark_mode);
                if self.drop_hovered.is_some() {
                    paint_drop_target(&painter, canvas_rect, self.dark_mode);
                }
                let document = editor.document();
                let hovered_element =
                    if self.active_tool == Tool::Select && self.selection_gesture.is_none() {
                        response.hover_pos().and_then(|position| {
                            let world = screen_to_world(*camera, position);
                            hit_test(&document, world, selection_tolerance(camera.zoom()))
                        })
                    } else {
                        None
                    };
                let selection_preview = self
                    .selection_gesture
                    .as_ref()
                    .map(SelectionGesture::preview_elements)
                    .unwrap_or_default();
                let hidden = if selection_preview.is_empty() {
                    None
                } else {
                    Some(&self.selected)
                };
                paint_document(
                    &painter,
                    &document,
                    *camera,
                    hidden,
                    self.text_edit
                        .as_ref()
                        .and_then(|text_edit| text_edit.element_id),
                    &mut self.image_textures,
                );
                self.paint_drop_preview(context, &painter, *camera, canvas_rect);
                if let Some(text_edit) = self.text_edit.as_ref() {
                    paint_text_edit_preview(&painter, text_edit, *camera);
                }
                if let Some(mut preview) = tools.preview() {
                    preview.style = self.draft_style;
                    paint_element(&painter, &preview, *camera, &mut self.image_textures);
                }
                if !selection_preview.is_empty() {
                    for preview in &selection_preview {
                        paint_element(&painter, preview, *camera, &mut self.image_textures);
                    }
                    let elements = selection_preview.iter().collect::<Vec<_>>();
                    paint_selection(&painter, &elements, *camera, self.dark_mode);
                } else if !self.selected.is_empty()
                    && !self
                        .selection_gesture
                        .as_ref()
                        .is_some_and(SelectionGesture::is_marquee)
                {
                    let elements = self
                        .selected
                        .iter()
                        .filter_map(|id| document.element(*id))
                        .collect::<Vec<_>>();
                    paint_selection(&painter, &elements, *camera, self.dark_mode);
                }
                if let Some(hovered_id) = hovered_element
                    && !self.selected.contains(&hovered_id)
                    && let Some(element) = document.element(hovered_id)
                {
                    paint_hover_outline(&painter, element, *camera, self.dark_mode);
                }
                if let Some(SelectionGesture::Marquee { start, current, .. }) =
                    self.selection_gesture.as_ref()
                {
                    paint_marquee(&painter, *start, *current, *camera);
                }
            });
    }

    fn prepare_drop_preview(&mut self, context: &egui::Context) {
        let result = self.drop_preview_decode.as_ref().map(Receiver::try_recv);
        let Some(result) = result else {
            return;
        };

        match result {
            Ok(decoded) => {
                self.drop_preview_decode = None;
                let Some(preview) = self.drop_preview.as_ref() else {
                    return;
                };
                if preview.path != decoded.path {
                    return;
                }
                let path = decoded.path;
                let (image, rgba) = match decoded.result {
                    Ok(decoded) => decoded,
                    Err(DropPreviewDecodeError::Read(error)) => {
                        tracing::warn!(
                            path = %path.display(),
                            error,
                            "hovered image preview read failed"
                        );
                        return;
                    }
                    Err(DropPreviewDecodeError::Decode(error)) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %error,
                            "hovered image preview decode failed"
                        );
                        return;
                    }
                };
                let width = usize::try_from(image.width).unwrap_or(usize::MAX);
                let height = usize::try_from(image.height).unwrap_or(usize::MAX);
                let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                let texture = context.load_texture(
                    "sketchi.drop-preview",
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                if let Some(preview) = self.drop_preview.as_mut() {
                    preview.image = Some(image);
                    preview.texture = Some(texture);
                }
                tracing::info!(
                    path = %path.display(),
                    width,
                    height,
                    "hovered image preview texture ready"
                );
            }
            Err(TryRecvError::Empty) => {
                context.request_repaint_after(Duration::from_millis(16));
            }
            Err(TryRecvError::Disconnected) => {
                self.drop_preview_decode = None;
                tracing::warn!("hovered image preview worker stopped before returning a result");
            }
        }
    }

    fn sync_egui_drop_input(&mut self, context: &egui::Context) {
        let (hovered_file_count, hovered_path_count, hovered_mime_count) = context.input(|input| {
            let files = &input.raw.hovered_files;
            (
                files.len(),
                files.iter().filter(|file| file.path.is_some()).count(),
                files.iter().filter(|file| !file.mime.is_empty()).count(),
            )
        });
        let hovered_state_changed = hovered_file_count != self.egui_hovered_file_count;
        if hovered_state_changed {
            tracing::info!(
                files = hovered_file_count,
                paths = hovered_path_count,
                mime_entries = hovered_mime_count,
                "egui hovered-file state changed"
            );
        }
        self.egui_hovered_file_count = hovered_file_count;
        let hovered_path = context.input(|input| {
            input
                .raw
                .hovered_files
                .iter()
                .rev()
                .find_map(|file| file.path.clone())
        });
        if hovered_state_changed && hovered_file_count > 0 && hovered_path.is_none() {
            tracing::warn!("egui reported hovered files, but no filesystem path was available");
        }
        if let Some(path) = hovered_path {
            let path_changed = self.egui_hovered_path.as_ref() != Some(&path);
            if path_changed {
                tracing::info!(
                    path = %path.display(),
                    "egui forwarded hovered-file payload"
                );
            }
            self.egui_hovered_path = Some(path.clone());
            self.set_drop_preview(path);
        } else {
            if self.egui_hovered_path.take().is_some() {
                tracing::info!("egui cleared hovered-file payload");
            }
            if self.pending_dropped_files.is_empty() && self.drop_hovered.is_some() {
                tracing::warn!(
                    "native hover state had no egui file payload; clearing image preview"
                );
                self.set_drop_hovered(false);
            }
        }

        if self.pending_dropped_files.is_empty() {
            let (dropped_file_count, dropped_path_count, dropped_byte_count) =
                context.input(|input| {
                    let files = &input.raw.dropped_files;
                    (
                        files.len(),
                        files.iter().filter(|file| file.path.is_some()).count(),
                        files.iter().filter(|file| file.bytes.is_some()).count(),
                    )
                });
            let dropped_state_changed = dropped_file_count != self.egui_dropped_file_count;
            if dropped_state_changed {
                tracing::info!(
                    files = dropped_file_count,
                    paths = dropped_path_count,
                    byte_payloads = dropped_byte_count,
                    "egui dropped-file state changed"
                );
            }
            self.egui_dropped_file_count = dropped_file_count;
            let dropped_paths = context.input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .filter_map(|file| file.path.clone())
                    .collect::<Vec<_>>()
            });
            if dropped_state_changed && dropped_file_count > 0 && dropped_paths.is_empty() {
                tracing::warn!("egui reported dropped files, but no filesystem path was available");
            }
            if !dropped_paths.is_empty() {
                tracing::info!(
                    count = dropped_paths.len(),
                    paths = ?dropped_paths,
                    "egui forwarded dropped-file payload"
                );
            }
            for path in dropped_paths {
                self.queue_dropped_file(path);
            }
        }
    }

    fn paint_drop_preview(
        &self,
        context: &egui::Context,
        painter: &Painter,
        camera: Camera,
        canvas: Rect,
    ) {
        if self.drop_hovered.is_none() {
            return;
        }
        let Some(preview) = self.drop_preview.as_ref() else {
            return;
        };
        let Some(image) = preview.image.as_ref() else {
            return;
        };
        let Some(texture) = preview.texture.as_ref() else {
            return;
        };
        let screen_position = self
            .drop_screen_position
            .filter(|position| canvas.contains(*position))
            .or_else(|| {
                context
                    .input(|input| input.pointer.latest_pos())
                    .filter(|position| canvas.contains(*position))
            })
            .unwrap_or_else(|| canvas.center());
        let world_position = screen_to_world(camera, screen_position);
        let size = image_display_size(image.width, image.height);
        let min = camera.world_to_screen(world_position);
        let rect = Rect::from_min_size(
            Pos2::new(min.x, min.y),
            Vec2::new(size.width * camera.zoom(), size.height * camera.zoom()),
        );
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::from_white_alpha(220),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(1.0_f32, ACCENT),
            StrokeKind::Middle,
        );
    }

    fn handle_dropped_images(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        camera: &Camera,
        canvas: Rect,
    ) {
        let dropped_files = std::mem::take(&mut self.pending_dropped_files);
        if dropped_files.is_empty() {
            return;
        }
        tracing::info!(count = dropped_files.len(), "processing queued image drops");
        let screen_position = context
            .input(|input| input.pointer.latest_pos())
            .filter(|position| canvas.contains(*position))
            .unwrap_or_else(|| canvas.center());
        let world_position = screen_to_world(*camera, screen_position);
        let mut imported = 0_usize;
        let mut last_error = None;

        for (index, path) in dropped_files.iter().enumerate() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("dropped file");
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "dropped image read failed"
                    );
                    last_error = Some(format!("Could not read {name}: {error}"));
                    continue;
                }
            };
            tracing::info!(
                path = %path.display(),
                bytes = bytes.len(),
                "dropped image bytes read"
            );
            let image = match embedded_image_from_bytes(bytes) {
                Ok(image) => image,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "dropped image decode failed"
                    );
                    last_error = Some(format!("Could not import {name}: {error}"));
                    continue;
                }
            };
            let offset = f32::from(u16::try_from(index).unwrap_or(u16::MAX)) * 24.0;
            let position = Point::new(world_position.x + offset, world_position.y + offset);
            let size = image_display_size(image.width, image.height);
            let element_id = ElementId::new();
            let mut element = Element::image(element_id, Transform::new(position, size), image);
            element.style = self.draft_style;
            match Self::execute_editor_command(editor, EditorCommand::Create(element)) {
                Ok(_) => {
                    tracing::info!(
                        path = %path.display(),
                        element_id = ?element_id,
                        "dropped image embedded in document"
                    );
                    imported += 1;
                    self.selected.clear();
                    self.selected.insert(element_id);
                    self.selection_gesture = None;
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %error,
                        "dropped image document insert failed"
                    );
                    last_error = Some(format!("Could not add {name}: {error}"));
                }
            }
        }

        if imported > 0 {
            self.status = if imported == 1 {
                String::from("Embedded image")
            } else {
                format!("Embedded {imported} images")
            };
        } else if let Some(error) = last_error {
            self.status = error;
        }
    }

    fn handle_clipboard_image(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        camera: &Camera,
        canvas: Rect,
    ) {
        if std::mem::take(&mut self.clipboard_paste_requested).is_none() {
            return;
        }
        if self.text_edit.is_some() || context.wants_keyboard_input() {
            return;
        }

        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let Some(clipboard) = self.clipboard.as_mut() else {
            tracing::debug!("native clipboard is unavailable for image paste");
            return;
        };
        let clipboard_image = match clipboard.get_image() {
            Ok(image) => image,
            Err(arboard::Error::ContentNotAvailable) => return,
            Err(error) => {
                tracing::debug!(error = %error, "clipboard does not contain a readable image");
                return;
            }
        };
        let width = clipboard_image.width;
        let height = clipboard_image.height;
        let bytes = clipboard_image.into_owned_bytes().into_owned();
        let image = match embedded_image_from_rgba(width, height, bytes) {
            Ok(image) => image,
            Err(error) => {
                self.status = format!("Could not paste image: {error}");
                return;
            }
        };
        let screen_position = context
            .input(|input| input.pointer.latest_pos())
            .filter(|position| canvas.contains(*position))
            .unwrap_or_else(|| canvas.center());
        let position = screen_to_world(*camera, screen_position);
        let size = image_display_size(image.width, image.height);
        let element_id = ElementId::new();
        let mut element = Element::image(element_id, Transform::new(position, size), image);
        element.style = self.draft_style;
        match Self::execute_editor_command(editor, EditorCommand::Create(element)) {
            Ok(_) => {
                self.selected.clear();
                self.selected.insert(element_id);
                self.selection_gesture = None;
                self.status = String::from("Embedded pasted image");
            }
            Err(error) => self.status = format!("Could not paste image: {error}"),
        }
    }

    fn show_text_editor(&mut self, context: &egui::Context, editor: &mut Editor, _camera: &Camera) {
        let mut commit = false;
        let mut cancel = false;
        let events = context.input(|input| input.events.clone());
        let pointer_pressed = context.input(|input| input.pointer.any_pressed());

        let Some(text_edit) = self.text_edit.as_mut() else {
            return;
        };

        for event in events {
            match event {
                egui::Event::Text(text) => text_edit.text.push_str(&text),
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    text_edit.text.pop();
                }
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.shift => text_edit.text.push('\n'),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => commit = true,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => cancel = true,
                _ => {}
            }
        }

        if text_edit.just_started {
            text_edit.just_started = false;
        } else if pointer_pressed {
            commit = true;
        }

        if cancel {
            let editing_element_id = self
                .text_edit
                .as_ref()
                .and_then(|text_edit| text_edit.element_id);
            self.text_edit = None;
            if let Some(element_id) = editing_element_id
                && editor.document().element(element_id).is_some()
            {
                self.selected.clear();
                self.selected.insert(element_id);
                self.sync_selected_style(&editor.document());
            }
            self.status = String::from("Text entry cancelled");
        } else if commit {
            let Some(text_edit) = self.text_edit.take() else {
                return;
            };
            if let Some(element_id) = text_edit.element_id {
                self.commit_existing_text(context, editor, element_id, &text_edit);
            } else {
                let element_id = ElementId::new();
                let size = measured_text_size(context, &text_edit.text, &text_edit.style);
                match text_create_command_with_size(
                    element_id,
                    text_edit.position,
                    &text_edit.text,
                    text_edit.style,
                    size,
                ) {
                    Some(command) => match Self::execute_editor_command(editor, command) {
                        Ok(_) => {
                            self.selected.clear();
                            self.selected.insert(element_id);
                            self.status = String::from("Text added");
                        }
                        Err(error) => self.status = format!("Could not add text: {error}"),
                    },
                    None => self.status = String::from("Empty text discarded"),
                }
            }
        }
    }

    fn commit_existing_text(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        element_id: ElementId,
        text_edit: &TextEditState,
    ) {
        let Some(element) = editor.document().element(element_id).cloned() else {
            self.status = String::from("Text object no longer exists");
            return;
        };

        if text_edit.text.trim().is_empty() {
            match editor.execute(EditorCommand::Delete(element_id)) {
                Ok(_) => {
                    self.selected.clear();
                    self.status = String::from("Empty text deleted");
                }
                Err(error) => self.status = format!("Could not delete text: {error}"),
            }
            return;
        }

        let mut failed = None;
        if element.style != text_edit.style
            && let Err(error) = editor.execute(EditorCommand::SetStyle(
                element_id,
                full_style_patch(text_edit.style),
            ))
        {
            failed = Some(error.to_string());
        }
        if failed.is_none()
            && element.text != text_edit.text
            && let Some(command) = text_update_command(element_id, &text_edit.text)
            && let Err(error) = editor.execute(command)
        {
            failed = Some(error.to_string());
        }

        if failed.is_none() {
            let next_transform = resized_text_transform_for_content(
                context,
                &element,
                &text_edit.text,
                text_edit.style,
            );
            if next_transform.position != element.transform.position
                && let Err(error) = editor.execute(EditorCommand::SetPosition(
                    element_id,
                    next_transform.position,
                ))
            {
                failed = Some(error.to_string());
            }
            if failed.is_none()
                && next_transform.size != element.transform.size
                && let Err(error) =
                    editor.execute(EditorCommand::SetSize(element_id, next_transform.size))
            {
                failed = Some(error.to_string());
            }
        }

        if let Some(error) = failed {
            self.status = format!("Could not update text: {error}");
        } else {
            self.selected.clear();
            self.selected.insert(element_id);
            self.status = String::from("Text updated");
        }
    }

    fn handle_canvas_input(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        document: &Document,
        editor: &mut Editor,
        tools: &mut ToolController,
        camera: &mut Camera,
    ) {
        if self.text_edit.is_some() {
            return;
        }

        if self.active_tool == Tool::Select {
            let shift = ui.input(|input| input.modifiers.shift);
            let press_origin = ui.input(|input| input.pointer.press_origin());
            self.handle_selection_input(response, document, editor, camera, shift, press_origin);
            return;
        }

        if self.active_tool == Tool::Text {
            if response.clicked()
                && let Some(position) = response.interact_pointer_pos()
            {
                let world_position = screen_to_world(*camera, position);
                let text_target =
                    hit_test(document, world_position, selection_tolerance(camera.zoom()))
                        .and_then(|element_id| document.element(element_id))
                        .filter(|element| element.kind == ElementKind::Text)
                        .cloned();
                if let Some(element) = text_target {
                    self.begin_text_edit(Some(&element));
                } else {
                    self.begin_text_edit_at(world_position);
                }
            }
            return;
        }

        if response.drag_started() {
            if self.active_tool == Tool::Pan {
                tools.cancel();
            } else if let Some(position) = response.interact_pointer_pos() {
                tools.pointer_down(ElementId::new(), screen_to_world(*camera, position));
            }
        }

        if response.dragged() {
            if self.active_tool == Tool::Pan {
                let delta = ui.input(|input| input.pointer.delta());
                if delta != Vec2::ZERO {
                    camera.pan_by_screen_delta(Point::new(delta.x, delta.y));
                }
            } else if self.active_tool != Tool::Select
                && let Some(position) = response.interact_pointer_pos()
            {
                let _ = tools.pointer_move(screen_to_world(*camera, position));
            }
        }

        if response.drag_stopped() {
            if self.active_tool == Tool::Pan {
                tools.cancel();
                self.status = String::from("Canvas moved");
            } else if self.active_tool != Tool::Select
                && let Some(position) = response.interact_pointer_pos()
                && let Some(ToolOutput::Command(command)) =
                    tools.pointer_up(screen_to_world(*camera, position))
            {
                let command = apply_draft_style(command, self.draft_style);
                match Self::execute_editor_command(editor, command) {
                    Ok(_) => {
                        self.status = format!("Created {}", tool_name(self.active_tool));
                    }
                    Err(error) => {
                        self.status = format!("Could not create element: {error}");
                    }
                }
            }
        }
    }

    fn begin_text_edit_at(&mut self, position: Point) {
        self.selected.clear();
        self.selection_gesture = None;
        self.text_edit = Some(TextEditState {
            element_id: None,
            position,
            rotation: 0.0,
            text: String::new(),
            style: self.draft_style,
            just_started: true,
        });
        self.status = String::from("Type text, then click outside to place it");
    }

    fn begin_text_edit(&mut self, element: Option<&Element>) {
        let Some(element) = element else {
            return;
        };
        self.selected.clear();
        self.selection_gesture = None;
        self.draft_style = element.style;
        self.text_edit = Some(TextEditState {
            element_id: Some(element.id),
            position: element.transform.position,
            rotation: element.transform.rotation,
            text: element.text.clone(),
            style: element.style,
            just_started: true,
        });
        self.status = String::from("Editing text — press Enter to save or Escape to cancel");
    }

    #[allow(clippy::too_many_lines)]
    fn handle_selection_input(
        &mut self,
        response: &egui::Response,
        document: &Document,
        editor: &mut Editor,
        camera: &Camera,
        shift: bool,
        press_origin: Option<Pos2>,
    ) {
        let text_edit_target = if response.double_clicked() {
            response.interact_pointer_pos().and_then(|position| {
                let world_position = screen_to_world(*camera, position);
                hit_test(document, world_position, selection_tolerance(camera.zoom()))
                    .and_then(|element_id| document.element(element_id))
                    .filter(|element| element.kind == ElementKind::Text)
                    .cloned()
            })
        } else {
            None
        };

        if response.clicked()
            && let Some(position) = response.interact_pointer_pos()
        {
            self.select_at(
                document,
                screen_to_world(*camera, position),
                camera.zoom(),
                shift,
            );
        }

        if let Some(element) = text_edit_target {
            self.begin_text_edit(Some(&element));
            return;
        }

        if response.drag_started()
            && let Some(position) = press_origin.or_else(|| response.interact_pointer_pos())
        {
            let world_position = screen_to_world(*camera, position);
            let tolerance = selection_tolerance(camera.zoom());
            let handle_tolerance = selection_handle_drag_tolerance(camera.zoom());
            let rotation_tolerance = rotation_handle_drag_tolerance(camera.zoom());
            let selection_padding = selection_padding_world(camera.zoom());

            if self.selected.len() == 1
                && let Some(selected_id) = self.selected.iter().next().copied()
                && let Some(element) = document.element(selected_id).cloned()
            {
                let bounds = element_bounds(&element);
                let center = Point::new(
                    bounds.min.x + bounds.size.width / 2.0,
                    bounds.min.y + bounds.size.height / 2.0,
                );
                if padded_selection_over_rotation_handle(
                    &element,
                    world_position,
                    rotation_tolerance,
                    selection_padding,
                ) {
                    let angle = pointer_angle(center, world_position);
                    self.selection_gesture = Some(SelectionGesture::Rotate {
                        element,
                        center,
                        start_angle: angle,
                        current_angle: angle,
                    });
                    self.status = String::from("Rotating object");
                    return;
                }
                if let Some(handle) = padded_selection_handle_at(
                    &element,
                    world_position,
                    handle_tolerance,
                    selection_padding,
                ) {
                    let actual_handle = selection_handle_position(&element, handle);
                    self.selection_gesture = Some(SelectionGesture::Resize {
                        bounds: selection_bounds(std::iter::once(&element))
                            .unwrap_or_else(|| element_bounds(&element)),
                        elements: vec![element],
                        handle,
                        pointer_current: world_position,
                        pointer_offset: Point::new(
                            actual_handle.x - world_position.x,
                            actual_handle.y - world_position.y,
                        ),
                    });
                    self.status = String::from("Resizing object");
                    return;
                }
            }

            if self.selected.len() > 1
                && let Some(bounds) =
                    selection_bounds(self.selected.iter().filter_map(|id| document.element(*id)))
                && let Some(handle) = selection_handle_at_bounds(
                    padded_selection_bounds(bounds, camera.zoom()),
                    world_position,
                    handle_tolerance,
                )
            {
                let actual_handle = crate::selection::handle_position(bounds, handle);
                self.selection_gesture = Some(SelectionGesture::Resize {
                    elements: self
                        .selected
                        .iter()
                        .filter_map(|id| document.element(*id).cloned())
                        .collect(),
                    bounds,
                    handle,
                    pointer_current: world_position,
                    pointer_offset: Point::new(
                        actual_handle.x - world_position.x,
                        actual_handle.y - world_position.y,
                    ),
                });
                self.status = String::from("Resizing selection");
                return;
            }

            let hit = hit_test(document, world_position, tolerance);
            let group_bounds_contains_pointer = self.selected.len() > 1
                && selection_bounds(self.selected.iter().filter_map(|id| document.element(*id)))
                    .is_some_and(|bounds| {
                        world_position.x >= bounds.min.x - tolerance
                            && world_position.x <= bounds.max().x + tolerance
                            && world_position.y >= bounds.min.y - tolerance
                            && world_position.y <= bounds.max().y + tolerance
                    });
            if let Some(element_id) = hit {
                if shift {
                    if self.selected.contains(&element_id) {
                        self.selected.remove(&element_id);
                    } else {
                        self.selected.insert(element_id);
                    }
                } else if !self.selected.contains(&element_id) {
                    self.selected.clear();
                    self.selected.insert(element_id);
                }
                if self.selected.contains(&element_id) {
                    self.selection_gesture = Some(SelectionGesture::Move {
                        elements: self
                            .selected
                            .iter()
                            .filter_map(|id| document.element(*id).cloned())
                            .collect(),
                        pointer_start: world_position,
                        pointer_current: world_position,
                    });
                    self.sync_selected_style(document);
                    self.status = if self.selected.len() == 1 {
                        String::from("Moving object")
                    } else {
                        format!("Moving {} objects", self.selected.len())
                    };
                } else {
                    self.selection_gesture = None;
                    self.status = String::from("Selection cleared");
                }
            } else if group_bounds_contains_pointer {
                self.selection_gesture = Some(SelectionGesture::Move {
                    elements: self
                        .selected
                        .iter()
                        .filter_map(|id| document.element(*id).cloned())
                        .collect(),
                    pointer_start: world_position,
                    pointer_current: world_position,
                });
                self.status = format!("Moving {} objects", self.selected.len());
            } else {
                self.selection_gesture = Some(SelectionGesture::Marquee {
                    start: world_position,
                    current: world_position,
                    additive: shift,
                });
                self.status = String::from("Selecting objects");
            }
        }

        if response.dragged()
            && let Some(position) = response.interact_pointer_pos()
            && let Some(gesture) = &mut self.selection_gesture
        {
            let world_position = screen_to_world(*camera, position);
            match gesture {
                SelectionGesture::Marquee { current, .. }
                | SelectionGesture::Move {
                    pointer_current: current,
                    ..
                } => *current = world_position,
                SelectionGesture::Resize {
                    pointer_current,
                    pointer_offset,
                    ..
                } => *pointer_current = resize_pointer_position(world_position, *pointer_offset),
                SelectionGesture::Rotate {
                    center,
                    current_angle,
                    ..
                } => *current_angle = pointer_angle(*center, world_position),
            }
        }

        if response.drag_stopped()
            && let Some(mut gesture) = self.selection_gesture.take()
            && let Some(position) = response.interact_pointer_pos()
        {
            let world_position = screen_to_world(*camera, position);
            match &mut gesture {
                SelectionGesture::Marquee { current, .. }
                | SelectionGesture::Move {
                    pointer_current: current,
                    ..
                } => *current = world_position,
                SelectionGesture::Resize {
                    pointer_current,
                    pointer_offset,
                    ..
                } => *pointer_current = resize_pointer_position(world_position, *pointer_offset),
                SelectionGesture::Rotate {
                    center,
                    current_angle,
                    ..
                } => *current_angle = pointer_angle(*center, world_position),
            }
            self.commit_selection_gesture(gesture, document, editor);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn commit_selection_gesture(
        &mut self,
        gesture: SelectionGesture,
        document: &Document,
        editor: &mut Editor,
    ) {
        match gesture {
            SelectionGesture::Marquee {
                start,
                current,
                additive,
            } => {
                let marquee = marquee_rect(start, current);
                let matches = document
                    .elements()
                    .filter(|element| marquee_intersects(element, marquee))
                    .map(|element| element.id)
                    .collect::<Vec<_>>();
                if additive {
                    for element_id in matches {
                        if !self.selected.remove(&element_id) {
                            self.selected.insert(element_id);
                        }
                    }
                } else {
                    self.selected = matches.into_iter().collect();
                }
                self.sync_selected_style(document);
                self.status = if self.selected.is_empty() {
                    String::from("No objects selected")
                } else {
                    format!("{} object(s) selected", self.selected.len())
                };
            }
            SelectionGesture::Move {
                elements,
                pointer_start,
                pointer_current,
            } => {
                let delta = Point::new(
                    pointer_current.x - pointer_start.x,
                    pointer_current.y - pointer_start.y,
                );
                let mut moved = 0_usize;
                for element in elements {
                    let preview = translated_element(&element, delta);
                    if preview.transform.position != element.transform.position
                        && editor
                            .execute(EditorCommand::SetPosition(
                                element.id,
                                preview.transform.position,
                            ))
                            .is_ok()
                    {
                        moved += 1;
                    }
                    if preview.points != element.points {
                        let _ =
                            editor.execute(EditorCommand::SetPoints(element.id, preview.points));
                    }
                }
                self.status = if moved == 0 {
                    String::from("Object selected")
                } else if moved == 1 {
                    String::from("Object moved")
                } else {
                    format!("Moved {moved} objects")
                };
            }
            SelectionGesture::Resize {
                elements,
                bounds,
                handle,
                pointer_current,
                ..
            } => {
                let count = elements.len();
                let mut resized = 0_usize;
                for element in elements {
                    let next = if count == 1 {
                        let mut next = element.clone();
                        next.transform =
                            crate::selection::resize_transform(&element, handle, pointer_current);
                        next
                    } else {
                        group_resized_element(&element, bounds, handle, pointer_current)
                    };
                    let mut changed = false;
                    if next.transform.position != element.transform.position {
                        let _ = editor.execute(EditorCommand::SetPosition(
                            element.id,
                            next.transform.position,
                        ));
                        changed = true;
                    }
                    if next.transform.size != element.transform.size {
                        let _ =
                            editor.execute(EditorCommand::SetSize(element.id, next.transform.size));
                        changed = true;
                    }
                    if next.points != element.points {
                        let _ = editor.execute(EditorCommand::SetPoints(element.id, next.points));
                        changed = true;
                    }
                    if changed {
                        resized += 1;
                    }
                }
                self.status = if resized == 0 {
                    String::from("Object selected")
                } else if count == 1 {
                    String::from("Object resized")
                } else {
                    format!("Resized {resized} objects")
                };
            }
            SelectionGesture::Rotate {
                element,
                start_angle,
                current_angle,
                ..
            } => {
                let next_rotation =
                    element.transform.rotation + angle_delta(start_angle, current_angle);
                if (next_rotation - element.transform.rotation).abs() > f32::EPSILON {
                    match editor.execute(EditorCommand::SetRotation(element.id, next_rotation)) {
                        Ok(_) => self.status = String::from("Object rotated"),
                        Err(error) => self.status = format!("Could not rotate object: {error}"),
                    }
                } else {
                    self.status = String::from("Object selected");
                }
            }
        }
    }

    fn select_at(&mut self, document: &Document, position: Point, zoom: f32, shift: bool) {
        let hit = hit_test(document, position, selection_tolerance(zoom));
        if shift {
            if let Some(element_id) = hit
                && !self.selected.remove(&element_id)
            {
                self.selected.insert(element_id);
            }
        } else {
            self.selected.clear();
            if let Some(element_id) = hit {
                self.selected.insert(element_id);
            }
        }
        self.selection_gesture = None;
        self.sync_selected_style(document);
        self.status = if self.selected.is_empty() {
            String::from("No object selected")
        } else if self.selected.len() == 1 {
            String::from("Object selected")
        } else {
            format!("{} objects selected", self.selected.len())
        };
    }

    fn sync_selected_style(&mut self, document: &Document) {
        if self.selected.len() == 1
            && let Some(selected) = self.selected.iter().next()
            && let Some(element) = document.element(*selected)
        {
            self.draft_style = element.style;
        }
    }

    fn active_palette(&self) -> [Color32; 7] {
        if self.dark_mode {
            self.dark_palette
        } else {
            self.light_palette
        }
    }

    fn sync_draft_palette(&mut self) {
        if self.selected.is_empty() {
            self.draft_style.stroke = to_core_color(self.active_palette()[0]);
        }
    }

    fn update_draft_palette_color(&mut self, previous: Color32, next: Color32) {
        if self.selected.is_empty() && to_color32(self.draft_style.stroke) == previous {
            self.draft_style.stroke = to_core_color(next);
        }
    }

    fn canvas_cursor(
        &self,
        document: &Document,
        response: &egui::Response,
        camera: Camera,
        dragging: bool,
    ) -> CursorIcon {
        if self.active_tool != Tool::Select {
            return canvas_cursor(self.active_tool, dragging);
        }
        if let Some(gesture) = &self.selection_gesture {
            return match gesture {
                SelectionGesture::Move { .. } => CursorIcon::Grabbing,
                SelectionGesture::Marquee { .. } | SelectionGesture::Rotate { .. } => {
                    CursorIcon::Crosshair
                }
                SelectionGesture::Resize { handle, .. } => resize_cursor(*handle),
            };
        }
        if self.selected.len() == 1
            && let Some(selected) = self.selected.iter().next()
            && let Some(element) = document.element(*selected)
            && let Some(pointer) = response.hover_pos().or(response.interact_pointer_pos())
        {
            let world = screen_to_world(camera, pointer);
            let tolerance = selection_tolerance(camera.zoom());
            let handle_tolerance = selection_handle_cursor_tolerance(camera.zoom());
            let rotation_tolerance = rotation_handle_cursor_tolerance(camera.zoom());
            let selection_padding = selection_padding_world(camera.zoom());
            if padded_selection_over_rotation_handle(
                element,
                world,
                rotation_tolerance,
                selection_padding,
            ) {
                return CursorIcon::Crosshair;
            }
            if let Some(handle) =
                padded_selection_handle_at(element, world, handle_tolerance, selection_padding)
            {
                return match handle {
                    SelectionHandle::TopLeft | SelectionHandle::BottomRight => {
                        CursorIcon::ResizeNwSe
                    }
                    SelectionHandle::TopRight | SelectionHandle::BottomLeft => {
                        CursorIcon::ResizeNeSw
                    }
                    SelectionHandle::Top | SelectionHandle::Bottom => CursorIcon::ResizeVertical,
                    SelectionHandle::Left | SelectionHandle::Right => CursorIcon::ResizeHorizontal,
                };
            }
            if hit_test(document, world, tolerance) == Some(*selected) {
                return if dragging {
                    CursorIcon::Grabbing
                } else {
                    CursorIcon::Move
                };
            }
        }
        if self.selected.len() > 1
            && let Some(pointer) = response.hover_pos().or(response.interact_pointer_pos())
            && let Some(bounds) =
                selection_bounds(self.selected.iter().filter_map(|id| document.element(*id)))
        {
            let world = screen_to_world(camera, pointer);
            if let Some(handle) = selection_handle_at_bounds(
                padded_selection_bounds(bounds, camera.zoom()),
                world,
                selection_handle_cursor_tolerance(camera.zoom()),
            ) {
                return resize_cursor(handle);
            }
            if hit_test(document, world, selection_tolerance(camera.zoom()))
                .is_some_and(|id| self.selected.contains(&id))
                || (world.x >= bounds.min.x
                    && world.x <= bounds.max().x
                    && world.y >= bounds.min.y
                    && world.y <= bounds.max().y)
            {
                return CursorIcon::Move;
            }
        }
        canvas_cursor(self.active_tool, dragging)
    }

    fn handle_keybind_input(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        tools: &mut ToolController,
    ) {
        if self.text_edit.is_some() {
            return;
        }

        if self.capturing_keybind.is_some() {
            let binding = context.input(|input| {
                input.events.iter().rev().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some(KeyBinding {
                        key: *key,
                        modifiers: *modifiers,
                    }),
                    _ => None,
                })
            });
            if let Some(binding) = binding {
                if binding.key == Key::Escape {
                    self.capturing_keybind = None;
                    self.status = String::from("Shortcut change cancelled");
                    return;
                }
                let Some(action) = self.capturing_keybind else {
                    return;
                };
                if let Some(existing) = self.keybinds.action_for(binding)
                    && existing != action
                {
                    self.status = format!(
                        "{} is already assigned to {}",
                        key_binding_label(binding),
                        keybind_action_name(existing),
                    );
                    return;
                }
                self.set_keybind(action, binding);
                self.capturing_keybind = None;
                self.status = format!("{} shortcut changed", keybind_action_name(action));
            }
            return;
        }

        if self.settings_open || context.wants_keyboard_input() {
            return;
        }

        let keybinds = self.keybinds;
        let action = context.input(|input| {
            KeybindAction::ALL.into_iter().find(|action| {
                let binding = keybinds.binding(*action);
                input.key_pressed(binding.key) && input.modifiers.matches_exact(binding.modifiers)
            })
        });

        match action {
            Some(KeybindAction::SelectAll) => {
                let document = editor.document();
                self.choose_tool(Tool::Select, tools);
                self.selected = document.elements().map(|element| element.id).collect();
                self.sync_selected_style(&document);
                self.status = format!("Selected {} objects", self.selected.len());
            }
            Some(KeybindAction::Delete) => self.delete_selected(editor),
            Some(KeybindAction::Undo) => {
                self.status = match editor.undo() {
                    Ok(_) => String::from("Undid the last operation"),
                    Err(error) => error.to_string(),
                };
            }
            Some(KeybindAction::Redo) => {
                self.status = match editor.redo() {
                    Ok(_) => String::from("Redid the last operation"),
                    Err(error) => error.to_string(),
                };
            }
            Some(KeybindAction::NewDocument) => self.new_document(editor),
            Some(KeybindAction::Save) => {
                self.status = String::from("Local save will use the journal");
            }
            Some(KeybindAction::Settings) => {
                self.settings_open = !self.settings_open;
            }
            Some(action) => {
                if let Some(tool) = action.tool() {
                    self.choose_tool(tool, tools);
                }
            }
            None => {}
        }
    }

    fn set_keybind(&mut self, action: KeybindAction, binding: KeyBinding) {
        self.keybinds.set_binding(action, binding);
    }

    fn delete_selected(&mut self, editor: &mut Editor) {
        let selected = self.selected.iter().copied().collect::<Vec<_>>();
        let mut deleted = 0_usize;
        for element_id in selected {
            if editor.execute(EditorCommand::Delete(element_id)).is_ok() {
                deleted += 1;
            }
        }
        self.selected.clear();
        self.selection_gesture = None;
        self.status = format!("Deleted {deleted} objects");
    }

    fn new_document(&mut self, editor: &mut Editor) {
        let client_id = editor.client_id();
        *editor = Editor::new(client_id);
        self.selected.clear();
        self.selection_gesture = None;
        self.status = String::from("New whiteboard created");
    }

    fn choose_tool(&mut self, tool: Tool, tools: &mut ToolController) {
        self.active_tool = tool;
        self.selected.clear();
        self.selection_gesture = None;
        self.text_edit = None;
        self.color_picker = None;
        self.color_picker_detail = None;
        tools.set_tool(tool);
        self.status = format!("{} tool selected", tool_name(tool));
    }

    fn show_file_actions(&mut self, context: &egui::Context, editor: &mut Editor) {
        let mut new_requested = false;
        let mut save_requested = false;
        let mut settings_requested = false;
        egui::Area::new(Id::new("sketchi.file_actions"))
            .fixed_pos(egui::pos2(16.0, 16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                toolbar_frame(self.dark_mode).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icon_button(
                            ui,
                            Icon::Settings,
                            "Settings",
                            self.settings_open,
                            self.dark_mode,
                        )
                        .clicked()
                        {
                            settings_requested = true;
                        }
                        if icon_button(ui, Icon::Save, "Save locally", false, self.dark_mode)
                            .clicked()
                        {
                            save_requested = true;
                        }
                        if icon_button(ui, Icon::New, "New whiteboard", false, self.dark_mode)
                            .clicked()
                        {
                            new_requested = true;
                        }
                    });
                });
            });

        if new_requested {
            self.new_document(editor);
        }
        if save_requested {
            self.status = String::from("Local save will use the journal");
        }
        if settings_requested {
            self.settings_open = !self.settings_open;
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn show_settings_window(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        tools: &mut ToolController,
    ) {
        if !self.settings_open {
            return;
        }

        if self.appearance == AppearanceMode::System {
            self.dark_mode = self.system_dark_mode.unwrap_or(false);
        }
        let mut selected_page = self.settings_page;
        let pages = [
            (SettingsPage::General, "General", Icon::Sun),
            (SettingsPage::Keybinds, "Keybinds", Icon::Settings),
            (SettingsPage::Input, "Input", Icon::Brush),
            (SettingsPage::About, "About", Icon::Save),
        ];
        context.set_visuals(if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        self.handle_keybind_input(context, editor, tools);
        egui::CentralPanel::default()
            .frame(settings_window_frame(self.dark_mode))
            .show(context, |ui| {
                let settings_body_width = ui.available_width();
                let settings_body_height = ui.available_height();
                ui.allocate_ui_with_layout(
                    Vec2::new(settings_body_width, settings_body_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.allocate_ui_with_layout(
                        Vec2::new(160.0, settings_body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                        ui.add_space(4.0);
                        for (page, label, icon) in pages {
                            if settings_nav_item(
                                ui,
                                label,
                                icon,
                                selected_page == page,
                                self.dark_mode,
                            )
                            .clicked()
                            {
                                selected_page = page;
                            }
                        }
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("Sketchi settings")
                                .size(11.0)
                                .color(muted_color(self.dark_mode)),
                        );
                    });
                    ui.separator();
                    ui.add_space(22.0);
                    let content_width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        Vec2::new(content_width, settings_body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("sketchi.settings.content", selected_page))
                            .max_height(settings_body_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                        ui.set_width(content_width);
                        match selected_page {
                        SettingsPage::General => {
                            ui.heading(
                                egui::RichText::new("General")
                                    .color(text_color(self.dark_mode)),
                            );
                            ui.label(
                                egui::RichText::new("Manage the canvas appearance and session behavior.")
                                    .color(muted_color(self.dark_mode)),
                            );
                            ui.add_space(16.0);
                            settings_group_frame(self.dark_mode).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("Session")
                                            .strong()
                                            .color(text_color(self.dark_mode)),
                                    );
                                });
                                ui.add_space(8.0);
                                ui.checkbox(
                                    &mut self.restore_session,
                                    "Restore previous session on start",
                                );
                                ui.add_space(6.0);
                                settings_form_row(ui, "Time interval for automatic saving", 160.0, |ui| {
                                    dropdown_field(
                                        ui,
                                        "sketchi.autosave_interval",
                                        self.autosave_interval.label(),
                                        |ui| {
                                            for interval in AutosaveInterval::ALL {
                                                ui.selectable_value(
                                                    &mut self.autosave_interval,
                                                    interval,
                                                    interval.label(),
                                                );
                                            }
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                settings_form_row(ui, "Directory for automatic saving", 160.0, |ui| {
                                    text_field(
                                        ui,
                                        &mut self.autosave_directory,
                                        "",
                                    );
                                });
                            });
                            ui.add_space(12.0);
                            settings_group_frame(self.dark_mode).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("Canvas")
                                            .strong()
                                            .color(text_color(self.dark_mode)),
                                    );
                                });
                                ui.add_space(8.0);
                                settings_form_row(ui, "Theme", 160.0, |ui| {
                                    let theme_label = match self.appearance {
                                        AppearanceMode::System => "Automatic",
                                        AppearanceMode::Light => "Light",
                                        AppearanceMode::Dark => "Dark",
                                    };
                                    dropdown_field(
                                        ui,
                                        "sketchi.canvas_theme",
                                        theme_label,
                                        |ui| {
                                            for (mode, label) in [
                                                (AppearanceMode::System, "Automatic"),
                                                (AppearanceMode::Light, "Light"),
                                                (AppearanceMode::Dark, "Dark"),
                                            ] {
                                                if ui
                                                    .selectable_value(
                                                        &mut self.appearance,
                                                        mode,
                                                        label,
                                                    )
                                                    .clicked()
                                                {
                                                    self.dark_mode = if mode
                                                        == AppearanceMode::System
                                                    {
                                                        self.system_dark_mode.unwrap_or(false)
                                                    } else {
                                                        mode == AppearanceMode::Dark
                                                    };
                                                    self.sync_draft_palette();
                                                }
                                            }
                                        },
                                    );
                                });
                                ui.add_space(8.0);
                                settings_form_row(ui, "Light background color", 160.0, |ui| {
                                    let light_color = color_swatch(
                                        ui,
                                        Some(self.light_canvas_color),
                                        false,
                                        self.dark_mode,
                                    );
                                    if light_color.clicked() {
                                        self.light_canvas_color = next_settings_color(
                                            self.light_canvas_color,
                                        );
                                    }
                                });
                                settings_form_row(ui, "Dark background color", 160.0, |ui| {
                                    let dark_color = color_swatch(
                                        ui,
                                        Some(self.dark_canvas_color),
                                        false,
                                        self.dark_mode,
                                    );
                                    if dark_color.clicked() {
                                        self.dark_canvas_color =
                                            next_settings_color(self.dark_canvas_color);
                                    }
                                });
                            });
                            ui.add_space(12.0);
                            settings_group_frame(self.dark_mode).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("Color Palette")
                                            .strong()
                                            .color(text_color(self.dark_mode)),
                                    );
                                });
                                ui.add_space(8.0);
                                settings_form_row(ui, "Light Mode", 160.0, |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for index in 0..5 {
                                        let Some(color) = self.light_palette.get(index).copied()
                                        else {
                                            continue;
                                        };
                                        if color_swatch(ui, Some(color), false, self.dark_mode)
                                            .clicked()
                                        {
                                            let next = next_settings_color(color);
                                            if let Some(slot) = self.light_palette.get_mut(index) {
                                                *slot = next;
                                            }
                                            self.update_draft_palette_color(color, next);
                                        }
                                    }
                                });
                                settings_form_row(ui, "Dark Mode", 160.0, |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for index in 0..5 {
                                        let Some(color) = self.dark_palette.get(index).copied()
                                        else {
                                            continue;
                                        };
                                        if color_swatch(ui, Some(color), false, self.dark_mode)
                                            .clicked()
                                        {
                                            let next = next_settings_color(color);
                                            if let Some(slot) = self.dark_palette.get_mut(index) {
                                                *slot = next;
                                            }
                                            self.update_draft_palette_color(color, next);
                                        }
                                    }
                                });
                            });
                        }
                        SettingsPage::Keybinds => {
                            ui.heading(
                                egui::RichText::new("Keybinds")
                                    .color(text_color(self.dark_mode)),
                            );
                            ui.label(
                                egui::RichText::new("Click a shortcut, then press the key combination you want to use.")
                                    .color(muted_color(self.dark_mode)),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Mouse wheel zooms the canvas. Hold the middle mouse button to pan.",
                                )
                                .color(muted_color(self.dark_mode)),
                            );
                            ui.add_space(20.0);
                            for action in KeybindAction::ALL {
                                let label = action.label();
                                let binding = self.keybinds.binding(action);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let (label_rect, _) = ui.allocate_exact_size(
                                        Vec2::new(235.0, STANDARD_CONTROL_SIZE.y),
                                        Sense::hover(),
                                    );
                                    ui.painter().text(
                                        label_rect.left_top(),
                                        Align2::LEFT_TOP,
                                        label,
                                        FontId::proportional(13.0),
                                        text_color(self.dark_mode),
                                    );
                                    ui.painter().text(
                                        label_rect.left_top() + Vec2::new(0.0, 15.0),
                                        Align2::LEFT_TOP,
                                        key_binding_label(binding),
                                        FontId::proportional(11.0),
                                        muted_color(self.dark_mode),
                                    );
                                    let capturing = self.capturing_keybind == Some(action);
                                    let shortcut_label = if capturing {
                                        String::from("Press a key…")
                                    } else {
                                        String::from("Change shortcut")
                                    };
                                    let button = button(
                                        ui,
                                        egui::RichText::new(shortcut_label).color(if capturing {
                                            Color32::WHITE
                                        } else {
                                            text_color(self.dark_mode)
                                        }),
                                        STANDARD_CONTROL_SIZE,
                                        if capturing {
                                            ACCENT
                                        } else if self.dark_mode {
                                            Color32::from_rgb(52, 54, 62)
                                        } else {
                                            Color32::from_rgb(245, 246, 249)
                                        },
                                    );
                                    let button =
                                        button.on_hover_text("Click to change this shortcut");
                                    if button.clicked() {
                                        self.capturing_keybind = Some(action);
                                    }
                                });
                                ui.add_space(8.0);
                            }
                            if button(
                                ui,
                                egui::RichText::new("Restore defaults").color(ACCENT),
                                Vec2::new(104.0, STANDARD_CONTROL_SIZE.y),
                                if self.dark_mode {
                                    Color32::from_rgb(52, 54, 62)
                                } else {
                                    Color32::from_rgb(245, 246, 249)
                                },
                            )
                            .clicked()
                            {
                                self.keybinds = Keybinds::default();
                                self.capturing_keybind = None;
                            }
                        }
                        SettingsPage::Input => {
                            ui.heading(
                                egui::RichText::new("Input")
                                    .color(text_color(self.dark_mode)),
                            );
                            ui.label(
                                egui::RichText::new("Tune how freehand input feels while drawing.")
                                    .color(muted_color(self.dark_mode)),
                            );
                            ui.add_space(16.0);
                            settings_group_frame(self.dark_mode).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        egui::RichText::new("Freehand settings")
                                            .strong()
                                            .color(text_color(self.dark_mode)),
                                    );
                                });
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        Vec2::new(125.0, 18.0),
                                        egui::Label::new("Stabilization"),
                                    );
                                    let stabilization_tooltip =
                                        format!("{:.0}%", self.stabilization * 100.0);
                                    let track = if self.dark_mode {
                                        DARK_BORDER
                                    } else {
                                        LIGHT_BORDER
                                    };
                                    range_slider(
                                        ui,
                                        &mut self.stabilization,
                                        0.0..=1.0,
                                        Vec2::new(180.0, 18.0),
                                        track,
                                        ACCENT,
                                        Some(stabilization_tooltip),
                                    );
                                    numeric_field(
                                        ui,
                                        &mut self.stabilization,
                                        0.0..=1.0,
                                        Vec2::new(58.0, 26.0),
                                        "",
                                    );
                                });
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        Vec2::new(125.0, 18.0),
                                        egui::Label::new("Pressure sensitivity"),
                                    );
                                    let pressure_tooltip =
                                        format!("{:.0}%", self.pressure_sensitivity * 100.0);
                                    let track = if self.dark_mode {
                                        DARK_BORDER
                                    } else {
                                        LIGHT_BORDER
                                    };
                                    range_slider(
                                        ui,
                                        &mut self.pressure_sensitivity,
                                        0.0..=1.0,
                                        Vec2::new(180.0, 18.0),
                                        track,
                                        ACCENT,
                                        Some(pressure_tooltip),
                                    );
                                    numeric_field(
                                        ui,
                                        &mut self.pressure_sensitivity,
                                        0.0..=1.0,
                                        Vec2::new(58.0, 26.0),
                                        "",
                                    );
                                });
                            });
                        }
                        SettingsPage::About => {
                            ui.heading(
                                egui::RichText::new("About Sketchi")
                                    .color(text_color(self.dark_mode)),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new("A collaborative Rust whiteboard for fast visual thinking.")
                                    .color(text_color(self.dark_mode)),
                            );
                            ui.add_space(20.0);
                            ui.label(
                                egui::RichText::new("Draw, write, and work together on an infinite canvas.")
                                    .color(muted_color(self.dark_mode)),
                            );
                        }
                        }
                            });
                    });
                });
            });

        self.settings_page = selected_page;
    }

    fn handle_canvas_navigation(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        tools: &mut ToolController,
        camera: &mut Camera,
        canvas: Rect,
    ) -> bool {
        let (middle_down, middle_pressed, middle_released, pointer_delta, scroll_delta, pointer) =
            ui.input(|input| {
                (
                    input.pointer.middle_down(),
                    input.pointer.button_pressed(PointerButton::Middle),
                    input.pointer.button_released(PointerButton::Middle),
                    input.pointer.delta(),
                    input.raw_scroll_delta,
                    input.pointer.hover_pos(),
                )
            });
        let pointer_over_canvas = pointer.is_some_and(|position| canvas.contains(position));

        if (middle_down || middle_pressed || middle_released) && pointer_over_canvas {
            tools.cancel();
            self.selection_gesture = None;
            if middle_down && pointer_delta != Vec2::ZERO {
                camera.pan_by_screen_delta(Point::new(pointer_delta.x, pointer_delta.y));
                self.status = String::from("Canvas panning");
            } else if middle_released {
                self.status = String::from("Canvas moved");
            }
            return true;
        }

        if response.hovered() && scroll_delta.y.abs() > f32::EPSILON {
            let cursor = pointer.unwrap_or_else(|| canvas.center());
            camera.zoom_by(
                Point::new(cursor.x, cursor.y),
                zoom_delta_for_scroll(scroll_delta.y),
            );
            return true;
        }

        false
    }

    fn show_tool_palette(&mut self, context: &egui::Context, tools: &mut ToolController) {
        let tool_buttons = [
            (Tool::Select, Icon::Select, "Select"),
            (Tool::Text, Icon::InputCursorMove, "Text"),
            (Tool::Freehand, Icon::Freehand, "Freehand"),
            (Tool::Rectangle, Icon::Rectangle, "Rectangle"),
            (Tool::Diamond, Icon::PokerDiamonds, "Diamond"),
            (Tool::Triangle, Icon::Triangle, "Triangle"),
            (Tool::Ellipse, Icon::Ellipse, "Ellipse"),
            (Tool::Line, Icon::Line, "Line"),
            (Tool::Arrow, Icon::ArrowLeftDownLong, "Arrow"),
            (Tool::Pan, Icon::Pan, "Pan"),
        ];
        let mut selected_tool = None;
        egui::Area::new(Id::new("sketchi.tool_palette"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                toolbar_frame(self.dark_mode).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (tool, icon, name) in tool_buttons {
                            let shortcut = KeybindAction::ALL
                                .into_iter()
                                .find(|action| action.tool() == Some(tool))
                                .map(|action| key_binding_label(self.keybinds.binding(action)));
                            let tooltip = shortcut.map_or_else(
                                || String::from(name),
                                |shortcut| format!("{name} · {shortcut}"),
                            );
                            if icon_button(
                                ui,
                                icon,
                                &tooltip,
                                self.active_tool == tool,
                                self.dark_mode,
                            )
                            .clicked()
                            {
                                selected_tool = Some(tool);
                            }
                        }
                    });
                });
            });
        if let Some(tool) = selected_tool {
            self.choose_tool(tool, tools);
        }
    }

    fn show_history(&mut self, context: &egui::Context, editor: &mut Editor) {
        let mut undo_requested = false;
        let mut redo_requested = false;
        egui::Area::new(Id::new("sketchi.history"))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(170.0, -16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                toolbar_frame(self.dark_mode).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icon_button(ui, Icon::Undo, "Undo", false, self.dark_mode).clicked() {
                            undo_requested = true;
                        }
                        if icon_button(ui, Icon::Redo, "Redo", false, self.dark_mode).clicked() {
                            redo_requested = true;
                        }
                    });
                });
            });

        if undo_requested {
            self.status = match editor.undo() {
                Ok(_) => String::from("Undid the last operation"),
                Err(error) => error.to_string(),
            };
        }
        if redo_requested {
            self.status = match editor.redo() {
                Ok(_) => String::from("Redid the last operation"),
                Err(error) => error.to_string(),
            };
        }
    }

    #[allow(clippy::too_many_lines)]
    fn show_properties(&mut self, context: &egui::Context, editor: &mut Editor) {
        if self.active_tool == Tool::Select && self.selected.is_empty() {
            return;
        }

        let document = editor.document();
        let style = self.property_style(editor);
        let palette = self.active_palette();
        let mut patch = StylePatch::default();
        let mut changed = false;
        let mut color_target = None;
        let mut layer_action = None;
        let mut align_action = None;
        let mut element_action = None;
        let text_selection = self.active_tool == Tool::Text
            || self.text_edit.is_some()
            || (self.selected.len() == 1
                && self
                    .selected
                    .iter()
                    .next()
                    .and_then(|id| document.element(*id))
                    .is_some_and(|element| element.kind == ElementKind::Text));
        let image_selection = self.selected.len() == 1
            && self
                .selected
                .iter()
                .next()
                .and_then(|id| document.element(*id))
                .is_some_and(|element| element.kind == ElementKind::Image);
        let panel_height = (context.content_rect().height() - 104.0).max(280.0);
        egui::Area::new(Id::new("sketchi.properties"))
            .fixed_pos(egui::pos2(16.0, 76.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                properties_frame(self.dark_mode).show(ui, |ui| {
                    // Keep enough room for the full palette rows before the
                    // properties scrollbar starts.
                    ui.set_width(260.0);
                    egui::ScrollArea::vertical()
                        .max_height(panel_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if !text_selection && !image_selection {
                                ui.label(
                                    egui::RichText::new(if self.selected.len() > 1 {
                                        "Selected objects"
                                    } else if !self.selected.is_empty() {
                                        "Selected object"
                                    } else {
                                        "New object"
                                    })
                                    .strong()
                                    .size(14.0)
                                    .color(text_color(self.dark_mode)),
                                );
                                ui.add_space(10.0);
                            }

                            if text_selection {
                                section_label(ui, "Stroke", self.dark_mode);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    for color in palette.into_iter().take(5) {
                                        if color_swatch(
                                            ui,
                                            Some(color),
                                            to_color32(style.stroke) == color,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.stroke = Some(to_core_color(color));
                                            changed = true;
                                        }
                                    }
                                    let custom = to_color32(style.stroke);
                                    ui.separator();
                                    if color_swatch(
                                        ui,
                                        Some(custom),
                                        self.color_picker == Some(ColorPickerTarget::Stroke),
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        color_target = Some(ColorPickerTarget::Stroke);
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Font family", self.dark_mode);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    for (family, icon, label) in [
                                        (TextFontFamily::Handwritten, Icon::Freehand, "Hand-drawn"),
                                        (TextFontFamily::Sans, Icon::FontSans, "Normal"),
                                        (TextFontFamily::Monospace, Icon::Code, "Code"),
                                    ] {
                                        if text_property_icon_choice(
                                            ui,
                                            icon,
                                            style.font_family == family,
                                            self.dark_mode,
                                        )
                                        .on_hover_text(label)
                                        .clicked()
                                        {
                                            patch.font_family = Some(family);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Font size", self.dark_mode);
                                let custom_size_open =
                                    self.custom_font_size == CustomFontSizeState::Open;
                                let has_preset_font_size = [12.0_f32, 16.0, 24.0, 32.0]
                                    .into_iter()
                                    .any(|size| (style.font_size - size).abs() < f32::EPSILON);
                                let custom_size_selected = custom_font_size_selected(
                                    self.custom_font_size,
                                    has_preset_font_size,
                                );
                                ui.horizontal(|ui| {
                                    for (size, label) in
                                        [(12.0_f32, "S"), (16.0, "M"), (24.0, "L"), (32.0, "XL")]
                                    {
                                        if text_size_choice(
                                            ui,
                                            size,
                                            label,
                                            preset_font_size_selected(
                                                self.custom_font_size,
                                                style.font_size,
                                                size,
                                            ),
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.font_size = Some(size);
                                            self.custom_font_size = CustomFontSizeState::Closed;
                                            changed = true;
                                        }
                                    }
                                    ui.separator();
                                    if custom_text_size_choice(
                                        ui,
                                        custom_size_selected,
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        self.custom_font_size = match self.custom_font_size {
                                            CustomFontSizeState::Closed => {
                                                CustomFontSizeState::Open
                                            }
                                            CustomFontSizeState::Open => {
                                                CustomFontSizeState::Closed
                                            }
                                        };
                                        if self.custom_font_size == CustomFontSizeState::Open {
                                            patch.font_size = Some(style.font_size);
                                            changed = true;
                                        }
                                    }
                                });
                                if custom_size_open {
                                    ui.add_space(8.0);
                                    let mut custom_size = style.font_size;
                                    if numeric_field_with_decimals(
                                        ui,
                                        &mut custom_size,
                                        1.0..=256.0,
                                        STANDARD_CONTROL_SIZE,
                                        0,
                                        "",
                                    )
                                    .changed()
                                    {
                                        patch.font_size = Some(custom_size);
                                        changed = true;
                                    }
                                }

                                ui.add_space(10.0);
                                section_label(ui, "Text align", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (align, icon, label) in [
                                        (TextAlign::Left, Icon::TextAlignLeft, "Left"),
                                        (TextAlign::Center, Icon::TextAlignCenter, "Center"),
                                        (TextAlign::Right, Icon::TextAlignRight, "Right"),
                                    ] {
                                        if text_property_icon_choice(
                                            ui,
                                            icon,
                                            style.text_align == align,
                                            self.dark_mode,
                                        )
                                        .on_hover_text(label)
                                        .clicked()
                                        {
                                            patch.text_align = Some(align);
                                            changed = true;
                                        }
                                    }
                                });
                            } else if image_selection {
                                section_label(ui, "Stroke", self.dark_mode);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    for color in palette {
                                        if color_swatch(
                                            ui,
                                            Some(color),
                                            to_color32(style.stroke) == color,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.stroke = Some(to_core_color(color));
                                            changed = true;
                                        }
                                    }
                                    let custom = to_color32(style.stroke);
                                    ui.separator();
                                    if color_swatch(
                                        ui,
                                        Some(custom),
                                        self.color_picker == Some(ColorPickerTarget::Stroke),
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        color_target = Some(ColorPickerTarget::Stroke);
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Stroke width", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for width in [1.0_f32, 2.0, 4.0] {
                                        if width_choice(
                                            ui,
                                            width,
                                            (style.stroke_width - width).abs() < f32::EPSILON,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.stroke_width = Some(width);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Stroke style", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (stroke_style, label) in [
                                        (StrokeStyle::Solid, "Solid"),
                                        (StrokeStyle::Dashed, "Dashed"),
                                        (StrokeStyle::Dotted, "Dotted"),
                                    ] {
                                        if stroke_style_choice(
                                            ui,
                                            stroke_style,
                                            style.stroke_style == stroke_style,
                                            self.dark_mode,
                                        )
                                        .on_hover_text(label)
                                        .clicked()
                                        {
                                            patch.stroke_style = Some(stroke_style);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Sloppiness", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (sloppiness, icon, label) in [
                                        (Sloppiness::Architect, Icon::PenNib, "Architect"),
                                        (Sloppiness::Artist, Icon::QuillPen, "Artist"),
                                        (Sloppiness::Cartoonist, Icon::Brush, "Cartoonist"),
                                    ] {
                                        if property_choice(
                                            ui,
                                            icon,
                                            label,
                                            style.sloppiness == sloppiness,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.sloppiness = Some(sloppiness);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Edges", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (edges, icon, label) in [
                                        (EdgeStyle::Sharp, Icon::Rectangle, "Sharp"),
                                        (EdgeStyle::Rounded, Icon::Rounded, "Rounded"),
                                    ] {
                                        if property_choice(
                                            ui,
                                            icon,
                                            label,
                                            style.edges == edges,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.edges = Some(edges);
                                            changed = true;
                                        }
                                    }
                                });
                            } else {
                                section_label(ui, "Stroke", self.dark_mode);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    for color in palette {
                                        if color_swatch(
                                            ui,
                                            Some(color),
                                            to_color32(style.stroke) == color,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.stroke = Some(to_core_color(color));
                                            changed = true;
                                        }
                                    }
                                    let custom = to_color32(style.stroke);
                                    ui.separator();
                                    let response = color_swatch(
                                        ui,
                                        Some(custom),
                                        self.color_picker == Some(ColorPickerTarget::Stroke),
                                        self.dark_mode,
                                    );
                                    if response.clicked() {
                                        color_target = Some(ColorPickerTarget::Stroke);
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Background", self.dark_mode);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if color_swatch(ui, None, style.fill.is_none(), self.dark_mode)
                                        .on_hover_text("Transparent")
                                        .clicked()
                                    {
                                        patch.fill = Some(None);
                                        changed = true;
                                    }
                                    for color in FILL_COLORS {
                                        if color_swatch(
                                            ui,
                                            Some(color),
                                            style
                                                .fill
                                                .is_some_and(|fill| to_color32(fill) == color),
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.fill = Some(Some(to_core_color(color)));
                                            changed = true;
                                        }
                                    }
                                    let custom = style
                                        .fill
                                        .map_or(Color32::from_rgb(221, 214, 254), to_color32);
                                    ui.separator();
                                    let response = color_swatch(
                                        ui,
                                        Some(custom),
                                        self.color_picker == Some(ColorPickerTarget::Fill),
                                        self.dark_mode,
                                    );
                                    if response.clicked() {
                                        color_target = Some(ColorPickerTarget::Fill);
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Fill", self.dark_mode);
                                ui.horizontal(|ui| {
                                    if property_choice(
                                        ui,
                                        Icon::FillNone,
                                        "None",
                                        style.fill.is_none(),
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        patch = fill_choice_patch(style.fill, false);
                                        changed = true;
                                    }
                                    if property_choice(
                                        ui,
                                        Icon::FillSolid,
                                        "Solid",
                                        style.fill.is_some(),
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        patch = fill_choice_patch(style.fill, true);
                                        changed = true;
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Stroke width", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for width in [1.0_f32, 2.0, 4.0] {
                                        if width_choice(
                                            ui,
                                            width,
                                            (style.stroke_width - width).abs() < f32::EPSILON,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.stroke_width = Some(width);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Stroke style", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (stroke_style, label) in [
                                        (StrokeStyle::Solid, "Solid"),
                                        (StrokeStyle::Dashed, "Dashed"),
                                        (StrokeStyle::Dotted, "Dotted"),
                                    ] {
                                        if stroke_style_choice(
                                            ui,
                                            stroke_style,
                                            style.stroke_style == stroke_style,
                                            self.dark_mode,
                                        )
                                        .on_hover_text(label)
                                        .clicked()
                                        {
                                            patch.stroke_style = Some(stroke_style);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Sloppiness", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (sloppiness, icon, label) in [
                                        (Sloppiness::Architect, Icon::PenNib, "Architect"),
                                        (Sloppiness::Artist, Icon::QuillPen, "Artist"),
                                        (Sloppiness::Cartoonist, Icon::Brush, "Cartoonist"),
                                    ] {
                                        if property_choice(
                                            ui,
                                            icon,
                                            label,
                                            style.sloppiness == sloppiness,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.sloppiness = Some(sloppiness);
                                            changed = true;
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Edges", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (edges, icon, label) in [
                                        (EdgeStyle::Sharp, Icon::Rectangle, "Sharp"),
                                        (EdgeStyle::Rounded, Icon::Rounded, "Rounded"),
                                    ] {
                                        if property_choice(
                                            ui,
                                            icon,
                                            label,
                                            style.edges == edges,
                                            self.dark_mode,
                                        )
                                        .clicked()
                                        {
                                            patch.edges = Some(edges);
                                            changed = true;
                                        }
                                    }
                                });
                            }

                            ui.add_space(10.0);
                            section_label(ui, "Opacity", self.dark_mode);
                            let mut opacity = style.opacity;
                            // Leave room for the properties scroll bar and the
                            // slider handle so the two controls never overlap.
                            let slider_width = (ui.available_width() - 18.0).max(80.0);
                            let track = if self.dark_mode {
                                DARK_BORDER
                            } else {
                                LIGHT_BORDER
                            };
                            let opacity_tooltip = format!("{:.0}%", opacity * 100.0);
                            let slider_response = range_slider(
                                ui,
                                &mut opacity,
                                0.0..=1.0,
                                Vec2::new(slider_width, 16.0),
                                track,
                                ACCENT,
                                Some(opacity_tooltip),
                            );
                            if slider_response.changed() {
                                patch.opacity = Some(opacity);
                                changed = true;
                            }
                            if self.selected.len() > 1 {
                                ui.add_space(10.0);
                                section_label(ui, "Align", self.dark_mode);
                                ui.horizontal_wrapped(|ui| {
                                    for (action, icon, label) in [
                                        (AlignAction::Left, Icon::AlignItemLeft, "Align left"),
                                        (
                                            AlignAction::CenterHorizontal,
                                            Icon::AlignItemHorizontalCenter,
                                            "Align center",
                                        ),
                                        (AlignAction::Right, Icon::AlignItemRight, "Align right"),
                                        (AlignAction::Top, Icon::AlignItemTop, "Align top"),
                                        (
                                            AlignAction::CenterVertical,
                                            Icon::AlignItemVerticalCenter,
                                            "Align middle",
                                        ),
                                        (
                                            AlignAction::Bottom,
                                            Icon::AlignItemBottom,
                                            "Align bottom",
                                        ),
                                    ] {
                                        if icon_button(ui, icon, label, false, self.dark_mode)
                                            .clicked()
                                        {
                                            align_action = Some(action);
                                        }
                                    }
                                });
                            }

                            if !self.selected.is_empty() {
                                ui.add_space(10.0);
                                section_label(ui, "Layers", self.dark_mode);
                                ui.horizontal(|ui| {
                                    for (action, icon, label) in [
                                        (
                                            LayerAction::SendToBack,
                                            Icon::LayerSendToBack,
                                            "Send to back",
                                        ),
                                        (
                                            LayerAction::SendBackward,
                                            Icon::LayerSendBackward,
                                            "Send backward",
                                        ),
                                        (
                                            LayerAction::BringForward,
                                            Icon::LayerBringForward,
                                            "Bring forward",
                                        ),
                                        (
                                            LayerAction::BringToFront,
                                            Icon::LayerBringToFront,
                                            "Bring to front",
                                        ),
                                    ] {
                                        if icon_button(ui, icon, label, false, self.dark_mode)
                                            .clicked()
                                        {
                                            layer_action = Some(action);
                                        }
                                    }
                                });

                                ui.add_space(10.0);
                                section_label(ui, "Actions", self.dark_mode);
                                ui.horizontal(|ui| {
                                    if icon_button(
                                        ui,
                                        Icon::Duplicate,
                                        "Duplicate",
                                        false,
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        element_action = Some(ElementAction::Duplicate);
                                    }
                                    if icon_button(
                                        ui,
                                        Icon::Delete,
                                        "Delete",
                                        false,
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        element_action = Some(ElementAction::Delete);
                                    }
                                    if icon_button(
                                        ui,
                                        Icon::Link,
                                        "Copy element link",
                                        false,
                                        self.dark_mode,
                                    )
                                    .clicked()
                                    {
                                        element_action = Some(ElementAction::CopyLink);
                                    }
                                });
                            }
                        });
                });
            });

        if let Some(target) = color_target {
            self.color_picker = Some(target);
            self.color_picker_detail = None;
        }
        if changed {
            self.color_picker = None;
            self.color_picker_detail = None;
            self.apply_style_patch(context, editor, patch);
        }
        if let Some(action) = layer_action {
            self.apply_layer_action(editor, action);
        }
        if let Some(action) = align_action {
            self.apply_align_action(editor, action);
        }
        if let Some(action) = element_action {
            self.apply_element_action(context, editor, action);
        }
    }

    fn property_style(&self, editor: &Editor) -> Style {
        self.selected
            .iter()
            .next()
            .and_then(|selected| {
                editor
                    .document()
                    .element(*selected)
                    .map(|element| element.style)
            })
            .unwrap_or(self.draft_style)
    }

    #[allow(clippy::too_many_lines)]
    fn show_color_picker(&mut self, context: &egui::Context, editor: &mut Editor) {
        let Some(target) = self.color_picker else {
            return;
        };
        let style = self.property_style(editor);
        let current = match target {
            ColorPickerTarget::Stroke => to_color32(style.stroke),
            ColorPickerTarget::Fill => style
                .fill
                .map_or(Color32::from_rgb(221, 214, 254), to_color32),
        };
        let mut picked = None;
        let mut close_after_pick = false;
        let mut picker_rect = Rect::NOTHING;
        let mut detail_rect = Rect::NOTHING;
        let mut detail_open = self.color_picker_detail.is_some();
        egui::Area::new(Id::new("sketchi.color_picker"))
            .fixed_pos(egui::pos2(306.0, 76.0))
            .order(egui::Order::Tooltip)
            .show(context, |ui| {
                let frame = egui::Frame::new()
                    .fill(if self.dark_mode {
                        DARK_PANEL
                    } else {
                        LIGHT_PANEL
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if self.dark_mode {
                            DARK_BORDER
                        } else {
                            LIGHT_BORDER
                        },
                    ))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(236.0);
                        ui.label(
                            egui::RichText::new("Colors")
                                .size(13.0)
                                .strong()
                                .color(text_color(self.dark_mode)),
                        );
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                            let transparent_selected = match target {
                                ColorPickerTarget::Stroke => current.a() == 0,
                                ColorPickerTarget::Fill => style.fill.is_none(),
                            };
                            if color_swatch(ui, None, transparent_selected, self.dark_mode)
                                .on_hover_text("Transparent")
                                .clicked()
                            {
                                picked = Some(Color32::TRANSPARENT);
                                close_after_pick = true;
                            }
                            for color in COLOR_PICKER_COLORS {
                                if color_swatch(ui, Some(color), color == current, self.dark_mode)
                                    .clicked()
                                {
                                    picked = Some(color);
                                    close_after_pick = true;
                                }
                            }
                        });
                        ui.add_space(14.0);
                        section_label(ui, "Shades", self.dark_mode);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for factor in [0.65_f32, 0.8, 1.0, 1.15, 1.3] {
                                let color = shade_color(current, factor);
                                if color_swatch(ui, Some(color), color == current, self.dark_mode)
                                    .clicked()
                                {
                                    picked = Some(color);
                                    close_after_pick = true;
                                }
                            }
                        });
                        ui.add_space(14.0);
                        section_label(ui, "Color picker", self.dark_mode);
                        if color_picker_trigger(ui, current, self.dark_mode).clicked() {
                            detail_open = !detail_open;
                        }
                    });
                picker_rect = frame.response.rect;
            });

        if detail_open {
            let detail_position = picker_rect.right_top() + Vec2::new(8.0, 0.0);
            egui::Area::new(Id::new("sketchi.color_picker.detail"))
                .fixed_pos(detail_position)
                .order(egui::Order::Tooltip)
                .show(context, |ui| {
                    let frame = egui::Frame::new()
                        .fill(if self.dark_mode {
                            DARK_PANEL
                        } else {
                            LIGHT_PANEL
                        })
                        .stroke(Stroke::new(
                            1.0_f32,
                            if self.dark_mode {
                                DARK_BORDER
                            } else {
                                LIGHT_BORDER
                            },
                        ))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width(178.0);
                            let mut picker_color = current;
                            if color_picker_editor(ui, &mut picker_color, self.dark_mode) {
                                picked = Some(picker_color);
                            }
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                let (preview, _) =
                                    ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                                ui.painter().rect_filled(
                                    preview,
                                    CornerRadius::same(4),
                                    picker_color,
                                );
                                ui.painter().rect_stroke(
                                    preview,
                                    CornerRadius::same(4),
                                    Stroke::new(
                                        1.0_f32,
                                        if self.dark_mode {
                                            DARK_BORDER
                                        } else {
                                            LIGHT_BORDER
                                        },
                                    ),
                                    StrokeKind::Inside,
                                );
                                let [red, green, blue, _] = picker_color.to_array();
                                ui.vertical(|ui| {
                                    ui.label(format!("#{red:02x}{green:02x}{blue:02x}"));
                                    ui.label(
                                        egui::RichText::new(format!("rgb({red}, {green}, {blue})"))
                                            .small()
                                            .color(text_color(self.dark_mode)),
                                    );
                                });
                            });
                        });
                    detail_rect = frame.response.rect;
                });
        }

        if let Some(color) = picked {
            let patch = color_picker_patch(target, color);
            self.apply_style_patch(context, editor, patch);
        }
        let outside_click = context.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|position| {
                    !picker_rect.contains(position) && !detail_rect.contains(position)
                })
        });
        if context.input(|input| input.key_pressed(egui::Key::Escape))
            || outside_click
            || close_after_pick
        {
            self.color_picker = None;
            detail_open = false;
        }
        self.color_picker_detail = detail_open.then_some(());
    }

    fn execute_editor_command(
        editor: &mut Editor,
        command: EditorCommand,
    ) -> Result<canvas_core::OperationId, crate::editor::EditorError> {
        let command = match command {
            EditorCommand::Create(mut element) => {
                element.z_index = next_z_index(&editor.document());
                EditorCommand::Create(element)
            }
            command => command,
        };
        editor.execute(command)
    }

    fn apply_style_patch(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        patch: StylePatch,
    ) {
        let next_style = patch.apply_to(self.property_style(editor));
        if self.selected.is_empty() {
            self.draft_style = next_style;
            if let Some(text_edit) = &mut self.text_edit {
                text_edit.style = next_style;
            }
            self.status = String::from("New object style updated");
        } else {
            let selected = self.selected.iter().copied().collect::<Vec<_>>();
            let resize_text = patch.font_family.is_some() || patch.font_size.is_some();
            let mut failed = None;
            for element_id in selected {
                let Some(element) = editor.document().element(element_id).cloned() else {
                    continue;
                };
                let next_element_style = patch.apply_to(element.style);
                if let Err(error) = editor.execute(EditorCommand::SetStyle(element_id, patch)) {
                    failed = Some(error.to_string());
                    break;
                }
                if resize_text && element.kind == ElementKind::Text {
                    let next_transform =
                        resized_text_transform(context, &element, next_element_style);
                    if next_transform.position != element.transform.position
                        && let Err(error) = editor.execute(EditorCommand::SetPosition(
                            element_id,
                            next_transform.position,
                        ))
                    {
                        failed = Some(error.to_string());
                        break;
                    }
                    if next_transform.size != element.transform.size
                        && let Err(error) =
                            editor.execute(EditorCommand::SetSize(element_id, next_transform.size))
                    {
                        failed = Some(error.to_string());
                        break;
                    }
                }
            }
            self.draft_style = next_style;
            self.status = failed.map_or_else(
                || String::from("Style updated"),
                |error| format!("Could not update style: {error}"),
            );
        }
    }

    fn apply_layer_action(&mut self, editor: &mut Editor, action: LayerAction) {
        let document = editor.document();
        let mut ordered = document
            .elements()
            .map(|element| (element.z_index, element.id))
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        let ordered_ids = ordered
            .iter()
            .map(|(_, element_id)| *element_id)
            .collect::<Vec<_>>();
        let next_order = reordered_layer_ids(&ordered_ids, &self.selected, action);
        let current_z = ordered
            .iter()
            .map(|(z_index, element_id)| (*element_id, *z_index))
            .collect::<HashMap<_, _>>();
        let mut updated = 0_usize;
        for (index, element_id) in next_order.into_iter().enumerate() {
            let z_index = i64::try_from(index).unwrap_or(i64::MAX);
            if current_z.get(&element_id).copied() == Some(z_index) {
                continue;
            }
            if editor
                .execute(EditorCommand::Reorder(element_id, z_index))
                .is_ok()
            {
                updated += 1;
            }
        }
        self.status = format!("Layer order updated for {updated} objects");
    }

    fn apply_align_action(&mut self, editor: &mut Editor, action: AlignAction) {
        let document = editor.document();
        let elements = self
            .selected
            .iter()
            .filter_map(|id| document.element(*id).cloned())
            .collect::<Vec<_>>();
        let Some(group) = selection_bounds(elements.iter()) else {
            return;
        };
        let group_max = group.max();
        let mut aligned = 0_usize;
        for element in elements {
            let bounds = element_bounds(&element);
            let delta = match action {
                AlignAction::Left => Point::new(group.min.x - bounds.min.x, 0.0),
                AlignAction::CenterHorizontal => Point::new(
                    group.min.x + group.size.width / 2.0 - (bounds.min.x + bounds.size.width / 2.0),
                    0.0,
                ),
                AlignAction::Right => Point::new(group_max.x - bounds.max().x, 0.0),
                AlignAction::Top => Point::new(0.0, group.min.y - bounds.min.y),
                AlignAction::CenterVertical => Point::new(
                    0.0,
                    group.min.y + group.size.height / 2.0
                        - (bounds.min.y + bounds.size.height / 2.0),
                ),
                AlignAction::Bottom => Point::new(0.0, group_max.y - bounds.max().y),
            };
            if delta == Point::default() {
                continue;
            }
            let translated = translated_element(&element, delta);
            if editor
                .execute(EditorCommand::SetPosition(
                    element.id,
                    translated.transform.position,
                ))
                .is_ok()
            {
                if translated.points != element.points {
                    let _ = editor.execute(EditorCommand::SetPoints(element.id, translated.points));
                }
                aligned += 1;
            }
        }
        self.status = format!("Aligned {aligned} objects");
    }

    fn apply_element_action(
        &mut self,
        context: &egui::Context,
        editor: &mut Editor,
        action: ElementAction,
    ) {
        match action {
            ElementAction::Delete => self.delete_selected(editor),
            ElementAction::Duplicate => {
                let elements = self
                    .selected
                    .iter()
                    .filter_map(|id| editor.document().element(*id).cloned())
                    .collect::<Vec<_>>();
                let mut duplicated = BTreeSet::new();
                for (index, element) in elements.into_iter().enumerate() {
                    let offset = 24.0 + f32::from(u16::try_from(index).unwrap_or(u16::MAX)) * 8.0;
                    let mut copy = translated_element(&element, Point::new(offset, offset));
                    copy.id = ElementId::new();
                    if Self::execute_editor_command(editor, EditorCommand::Create(copy.clone()))
                        .is_ok()
                    {
                        duplicated.insert(copy.id);
                    }
                }
                if !duplicated.is_empty() {
                    self.selected = duplicated;
                    self.status = String::from("Objects duplicated");
                }
            }
            ElementAction::CopyLink => {
                if let Some(element_id) = self.selected.iter().next() {
                    context.copy_text(format!("sketchi://element/{element_id}"));
                    self.status = String::from("Element link copied");
                }
            }
        }
    }

    fn show_zoom_controls(&mut self, context: &egui::Context, camera: &mut Camera) {
        let mut zoom_out = false;
        let mut zoom_in = false;
        let mut reset = false;
        egui::Area::new(Id::new("sketchi.zoom"))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                toolbar_frame(self.dark_mode).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icon_button(ui, Icon::ZoomOut, "Zoom out", false, self.dark_mode)
                            .clicked()
                        {
                            zoom_out = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(zoom_percent(camera.zoom()))
                                        .size(12.0)
                                        .color(text_color(self.dark_mode)),
                                )
                                .frame(false)
                                .min_size(Vec2::new(48.0, 30.0)),
                            )
                            .on_hover_text("Reset zoom")
                            .clicked()
                        {
                            reset = true;
                        }
                        if icon_button(ui, Icon::ZoomIn, "Zoom in", false, self.dark_mode).clicked()
                        {
                            zoom_in = true;
                        }
                    });
                });
            });

        let viewport = camera.viewport();
        let center = Point::new(viewport.width / 2.0, viewport.height / 2.0);
        if zoom_out {
            camera.zoom_by(center, -0.15);
        }
        if zoom_in {
            camera.zoom_by(center, 0.15);
        }
        if reset {
            camera.zoom_to(center, 1.0);
            self.status = String::from("Zoom reset");
        }
    }

    fn show_status(&self, context: &egui::Context, editor: &Editor, camera: &Camera) {
        egui::Area::new(Id::new("sketchi.status"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(context, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}  ·  {} objects  ·  {}",
                        tool_name(self.active_tool),
                        editor.document().len(),
                        zoom_percent(camera.zoom())
                    ))
                    .small()
                    .color(muted_color(self.dark_mode)),
                );
                ui.label(
                    egui::RichText::new(&self.status)
                        .small()
                        .color(muted_color(self.dark_mode)),
                );
            });
    }
}

fn settings_window_frame(dark_mode: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if dark_mode {
            Color32::from_rgb(31, 32, 37)
        } else {
            Color32::from_rgb(255, 255, 255)
        })
        .stroke(Stroke::new(
            1.0_f32,
            if dark_mode { DARK_BORDER } else { LIGHT_BORDER },
        ))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(24))
}

fn settings_group_frame(dark_mode: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if dark_mode {
            Color32::from_rgb(39, 40, 46)
        } else {
            Color32::from_rgb(248, 249, 251)
        })
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(14))
}

fn settings_form_row(
    ui: &mut egui::Ui,
    label: &str,
    label_width: f32,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (label_rect, _) = ui.allocate_exact_size(
            Vec2::new(label_width, STANDARD_CONTROL_SIZE.y),
            Sense::hover(),
        );
        ui.put(label_rect, egui::Label::new(label).truncate());
        add_control(ui);
    });
}

fn settings_nav_item(
    ui: &mut egui::Ui,
    label: &str,
    icon: Icon,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(160.0, 38.0), Sense::click());
    let fill = if selected {
        if dark_mode {
            Color32::from_rgb(65, 62, 128)
        } else {
            Color32::from_rgb(232, 230, 255)
        }
    } else if response.hovered() {
        if dark_mode {
            Color32::from_rgb(48, 49, 56)
        } else {
            Color32::from_rgb(246, 247, 250)
        }
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        fill,
        Stroke::NONE,
        StrokeKind::Inside,
    );
    paint_remix_icon(
        ui.painter(),
        icon,
        Rect::from_center_size(
            Pos2::new(rect.left() + 22.0, rect.center().y),
            Vec2::splat(22.0),
        ),
        if selected {
            ACCENT
        } else {
            muted_color(dark_mode)
        },
    );
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.0, egui::FontFamily::Proportional),
        if selected {
            text_color(dark_mode)
        } else {
            muted_color(dark_mode)
        },
    );
    response
}

fn key_binding_label(binding: KeyBinding) -> String {
    let mut label = String::new();
    if binding.modifiers.command {
        label.push_str("Ctrl + ");
    }
    if binding.modifiers.alt {
        label.push_str("Alt + ");
    }
    if binding.modifiers.shift {
        label.push_str("Shift + ");
    }
    let _ = write!(label, "{:?}", binding.key);
    label
}

fn keybind_action_name(action: KeybindAction) -> &'static str {
    action.label()
}

fn toolbar_frame(dark_mode: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if dark_mode { DARK_PANEL } else { LIGHT_PANEL })
        .stroke(Stroke::new(
            1.0_f32,
            if dark_mode { DARK_BORDER } else { LIGHT_BORDER },
        ))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(6, 6))
}

fn properties_frame(dark_mode: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(if dark_mode {
            Color32::from_rgba_unmultiplied(37, 38, 43, 250)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 250)
        })
        .stroke(Stroke::new(
            1.0_f32,
            if dark_mode { DARK_BORDER } else { LIGHT_BORDER },
        ))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 12))
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    tooltip: &str,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    let fill = if selected {
        ACCENT
    } else if response.hovered() {
        if dark_mode {
            Color32::from_rgb(58, 60, 68)
        } else {
            Color32::from_rgb(241, 242, 246)
        }
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(4),
        fill,
        Stroke::NONE,
        StrokeKind::Inside,
    );
    let icon_color = if selected {
        Color32::WHITE
    } else {
        text_color(dark_mode)
    };
    paint_remix_icon(ui.painter(), icon, rect, icon_color);
    response.on_hover_text(tooltip)
}

fn paint_remix_icon(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.glyph().to_string(),
        FontId::new(
            18.0,
            egui::FontFamily::Name(remix_icons::FONT_FAMILY.into()),
        ),
        color,
    );
}

fn section_label(ui: &mut egui::Ui, label: &str, dark_mode: bool) {
    ui.label(
        egui::RichText::new(label)
            .size(12.0)
            .color(muted_color(dark_mode)),
    );
    ui.add_space(4.0);
}

fn next_settings_color(current: Color32) -> Color32 {
    let colors = [
        LIGHT_CANVAS,
        DARK_CANVAS,
        Color32::from_rgb(31, 31, 31),
        Color32::from_rgb(224, 49, 49),
        Color32::from_rgb(47, 158, 68),
        Color32::from_rgb(25, 113, 194),
        Color32::from_rgb(240, 140, 0),
        Color32::from_rgb(121, 80, 242),
    ];
    let index = colors
        .iter()
        .position(|color| *color == current)
        .unwrap_or(0);
    colors
        .get((index + 1) % colors.len())
        .copied()
        .unwrap_or(current)
}

fn kde_system_dark_mode() -> Option<bool> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let contents = fs::read_to_string(config_home.join("kdeglobals")).ok()?;
    let scheme = contents
        .lines()
        .find_map(|line| line.strip_prefix("ColorScheme="))?
        .trim()
        .to_ascii_lowercase();
    Some(scheme.contains("dark"))
}

fn property_choice(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = choice_card(ui, selected, dark_mode);
    paint_remix_icon(
        ui.painter(),
        icon,
        Rect::from_center_size(
            Pos2::new(rect.center().x, rect.center().y - 6.0),
            Vec2::splat(22.0),
        ),
        if selected {
            ACCENT
        } else {
            text_color(dark_mode)
        },
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        muted_color(dark_mode),
    );
    response.on_hover_text(label)
}

fn width_choice(ui: &mut egui::Ui, width: f32, selected: bool, dark_mode: bool) -> egui::Response {
    let (rect, response) = choice_card(ui, selected, dark_mode);
    let center = rect.center();
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + 14.0, center.y),
            Pos2::new(rect.right() - 14.0, center.y),
        ],
        Stroke::new(
            width,
            if selected {
                ACCENT
            } else {
                text_color(dark_mode)
            },
        ),
    );
    response.on_hover_text(format!("{width:.0}px stroke"))
}

fn text_size_choice(
    ui: &mut egui::Ui,
    size: f32,
    label: &str,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = text_choice_card(ui, Vec2::splat(32.0), selected, dark_mode);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        if selected {
            ACCENT
        } else {
            text_color(dark_mode)
        },
    );
    response.on_hover_text(format!("{size:.0}px text"))
}

fn custom_text_size_choice(ui: &mut egui::Ui, selected: bool, dark_mode: bool) -> egui::Response {
    let (rect, response) = text_choice_card(ui, Vec2::splat(32.0), selected, dark_mode);
    paint_remix_icon(
        ui.painter(),
        Icon::CustomSize,
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        if selected {
            ACCENT
        } else {
            text_color(dark_mode)
        },
    );
    response.on_hover_text("Choose a custom text size")
}

fn stroke_style_choice(
    ui: &mut egui::Ui,
    stroke_style: StrokeStyle,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = choice_card(ui, selected, dark_mode);
    let stroke = Stroke::new(
        2.0_f32,
        if selected {
            ACCENT
        } else {
            text_color(dark_mode)
        },
    );
    paint_styled_segment(
        ui.painter(),
        Pos2::new(rect.left() + 10.0, rect.center().y),
        Pos2::new(rect.right() - 10.0, rect.center().y),
        stroke,
        stroke_style,
    );
    response
}

fn choice_card(ui: &mut egui::Ui, selected: bool, dark_mode: bool) -> (Rect, egui::Response) {
    choice_card_with_size(ui, Vec2::new(54.0, 42.0), selected, dark_mode)
}

fn text_property_icon_choice(
    ui: &mut egui::Ui,
    icon: Icon,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    let (rect, response) = text_choice_card(ui, Vec2::splat(32.0), selected, dark_mode);
    paint_remix_icon(
        ui.painter(),
        icon,
        rect,
        if selected {
            ACCENT
        } else {
            text_color(dark_mode)
        },
    );
    response
}

fn text_choice_card(
    ui: &mut egui::Ui,
    size: Vec2,
    selected: bool,
    dark_mode: bool,
) -> (Rect, egui::Response) {
    let (rect, response) = choice_card_with_size(ui, size, selected, dark_mode);
    if selected {
        ui.painter().rect_stroke(
            rect.shrink(0.5),
            CornerRadius::same(5),
            Stroke::new(1.0_f32, ACCENT),
            StrokeKind::Inside,
        );
    }
    (rect, response)
}

fn choice_card_with_size(
    ui: &mut egui::Ui,
    size: Vec2,
    selected: bool,
    dark_mode: bool,
) -> (Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected {
        if dark_mode {
            Color32::from_rgb(58, 57, 92)
        } else {
            Color32::from_rgb(232, 230, 255)
        }
    } else if response.hovered() {
        if dark_mode {
            Color32::from_rgb(52, 53, 60)
        } else {
            Color32::from_rgb(245, 246, 249)
        }
    } else if dark_mode {
        Color32::from_rgb(47, 48, 54)
    } else {
        Color32::from_rgb(247, 248, 250)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
    (rect, response)
}

fn paint_styled_segment(
    painter: &Painter,
    start: Pos2,
    end: Pos2,
    stroke: Stroke,
    stroke_style: StrokeStyle,
) {
    if stroke_style == StrokeStyle::Solid {
        painter.line_segment([start, end], stroke);
        return;
    }
    let direction = end - start;
    let length = direction.length();
    if length <= f32::EPSILON {
        return;
    }
    let unit = direction / length;
    let step = match stroke_style {
        StrokeStyle::Solid => length,
        StrokeStyle::Dashed => 12.0,
        StrokeStyle::Dotted => 6.0,
    };
    let mut distance = 0.0;
    while distance < length {
        let next = (distance + step).min(length);
        let from = start + unit * distance;
        let to = start + unit * next;
        match stroke_style {
            StrokeStyle::Dashed if (distance / step).floor().rem_euclid(2.0) < 1.0 => {
                painter.line_segment([from, to], stroke);
            }
            StrokeStyle::Dotted => {
                painter.circle_filled(from, (stroke.width / 2.0).max(1.0), stroke.color);
            }
            _ => {}
        }
        distance = next;
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn shade_color(color: Color32, factor: f32) -> Color32 {
    let scale = if factor.is_finite() {
        factor.clamp(0.0, 2.0)
    } else {
        1.0
    };
    Color32::from_rgba_unmultiplied(
        (f32::from(color.r()) * scale).round().clamp(0.0, 255.0) as u8,
        (f32::from(color.g()) * scale).round().clamp(0.0, 255.0) as u8,
        (f32::from(color.b()) * scale).round().clamp(0.0, 255.0) as u8,
        color.a(),
    )
}

fn paint_dot_grid(painter: &Painter, canvas: Rect, camera: Camera, dark_mode: bool) {
    let step = grid_step_for_zoom(camera.zoom());
    let top_left = camera.screen_to_world(Point::new(canvas.left(), canvas.top()));
    let bottom_right = camera.screen_to_world(Point::new(canvas.right(), canvas.bottom()));
    let start_x = (top_left.x / step).floor() * step;
    let start_y = (top_left.y / step).floor() * step;
    let dot_color = if dark_mode {
        Color32::from_rgb(58, 60, 67)
    } else {
        Color32::from_rgb(191, 194, 201)
    };
    let mut x = start_x;
    while x <= bottom_right.x + step {
        let mut y = start_y;
        while y <= bottom_right.y + step {
            let screen = camera.world_to_screen(Point::new(x, y));
            painter.circle_filled(Pos2::new(screen.x, screen.y), 1.15, dot_color);
            y += step;
        }
        x += step;
    }
}

fn paint_drop_target(painter: &Painter, canvas: Rect, dark_mode: bool) {
    let target = canvas.shrink(24.0);
    let fill = if dark_mode {
        Color32::from_rgba_unmultiplied(91, 87, 214, 34)
    } else {
        Color32::from_rgba_unmultiplied(91, 87, 214, 24)
    };
    painter.rect_filled(target, CornerRadius::same(10), fill);
    painter.rect_stroke(
        target,
        CornerRadius::same(10),
        Stroke::new(2.0_f32, ACCENT),
        StrokeKind::Inside,
    );
    painter.text(
        target.center(),
        Align2::CENTER_CENTER,
        "Drop image to embed",
        FontId::new(18.0, egui::FontFamily::Proportional),
        text_color(dark_mode),
    );
}

fn grid_step_for_zoom(zoom: f32) -> f32 {
    let zoom = if zoom.is_finite() {
        zoom.clamp(0.05, 64.0)
    } else {
        1.0
    };
    for step in [
        0.5_f32, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0,
    ] {
        if step * zoom >= 24.0 {
            return step;
        }
    }
    1024.0
}

fn paint_document(
    painter: &Painter,
    document: &Document,
    camera: Camera,
    hidden: Option<&BTreeSet<ElementId>>,
    editing_element_id: Option<ElementId>,
    image_textures: &mut HashMap<ElementId, egui::TextureHandle>,
) {
    let mut elements = document
        .elements()
        .filter(|element| {
            !hidden.is_some_and(|hidden| hidden.contains(&element.id))
                && editing_element_id != Some(element.id)
        })
        .collect::<Vec<_>>();
    elements.sort_by_key(|element| (element.z_index, element.id));
    for element in elements {
        paint_element(painter, element, camera, image_textures);
    }
}

fn next_z_index(document: &Document) -> i64 {
    document
        .elements()
        .map(|element| element.z_index)
        .max()
        .map_or(0, |z_index| z_index.saturating_add(1))
}

fn text_font_id(style: &Style, zoom: f32) -> FontId {
    let font_family = match style.font_family {
        TextFontFamily::Handwritten => {
            egui::FontFamily::Name(remix_icons::HANDWRITTEN_FONT_FAMILY.into())
        }
        TextFontFamily::Monospace => egui::FontFamily::Monospace,
        TextFontFamily::Sans | TextFontFamily::Serif => egui::FontFamily::Proportional,
    };
    FontId::new((style.font_size * zoom).max(1.0), font_family)
}

const TEXT_BOUNDS_PADDING: f32 = 2.0;

fn measured_text_size(context: &egui::Context, text: &str, style: &Style) -> Size {
    let galley = context.fonts_mut(|fonts| {
        fonts.layout_no_wrap(text.to_owned(), text_font_id(style, 1.0), Color32::WHITE)
    });
    let measured = galley.size();
    let minimum_width = (style.font_size * 0.5).max(4.0);
    let minimum_height = (style.font_size * 1.25).max(4.0);
    Size::new(
        (measured.x + TEXT_BOUNDS_PADDING * 2.0).max(minimum_width),
        (measured.y + TEXT_BOUNDS_PADDING * 2.0).max(minimum_height),
    )
}

fn resized_text_transform(context: &egui::Context, element: &Element, style: Style) -> Transform {
    resized_text_transform_for_content(context, element, &element.text, style)
}

fn resized_text_transform_for_content(
    context: &egui::Context,
    element: &Element,
    text: &str,
    style: Style,
) -> Transform {
    let size = measured_text_size(context, text, &style);
    let width_delta = element.transform.size.width - size.width;
    let mut position = element.transform.position;
    match style.text_align {
        TextAlign::Left => {}
        TextAlign::Center => position.x += width_delta / 2.0,
        TextAlign::Right => position.x += width_delta,
    }
    Transform {
        position,
        size,
        rotation: element.transform.rotation,
    }
}

fn full_style_patch(style: Style) -> StylePatch {
    StylePatch {
        stroke: Some(style.stroke),
        fill: Some(style.fill),
        stroke_width: Some(style.stroke_width),
        stroke_style: Some(style.stroke_style),
        sloppiness: Some(style.sloppiness),
        edges: Some(style.edges),
        opacity: Some(style.opacity),
        font_family: Some(style.font_family),
        font_size: Some(style.font_size),
        text_align: Some(style.text_align),
    }
}

fn text_update_command(element_id: ElementId, text: &str) -> Option<EditorCommand> {
    (!text.trim().is_empty()).then(|| EditorCommand::SetText(element_id, text.to_owned()))
}

fn paint_text_edit_preview(painter: &Painter, text_edit: &TextEditState, camera: Camera) {
    let screen_position = camera.world_to_screen(text_edit.position);
    let screen_position = Pos2::new(screen_position.x, screen_position.y);
    let color = apply_opacity(to_color32(text_edit.style.stroke), text_edit.style.opacity);
    let font_id = text_font_id(&text_edit.style, camera.zoom());
    let galley = painter.layout_no_wrap(text_edit.text.clone(), font_id.clone(), color);
    let text_width = galley.size().x;
    let text_position = match text_edit.style.text_align {
        TextAlign::Left => screen_position,
        TextAlign::Center => screen_position - Vec2::new(text_width / 2.0, 0.0),
        TextAlign::Right => screen_position - Vec2::new(text_width, 0.0),
    };
    let text_rect = Rect::from_min_size(text_position, galley.size());
    let rotated_position = rotated_text_origin(text_position, text_rect, text_edit.rotation);
    painter.add(TextShape::new(rotated_position, galley, color).with_angle(text_edit.rotation));

    let caret_x = match text_edit.style.text_align {
        TextAlign::Left => screen_position.x + text_width,
        TextAlign::Center => screen_position.x + text_width / 2.0,
        TextAlign::Right => screen_position.x,
    };
    let caret_height = (font_id.size * 1.25).max(16.0);
    let caret_start = rotate_screen_point(
        Pos2::new(caret_x, screen_position.y),
        text_rect.center(),
        text_edit.rotation,
    );
    let caret_end = rotate_screen_point(
        Pos2::new(caret_x, screen_position.y + caret_height),
        text_rect.center(),
        text_edit.rotation,
    );
    painter.line_segment([caret_start, caret_end], Stroke::new(1.2_f32, color));
}

#[allow(clippy::too_many_lines)]
fn paint_element(
    painter: &Painter,
    element: &Element,
    camera: Camera,
    image_textures: &mut HashMap<ElementId, egui::TextureHandle>,
) {
    let min = camera.world_to_screen(element.transform.position);
    let size = Vec2::new(
        element.transform.size.width * camera.zoom(),
        element.transform.size.height * camera.zoom(),
    );
    let rect = Rect::from_min_size(Pos2::new(min.x, min.y), size);
    let stroke_color = apply_opacity(to_color32(element.style.stroke), element.style.opacity);
    let stroke = Stroke::new(
        (element.style.stroke_width * camera.zoom()).clamp(1.0, 12.0),
        stroke_color,
    );
    let fill = element.style.fill.map_or(Color32::TRANSPARENT, |color| {
        apply_opacity(to_color32(color), element.style.opacity)
    });
    let corner_radius = corner_radius(element.style.edges, camera.zoom());
    match element.kind {
        ElementKind::Rectangle => {
            if element.transform.rotation.abs() > f32::EPSILON {
                let base_points = rotated_screen_points(
                    rounded_rect_points(rect, corner_radius),
                    rect.center(),
                    element.transform.rotation,
                );
                let points = sloppy_polyline(
                    &base_points,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    stroke.width,
                    true,
                );
                painter.add(egui::Shape::convex_polygon(
                    points.clone(),
                    fill,
                    Stroke::NONE,
                ));
                paint_sloppiness_outline(
                    painter,
                    &base_points,
                    stroke,
                    element.style.stroke_style,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    true,
                );
            } else if element.style.sloppiness != Sloppiness::Architect {
                let base_points = rounded_rect_points(rect, corner_radius);
                let points = sloppy_polyline(
                    &base_points,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    stroke.width,
                    true,
                );
                painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
                paint_sloppiness_outline(
                    painter,
                    &base_points,
                    stroke,
                    element.style.stroke_style,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    true,
                );
            } else {
                painter.rect_filled(rect, CornerRadius::same(corner_radius), fill);
                if element.style.stroke_style == StrokeStyle::Solid {
                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(corner_radius),
                        stroke,
                        StrokeKind::Middle,
                    );
                } else {
                    let points = rounded_rect_points(rect, corner_radius);
                    paint_styled_polyline(
                        painter,
                        &points,
                        stroke,
                        element.style.stroke_style,
                        true,
                    );
                }
            }
        }
        ElementKind::Diamond => {
            let base_points = diamond_points(rect, element.transform.rotation);
            let points = sloppy_polyline(
                &base_points,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                stroke.width,
                true,
            );
            painter.add(egui::Shape::convex_polygon(
                points.clone(),
                fill,
                Stroke::NONE,
            ));
            paint_sloppiness_outline(
                painter,
                &base_points,
                stroke,
                element.style.stroke_style,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                true,
            );
        }
        ElementKind::Triangle => {
            let base_points = triangle_points(rect, element.transform.rotation);
            let points = sloppy_polyline(
                &base_points,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                stroke.width,
                true,
            );
            painter.add(egui::Shape::convex_polygon(
                points.clone(),
                fill,
                Stroke::NONE,
            ));
            paint_sloppiness_outline(
                painter,
                &base_points,
                stroke,
                element.style.stroke_style,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                true,
            );
        }
        ElementKind::Ellipse => {
            if element.transform.rotation.abs() > f32::EPSILON {
                let base_points = rotated_screen_points(
                    ellipse_points(rect),
                    rect.center(),
                    element.transform.rotation,
                );
                let points = sloppy_polyline(
                    &base_points,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    stroke.width,
                    true,
                );
                painter.add(egui::Shape::convex_polygon(
                    points.clone(),
                    fill,
                    Stroke::NONE,
                ));
                paint_sloppiness_outline(
                    painter,
                    &base_points,
                    stroke,
                    element.style.stroke_style,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    true,
                );
            } else if element.style.sloppiness != Sloppiness::Architect {
                let base_points = ellipse_points(rect);
                let points = sloppy_polyline(
                    &base_points,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    stroke.width,
                    true,
                );
                painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
                paint_sloppiness_outline(
                    painter,
                    &base_points,
                    stroke,
                    element.style.stroke_style,
                    element.style.sloppiness,
                    element.id.as_uuid().as_u128(),
                    true,
                );
            } else {
                painter.add(egui::Shape::ellipse_filled(
                    rect.center(),
                    rect.size() / 2.0,
                    fill,
                ));
                if element.style.stroke_style == StrokeStyle::Solid {
                    painter.add(egui::Shape::ellipse_stroke(
                        rect.center(),
                        rect.size() / 2.0,
                        stroke,
                    ));
                } else {
                    let points = ellipse_points(rect);
                    paint_styled_polyline(
                        painter,
                        &points,
                        stroke,
                        element.style.stroke_style,
                        true,
                    );
                }
            }
        }
        ElementKind::Line | ElementKind::Arrow => {
            let base_points = element_screen_points(element, camera, rect);
            let points = sloppy_polyline(
                &base_points,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                stroke.width,
                false,
            );
            paint_sloppiness_outline(
                painter,
                &base_points,
                stroke,
                element.style.stroke_style,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
                false,
            );
            if element.kind == ElementKind::Arrow
                && let (Some(start), Some(end)) = (points.first(), points.last())
            {
                paint_arrowhead(painter, *start, *end, stroke);
            }
        }
        ElementKind::Freehand => {
            let center = Point::new(
                element.transform.position.x + element.transform.size.width / 2.0,
                element.transform.position.y + element.transform.size.height / 2.0,
            );
            let points: Vec<Pos2> = element
                .points
                .iter()
                .copied()
                .map(|point| {
                    let screen = camera.world_to_screen(rotate_around(
                        point,
                        center,
                        element.transform.rotation,
                    ));
                    Pos2::new(screen.x, screen.y)
                })
                .collect();
            paint_styled_polyline(painter, &points, stroke, element.style.stroke_style, false);
        }
        ElementKind::Text => {
            paint_text_element(painter, element, rect, stroke_color, camera);
        }
        ElementKind::Image => paint_image(
            painter,
            element,
            rect,
            stroke,
            corner_radius,
            element.transform.rotation,
            image_textures,
        ),
    }
}

fn paint_text_element(
    painter: &Painter,
    element: &Element,
    rect: Rect,
    color: Color32,
    camera: Camera,
) {
    let (anchor, anchor_position) = text_anchor(rect, element.style.text_align);
    let font_id = text_font_id(&element.style, camera.zoom());
    let galley = painter.layout_no_wrap(element.text.clone(), font_id, color);
    let text_position = anchor.anchor_size(anchor_position, galley.size()).min;
    let rotated_position = rotated_text_origin(text_position, rect, element.transform.rotation);
    painter.add(
        TextShape::new(rotated_position, galley, color).with_angle(element.transform.rotation),
    );
}

fn text_anchor(rect: Rect, align: TextAlign) -> (Align2, Pos2) {
    match align {
        TextAlign::Left => (Align2::LEFT_TOP, rect.left_top()),
        TextAlign::Center => (Align2::CENTER_TOP, Pos2::new(rect.center().x, rect.top())),
        TextAlign::Right => (Align2::RIGHT_TOP, rect.right_top()),
    }
}

fn rotated_text_origin(position: Pos2, rect: Rect, rotation: f32) -> Pos2 {
    rotate_screen_point(position, rect.center(), rotation)
}

fn paint_image(
    painter: &Painter,
    element: &Element,
    rect: Rect,
    stroke: Stroke,
    corner_radius: u8,
    rotation: f32,
    image_textures: &mut HashMap<ElementId, egui::TextureHandle>,
) {
    let Some(image) = element.image.as_ref() else {
        paint_image_placeholder(
            painter,
            rect,
            stroke,
            corner_radius,
            rotation,
            element.style.sloppiness,
            element.id.as_uuid().as_u128(),
        );
        return;
    };
    if let Entry::Vacant(entry) = image_textures.entry(element.id) {
        let Ok(decoded) = image::load_from_memory(&image.bytes) else {
            paint_image_placeholder(
                painter,
                rect,
                stroke,
                corner_radius,
                rotation,
                element.style.sloppiness,
                element.id.as_uuid().as_u128(),
            );
            return;
        };
        let rgba = decoded.to_rgba8();
        let width = usize::try_from(image.width).unwrap_or(usize::MAX);
        let height = usize::try_from(image.height).unwrap_or(usize::MAX);
        let color_image = egui::ColorImage::from_rgba_unmultiplied([width, height], rgba.as_raw());
        let texture = painter.ctx().load_texture(
            format!("sketchi.image.{}", element.id),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        entry.insert(texture);
    }
    if let Some(texture) = image_textures.get(&element.id) {
        let tint = apply_opacity(Color32::WHITE, element.style.opacity);
        let local_points = rounded_rect_points(rect, corner_radius);
        let points = rotated_screen_points(local_points.clone(), rect.center(), rotation);
        let mut mesh = Mesh::with_texture(texture.id());
        mesh.vertices.push(Vertex {
            pos: rect.center(),
            uv: Pos2::new(0.5, 0.5),
            color: tint,
        });
        for (point, local_point) in points.iter().zip(local_points.iter()) {
            mesh.vertices.push(Vertex {
                pos: *point,
                uv: image_uv(*local_point, rect),
                color: tint,
            });
        }
        for (index, _) in points.iter().enumerate() {
            let current = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let next = u32::try_from((index + 1) % points.len() + 1).unwrap_or(u32::MAX);
            mesh.add_triangle(0, current, next);
        }
        painter.add(egui::Shape::mesh(mesh));
        paint_sloppiness_outline(
            painter,
            &points,
            stroke,
            element.style.stroke_style,
            element.style.sloppiness,
            element.id.as_uuid().as_u128(),
            true,
        );
    }
}

fn image_uv(point: Pos2, rect: Rect) -> Pos2 {
    Pos2::new(
        ((point.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
        ((point.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
    )
}

fn paint_image_placeholder(
    painter: &Painter,
    rect: Rect,
    stroke: Stroke,
    corner_radius: u8,
    rotation: f32,
    sloppiness: Sloppiness,
    seed: u128,
) {
    let local_points = rounded_rect_points(rect, corner_radius);
    let points = rotated_screen_points(local_points, rect.center(), rotation);
    painter.add(egui::Shape::convex_polygon(
        points.clone(),
        Color32::from_gray(225),
        Stroke::NONE,
    ));
    paint_sloppiness_outline(
        painter,
        &points,
        stroke,
        StrokeStyle::Solid,
        sloppiness,
        seed,
        true,
    );
    let diagonals = rotated_screen_points(
        vec![
            rect.left_top(),
            rect.right_bottom(),
            rect.right_top(),
            rect.left_bottom(),
        ],
        rect.center(),
        rotation,
    );
    if let [top_left, bottom_right, top_right, bottom_left] = diagonals.as_slice() {
        painter.line_segment([*top_left, *bottom_right], stroke);
        painter.line_segment([*top_right, *bottom_left], stroke);
    }
}

fn corner_radius(edges: EdgeStyle, zoom: f32) -> u8 {
    if edges == EdgeStyle::Rounded {
        rounded_corner_radius(zoom)
    } else {
        0
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_corner_radius(zoom: f32) -> u8 {
    (8.0 * zoom).clamp(0.0, 32.0) as u8
}

fn paint_styled_polyline(
    painter: &Painter,
    points: &[Pos2],
    stroke: Stroke,
    stroke_style: StrokeStyle,
    close: bool,
) {
    if points.len() < 2 {
        if let Some(point) = points.first() {
            painter.circle_filled(*point, (stroke.width / 2.0).max(1.0), stroke.color);
        }
        return;
    }
    for segment in points.windows(2) {
        if let (Some(start), Some(end)) = (segment.first(), segment.get(1)) {
            paint_styled_segment(painter, *start, *end, stroke, stroke_style);
        }
    }
    if close && let (Some(first), Some(last)) = (points.first(), points.last()) {
        paint_styled_segment(painter, *last, *first, stroke, stroke_style);
    }
}

fn paint_sloppiness_outline(
    painter: &Painter,
    points: &[Pos2],
    stroke: Stroke,
    stroke_style: StrokeStyle,
    sloppiness: Sloppiness,
    seed: u128,
    close: bool,
) {
    let primary = sloppy_polyline(points, sloppiness, seed, stroke.width, close);
    paint_styled_polyline(painter, &primary, stroke, stroke_style, close);
    if sloppiness != Sloppiness::Architect {
        let secondary_scale = if sloppiness == Sloppiness::Artist {
            0.45
        } else {
            0.72
        };
        let secondary = sloppy_polyline_scaled(
            points,
            sloppiness,
            seed.wrapping_add(0xD1B5_4A32_D192_ED03),
            stroke.width,
            close,
            secondary_scale,
        );
        let secondary_stroke = Stroke::new(
            (stroke.width * 0.78).max(1.0),
            stroke
                .color
                .gamma_multiply(if sloppiness == Sloppiness::Artist {
                    0.62
                } else {
                    0.72
                }),
        );
        paint_styled_polyline(painter, &secondary, secondary_stroke, stroke_style, close);
    }
}

fn sloppy_polyline(
    points: &[Pos2],
    sloppiness: Sloppiness,
    seed: u128,
    stroke_width: f32,
    close: bool,
) -> Vec<Pos2> {
    sloppy_polyline_scaled(points, sloppiness, seed, stroke_width, close, 1.0)
}

fn sloppy_polyline_scaled(
    points: &[Pos2],
    sloppiness: Sloppiness,
    seed: u128,
    stroke_width: f32,
    close: bool,
    deformation_scale: f32,
) -> Vec<Pos2> {
    if sloppiness == Sloppiness::Architect || points.len() < 2 {
        return points.to_vec();
    }
    if close && points.len() > 16 {
        return sloppy_closed_curve(points, sloppiness, seed, stroke_width, deformation_scale);
    }
    let edge_count = if close {
        points.len()
    } else {
        points.len() - 1
    };
    let subdivisions = if sloppiness == Sloppiness::Artist {
        8
    } else {
        12
    };
    let amplitude = sloppiness_amplitude(sloppiness, stroke_width, deformation_scale);
    let mut result = Vec::with_capacity(edge_count * subdivisions + 1);
    for edge in 0..edge_count {
        let Some(start) = points.get(edge).copied() else {
            continue;
        };
        let Some(end) = points.get((edge + 1) % points.len()).copied() else {
            continue;
        };
        let direction = end - start;
        let length = direction.length();
        let normal = if length > f32::EPSILON {
            Vec2::new(-direction.y / length, direction.x / length)
        } else {
            Vec2::ZERO
        };
        for step in 0..=subdivisions {
            if edge > 0 && step == 0 {
                continue;
            }
            if close && edge == edge_count - 1 && step == subdivisions {
                continue;
            }
            let step_value = f32::from(u16::try_from(step).unwrap_or(u16::MAX));
            let subdivision_value = f32::from(u16::try_from(subdivisions).unwrap_or(u16::MAX));
            let t = step_value / subdivision_value;
            let point = start + direction * t;
            let envelope = (t * std::f32::consts::PI).sin().max(0.0);
            let start_noise = stable_noise(seed, edge, 0);
            let end_noise = stable_noise(seed, edge + 1, 0);
            let detail_noise = stable_noise(seed, edge, 1);
            let edge_noise = stable_noise(seed, edge, 2) * 0.7 + stable_noise(seed, edge, 3) * 0.3;
            let edge_noise = if edge_noise.abs() >= 0.18 {
                edge_noise
            } else if edge % 2 == 0 {
                0.24
            } else {
                -0.24
            };
            let broad_noise = start_noise + (end_noise - start_noise) * t;
            let detail = detail_noise * (t * std::f32::consts::PI).sin();
            let noise = if sloppiness == Sloppiness::Artist {
                broad_noise * 0.55 + detail * 0.1 + edge_noise * 0.35
            } else {
                let wave = (t * std::f32::consts::TAU).sin() * stable_noise(seed, edge, 4);
                broad_noise * 0.4 + detail * 0.25 + edge_noise * 0.35 + wave * 0.15
            };
            result.push(point + normal * noise * amplitude * envelope);
        }
    }
    result
}

fn sloppy_closed_curve(
    points: &[Pos2],
    sloppiness: Sloppiness,
    seed: u128,
    stroke_width: f32,
    deformation_scale: f32,
) -> Vec<Pos2> {
    let point_count = if points
        .first()
        .zip(points.last())
        .is_some_and(|(first, last)| first.distance(*last) < f32::EPSILON)
    {
        points.len() - 1
    } else {
        points.len()
    };
    if point_count < 3 {
        return points.to_vec();
    }

    let center = points
        .iter()
        .take(point_count)
        .copied()
        .fold(Vec2::ZERO, |sum, point| sum + point.to_vec2())
        / f32::from(u16::try_from(point_count).unwrap_or(u16::MAX));
    let center = Pos2::new(center.x, center.y);
    let control_count = if sloppiness == Sloppiness::Artist {
        9
    } else {
        13
    };
    let amplitude = sloppiness_amplitude(sloppiness, stroke_width, deformation_scale);
    let controls: Vec<f32> = (0..control_count)
        .map(|index| stable_noise(seed, index, 1))
        .collect();
    let point_count_value = f32::from(u16::try_from(point_count).unwrap_or(u16::MAX));
    let mut result = Vec::with_capacity(point_count);

    for (index, point) in points.iter().take(point_count).enumerate() {
        let scaled_index = index.saturating_mul(control_count);
        let left_index = (scaled_index / point_count) % control_count;
        let right_index = (left_index + 1) % control_count;
        let remainder = scaled_index % point_count;
        let local = f32::from(u16::try_from(remainder).unwrap_or(u16::MAX)) / point_count_value;
        let smooth_local = local * local * (3.0 - 2.0 * local);
        let left_noise = controls.get(left_index).copied().unwrap_or(0.0);
        let right_noise = controls.get(right_index).copied().unwrap_or(0.0);
        let noise = left_noise + (right_noise - left_noise) * smooth_local;
        let radial = *point - center;
        let radial_length = radial.length();
        let direction = if radial_length > f32::EPSILON {
            radial / radial_length
        } else {
            Vec2::ZERO
        };
        result.push(*point + direction * noise * amplitude);
    }
    result
}

fn sloppiness_amplitude(sloppiness: Sloppiness, stroke_width: f32, deformation_scale: f32) -> f32 {
    let base = match sloppiness {
        Sloppiness::Architect => 0.0,
        Sloppiness::Artist => 4.2 + stroke_width * 0.18,
        Sloppiness::Cartoonist => 8.0 + stroke_width * 0.28,
    };
    base * deformation_scale
}

fn stable_noise(seed: u128, edge: usize, step: usize) -> f32 {
    let seed_value = u64::try_from(seed & u128::from(u64::MAX)).unwrap_or(0);
    let edge_value = u64::try_from(edge).unwrap_or(u64::MAX);
    let step_value = u64::try_from(step).unwrap_or(u64::MAX);
    let mut value = seed_value
        .wrapping_add(edge_value.wrapping_mul(0x9E37_79B9))
        .wrapping_add(step_value.wrapping_mul(0x85EB_CA6B));
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    let high_bits = u16::try_from(value >> 48).unwrap_or(u16::MAX);
    (f32::from(high_bits) / f32::from(u16::MAX)) * 2.0 - 1.0
}

fn rounded_rect_points(rect: Rect, corner_radius: u8) -> Vec<Pos2> {
    if corner_radius == 0 {
        return vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    let radius = f32::from(corner_radius)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    vec![
        Pos2::new(rect.left() + radius, rect.top()),
        Pos2::new(rect.right() - radius, rect.top()),
        Pos2::new(rect.right(), rect.top() + radius),
        Pos2::new(rect.right(), rect.bottom() - radius),
        Pos2::new(rect.right() - radius, rect.bottom()),
        Pos2::new(rect.left() + radius, rect.bottom()),
        Pos2::new(rect.left(), rect.bottom() - radius),
        Pos2::new(rect.left(), rect.top() + radius),
    ]
}

fn ellipse_points(rect: Rect) -> Vec<Pos2> {
    let center = rect.center();
    let radius = rect.size() / 2.0;
    (0..=48)
        .map(|index| {
            let angle =
                f32::from(u16::try_from(index).unwrap_or(u16::MAX)) / 48.0 * std::f32::consts::TAU;
            Pos2::new(
                center.x + radius.x * angle.cos(),
                center.y + radius.y * angle.sin(),
            )
        })
        .collect()
}

fn diamond_points(rect: Rect, rotation: f32) -> Vec<Pos2> {
    let center = rect.center();
    let points = vec![
        Pos2::new(center.x, rect.top()),
        Pos2::new(rect.right(), center.y),
        Pos2::new(center.x, rect.bottom()),
        Pos2::new(rect.left(), center.y),
    ];
    rotated_screen_points(points, center, rotation)
}

fn triangle_points(rect: Rect, rotation: f32) -> Vec<Pos2> {
    let center = rect.center();
    let points = vec![
        Pos2::new(center.x, rect.top()),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    rotated_screen_points(points, center, rotation)
}

fn rotated_screen_points(points: Vec<Pos2>, center: Pos2, angle: f32) -> Vec<Pos2> {
    points
        .into_iter()
        .map(|point| {
            let offset = point - center;
            let sin = angle.sin();
            let cos = angle.cos();
            center
                + Vec2::new(
                    offset.x * cos - offset.y * sin,
                    offset.x * sin + offset.y * cos,
                )
        })
        .collect()
}

fn rotate_screen_point(point: Pos2, center: Pos2, angle: f32) -> Pos2 {
    let offset = point - center;
    let sin = angle.sin();
    let cos = angle.cos();
    center
        + Vec2::new(
            offset.x * cos - offset.y * sin,
            offset.x * sin + offset.y * cos,
        )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn apply_opacity(color: Color32, opacity: f32) -> Color32 {
    let opacity = if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    };
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (f32::from(color.a()) * opacity).round().clamp(0.0, 255.0) as u8,
    )
}

fn paint_selection(painter: &Painter, elements: &[&Element], camera: Camera, dark_mode: bool) {
    const HANDLES: [SelectionHandle; 8] = [
        SelectionHandle::TopLeft,
        SelectionHandle::Top,
        SelectionHandle::TopRight,
        SelectionHandle::Right,
        SelectionHandle::BottomRight,
        SelectionHandle::Bottom,
        SelectionHandle::BottomLeft,
        SelectionHandle::Left,
    ];
    let Some(bounds) = selection_bounds(elements.iter().copied()) else {
        return;
    };
    let single = elements.first().copied().filter(|_| elements.len() == 1);
    let selection_padding = selection_padding_world(camera.zoom());

    if elements.len() > 1 {
        let individual_stroke = Stroke::new(
            SELECTION_STROKE_WIDTH,
            Color32::from_rgba_unmultiplied(91, 87, 214, 180),
        );
        for element in elements {
            let corners = padded_selection_corners(element, selection_padding)
                .into_iter()
                .map(|point| {
                    let screen = camera.world_to_screen(point);
                    Pos2::new(screen.x, screen.y)
                })
                .collect::<Vec<_>>();
            paint_closed_selection_outline(painter, &corners, individual_stroke, false);
        }
    }

    if let Some(element) = single {
        let corners = padded_selection_corners(element, selection_padding)
            .into_iter()
            .map(|point| {
                let screen = camera.world_to_screen(point);
                Pos2::new(screen.x, screen.y)
            })
            .collect::<Vec<_>>();
        paint_closed_selection_outline(
            painter,
            &corners,
            Stroke::new(SELECTION_STROKE_WIDTH, ACCENT),
            false,
        );
    } else {
        let rect = world_rect_to_screen(padded_selection_bounds(bounds, camera.zoom()), camera);
        let corners = rounded_rect_points(rect, 0);
        paint_closed_selection_outline(
            painter,
            &corners,
            Stroke::new(SELECTION_STROKE_WIDTH, ACCENT),
            true,
        );
    }
    if elements.len() == 1 {
        let Some(selected) = elements.first().copied() else {
            return;
        };
        let handle_fill = selection_handle_fill(dark_mode);
        let rotation_world =
            padded_selection_rotation_handle_position(selected, 28.0, selection_padding);
        let top_world =
            padded_selection_handle_position(selected, SelectionHandle::Top, selection_padding);
        let rotation_screen = camera.world_to_screen(rotation_world);
        let top_screen = camera.world_to_screen(top_world);
        let rotation = Pos2::new(rotation_screen.x, rotation_screen.y);
        let top = Pos2::new(top_screen.x, top_screen.y);
        painter.line_segment([top, rotation], Stroke::new(SELECTION_STROKE_WIDTH, ACCENT));
        for handle in HANDLES {
            let world = padded_selection_handle_position(selected, handle, selection_padding);
            let screen = camera.world_to_screen(world);
            let center = if handle == SelectionHandle::Top {
                top
            } else {
                Pos2::new(screen.x, screen.y)
            };
            paint_selection_handle(painter, center, handle_fill);
        }
        painter.circle_filled(rotation, 5.0, Color32::WHITE);
        painter.circle_stroke(rotation, 5.0, Stroke::new(SELECTION_STROKE_WIDTH, ACCENT));
    } else {
        let handle_fill = selection_handle_fill(dark_mode);
        let handle_bounds = padded_selection_bounds(bounds, camera.zoom());
        for handle in HANDLES {
            let world = crate::selection::handle_position(handle_bounds, handle);
            let screen = camera.world_to_screen(world);
            paint_selection_handle(painter, Pos2::new(screen.x, screen.y), handle_fill);
        }
    }
}

fn selection_handle_fill(dark_mode: bool) -> Color32 {
    if dark_mode { DARK_CANVAS } else { LIGHT_CANVAS }
}

fn paint_selection_handle(painter: &Painter, center: Pos2, fill: Color32) {
    const HANDLE_SIZE: f32 = 10.0;
    const HANDLE_RADIUS: u8 = 2;
    let rect = Rect::from_center_size(center, Vec2::splat(HANDLE_SIZE));
    painter.rect_filled(rect, CornerRadius::same(HANDLE_RADIUS), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(HANDLE_RADIUS),
        Stroke::new(1.25_f32, ACCENT),
        StrokeKind::Inside,
    );
}

fn paint_hover_outline(painter: &Painter, element: &Element, camera: Camera, dark_mode: bool) {
    let padding = 3.0 / normalized_zoom(camera.zoom());
    let corners = padded_selection_corners(element, padding)
        .into_iter()
        .map(|point| {
            let screen = camera.world_to_screen(point);
            Pos2::new(screen.x, screen.y)
        })
        .collect::<Vec<_>>();
    let color = if dark_mode {
        Color32::from_rgba_unmultiplied(145, 120, 242, 190)
    } else {
        Color32::from_rgba_unmultiplied(91, 87, 214, 170)
    };
    paint_closed_selection_outline(painter, &corners, Stroke::new(1.0_f32, color), true);
}

fn paint_closed_selection_outline(
    painter: &Painter,
    corners: &[Pos2],
    stroke: Stroke,
    dotted: bool,
) {
    if corners.len() < 2 {
        return;
    }
    let mut outline = corners.to_vec();
    if let Some(first) = corners.first().copied() {
        outline.push(first);
    }
    paint_styled_polyline(
        painter,
        &outline,
        stroke,
        if dotted {
            StrokeStyle::Dotted
        } else {
            StrokeStyle::Solid
        },
        false,
    );
}

fn paint_marquee(painter: &Painter, start: Point, current: Point, camera: Camera) {
    let rect = world_rect_to_screen(marquee_rect(start, current), camera);
    painter.rect_filled(
        rect,
        CornerRadius::same(0),
        Color32::from_rgba_unmultiplied(91, 87, 214, 28),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(0),
        Stroke::new(1.0_f32, ACCENT),
        StrokeKind::Inside,
    );
}

fn world_rect_to_screen(rect: canvas_core::Rect, camera: Camera) -> Rect {
    let min = camera.world_to_screen(rect.min);
    let max = camera.world_to_screen(rect.max());
    Rect::from_min_max(
        Pos2::new(min.x.min(max.x), min.y.min(max.y)),
        Pos2::new(min.x.max(max.x), min.y.max(max.y)),
    )
}

fn padded_selection_bounds(bounds: canvas_core::Rect, zoom: f32) -> canvas_core::Rect {
    let padding = selection_padding_world(zoom);
    canvas_core::Rect::new(
        Point::new(bounds.min.x - padding, bounds.min.y - padding),
        Size::new(
            bounds.size.width + padding * 2.0,
            bounds.size.height + padding * 2.0,
        ),
    )
}

fn selection_padding_world(zoom: f32) -> f32 {
    5.0 / normalized_zoom(zoom)
}

fn resize_pointer_position(pointer: Point, offset: Point) -> Point {
    Point::new(pointer.x + offset.x, pointer.y + offset.y)
}

fn marquee_rect(start: Point, current: Point) -> canvas_core::Rect {
    canvas_core::Rect::new(
        Point::new(start.x.min(current.x), start.y.min(current.y)),
        Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
    )
}

fn element_screen_points(element: &Element, camera: Camera, rect: Rect) -> Vec<Pos2> {
    if element.points.is_empty() {
        return vec![rect.left_top(), rect.right_bottom()];
    }
    let center = Point::new(
        element.transform.position.x + element.transform.size.width / 2.0,
        element.transform.position.y + element.transform.size.height / 2.0,
    );
    element
        .points
        .iter()
        .copied()
        .map(|point| {
            let screen =
                camera.world_to_screen(rotate_around(point, center, element.transform.rotation));
            Pos2::new(screen.x, screen.y)
        })
        .collect()
}

fn paint_arrowhead(painter: &Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let direction = end - start;
    let length = direction.length();
    if length <= f32::EPSILON {
        return;
    }
    let unit = direction / length;
    let side = Vec2::new(-unit.y, unit.x);
    let base = end - unit * 12.0;
    painter.line_segment([end, base + side * 5.0], stroke);
    painter.line_segment([end, base - side * 5.0], stroke);
}

fn screen_to_world(camera: Camera, position: Pos2) -> Point {
    camera.screen_to_world(Point::new(position.x, position.y))
}

#[cfg(test)]
fn selection_drag_position(origin: Point, pointer_start: Point, pointer_current: Point) -> Point {
    Point::new(
        origin.x + pointer_current.x - pointer_start.x,
        origin.y + pointer_current.y - pointer_start.y,
    )
}

fn selection_tolerance(zoom: f32) -> f32 {
    let zoom = if zoom.is_finite() {
        zoom.clamp(0.05, 64.0)
    } else {
        1.0
    };
    8.0 / zoom
}

fn normalized_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(0.05, 64.0)
    } else {
        1.0
    }
}

/// Returns the strict screen-space half-size of the painted resize handle.
///
/// The cursor deliberately uses the inside of the visible node. This prevents
/// the resize cursor from appearing while the pointer is merely near a node.
fn selection_handle_cursor_tolerance(zoom: f32) -> f32 {
    4.5 / normalized_zoom(zoom)
}

/// Returns a forgiving screen-space resize-start radius.
///
/// A mouse-down can move by a pixel between frames, so starting a resize gets
/// a small target larger than the cursor target. The cursor itself remains
/// strict and therefore does not advertise resize outside the painted node.
fn selection_handle_drag_tolerance(zoom: f32) -> f32 {
    7.0 / normalized_zoom(zoom)
}

fn rotation_handle_cursor_tolerance(zoom: f32) -> f32 {
    4.5 / normalized_zoom(zoom)
}

fn rotation_handle_drag_tolerance(zoom: f32) -> f32 {
    7.0 / normalized_zoom(zoom)
}

fn canvas_cursor(tool: Tool, dragging: bool) -> CursorIcon {
    match tool {
        Tool::Pan if dragging => CursorIcon::Grabbing,
        Tool::Pan => CursorIcon::Grab,
        Tool::Select => CursorIcon::Default,
        Tool::Text
        | Tool::Rectangle
        | Tool::Diamond
        | Tool::Triangle
        | Tool::Ellipse
        | Tool::Line
        | Tool::Arrow
        | Tool::Freehand => CursorIcon::Crosshair,
    }
}

fn resize_cursor(handle: SelectionHandle) -> CursorIcon {
    match handle {
        SelectionHandle::TopLeft | SelectionHandle::BottomRight => CursorIcon::ResizeNwSe,
        SelectionHandle::TopRight | SelectionHandle::BottomLeft => CursorIcon::ResizeNeSw,
        SelectionHandle::Top | SelectionHandle::Bottom => CursorIcon::ResizeVertical,
        SelectionHandle::Left | SelectionHandle::Right => CursorIcon::ResizeHorizontal,
    }
}

fn apply_draft_style(command: EditorCommand, style: Style) -> EditorCommand {
    match command {
        EditorCommand::Create(mut element) => {
            element.style = style;
            EditorCommand::Create(element)
        }
        command => command,
    }
}

fn fill_choice_patch(current: Option<Color>, solid: bool) -> StylePatch {
    StylePatch {
        fill: Some(if solid {
            Some(current.unwrap_or(Color::rgb(221, 214, 254)))
        } else {
            None
        }),
        ..StylePatch::default()
    }
}

fn color_picker_patch(target: ColorPickerTarget, color: Color32) -> StylePatch {
    match target {
        ColorPickerTarget::Stroke => StylePatch {
            stroke: Some(to_core_color(color)),
            ..StylePatch::default()
        },
        ColorPickerTarget::Fill => StylePatch {
            fill: Some(if color.a() == 0 {
                None
            } else {
                Some(to_core_color(color))
            }),
            ..StylePatch::default()
        },
    }
}

fn to_color32(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.red, color.green, color.blue, color.alpha)
}

fn to_core_color(color: Color32) -> Color {
    Color::rgba(color.r(), color.g(), color.b(), color.a())
}

fn text_color(dark_mode: bool) -> Color32 {
    if dark_mode { DARK_TEXT } else { LIGHT_TEXT }
}

fn muted_color(dark_mode: bool) -> Color32 {
    if dark_mode { DARK_MUTED } else { LIGHT_MUTED }
}

fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Select",
        Tool::Text => "Text",
        Tool::Rectangle => "Rectangle",
        Tool::Diamond => "Diamond",
        Tool::Triangle => "Triangle",
        Tool::Ellipse => "Ellipse",
        Tool::Line => "Line",
        Tool::Arrow => "Arrow",
        Tool::Freehand => "Freehand",
        Tool::Pan => "Pan",
    }
}

fn preset_font_size_selected(
    custom_state: CustomFontSizeState,
    current_size: f32,
    preset_size: f32,
) -> bool {
    custom_state == CustomFontSizeState::Closed && (current_size - preset_size).abs() < f32::EPSILON
}

fn custom_font_size_selected(custom_state: CustomFontSizeState, has_preset_size: bool) -> bool {
    custom_state == CustomFontSizeState::Open || !has_preset_size
}

#[cfg(test)]
fn text_create_command(
    id: ElementId,
    position: Point,
    text: &str,
    style: Style,
) -> Option<EditorCommand> {
    let line_count = f32::from(u16::try_from(text.lines().count().max(1)).unwrap_or(u16::MAX));
    let longest_line = f32::from(
        u16::try_from(
            text.lines()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(1),
        )
        .unwrap_or(u16::MAX),
    );
    let size = Size::new(
        (longest_line * 9.0 + 16.0).max(40.0),
        (line_count * 24.0 + 8.0).max(28.0),
    );
    text_create_command_with_size(id, position, text, style, size)
}

fn text_create_command_with_size(
    id: ElementId,
    position: Point,
    text: &str,
    style: Style,
    size: Size,
) -> Option<EditorCommand> {
    if text.trim().is_empty() {
        return None;
    }
    let mut element = Element::text(id, Transform::new(position, size), text);
    element.style = style;
    Some(EditorCommand::Create(element))
}

fn image_display_size(width: u32, height: u32) -> Size {
    let width = f32::from(u16::try_from(width).unwrap_or(u16::MAX)).max(1.0);
    let height = f32::from(u16::try_from(height).unwrap_or(u16::MAX)).max(1.0);
    let scale = (640.0 / width).min(480.0 / height).min(1.0);
    Size::new(width * scale, height * scale)
}

fn zoom_percent(zoom: f32) -> String {
    let zoom = if zoom.is_finite() {
        zoom.clamp(0.05, 64.0)
    } else {
        1.0
    };
    format!("{:.0}%", zoom * 100.0)
}

fn zoom_delta_for_scroll(scroll_y: f32) -> f32 {
    (scroll_y * 0.002).clamp(-0.25, 0.25)
}

#[cfg(test)]
mod tests {
    use canvas_core::{
        ClientId, Color, EditorCommand, Element, ElementId, ElementKind, Point, Size, Style,
        StylePatch, Transform,
    };
    use egui::{Color32, Key, Modifiers, Pos2, Rect, Vec2};

    use crate::editor::Editor;

    use super::{
        ColorPickerTarget, CustomFontSizeState, KeyBinding, KeybindAction, Keybinds, LayerAction,
        WorkspaceUi, color_picker_patch, custom_font_size_selected, fill_choice_patch,
        grid_step_for_zoom, next_z_index, padded_selection_bounds, preset_font_size_selected,
        reordered_layer_ids, rotated_text_origin, selection_drag_position,
        selection_handle_cursor_tolerance, selection_handle_drag_tolerance, sloppiness_amplitude,
        sloppy_polyline, text_create_command, text_update_command, zoom_delta_for_scroll,
        zoom_percent,
    };

    #[test]
    fn rotated_text_origin_uses_the_element_center() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(100.0, 20.0));
        let rotated = rotated_text_origin(rect.left_top(), rect, std::f32::consts::FRAC_PI_2);

        assert!((rotated.x - 70.0).abs() < 1e-5);
        assert!((rotated.y + 20.0).abs() < 1e-5);
    }

    #[test]
    fn default_keybinds_cover_all_toolbar_and_editing_actions() {
        let keybinds = Keybinds::default();

        assert_eq!(KeybindAction::ALL.len(), 17);
        assert_eq!(
            keybinds.binding(KeybindAction::SelectAll),
            KeyBinding {
                key: Key::A,
                modifiers: Modifiers::CTRL,
            }
        );
        assert_eq!(
            keybinds.binding(KeybindAction::Delete),
            KeyBinding {
                key: Key::Backspace,
                modifiers: Modifiers::NONE,
            }
        );
        assert_eq!(
            KeybindAction::Rectangle.tool(),
            Some(super::Tool::Rectangle)
        );
        assert_eq!(KeybindAction::Settings.tool(), None);
    }

    #[test]
    fn scroll_zoom_delta_is_directional_and_bounded() {
        assert!((zoom_delta_for_scroll(40.0) - 0.08).abs() < f32::EPSILON);
        assert!((zoom_delta_for_scroll(-40.0) + 0.08).abs() < f32::EPSILON);
        assert!((zoom_delta_for_scroll(10_000.0) - 0.25).abs() < f32::EPSILON);
        assert!((zoom_delta_for_scroll(-10_000.0) + 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn custom_font_size_does_not_switch_to_a_preset_while_open() {
        assert!(!preset_font_size_selected(
            CustomFontSizeState::Open,
            24.0,
            24.0
        ));
        assert!(custom_font_size_selected(CustomFontSizeState::Open, true));
        assert!(preset_font_size_selected(
            CustomFontSizeState::Closed,
            24.0,
            24.0
        ));
        assert!(custom_font_size_selected(
            CustomFontSizeState::Closed,
            false
        ));
    }

    #[test]
    fn solid_fill_choice_assigns_a_default_without_losing_an_existing_color() {
        let default_fill = fill_choice_patch(None, true);
        let existing_fill = fill_choice_patch(Some(Color::rgb(1, 2, 3)), true);
        let clear_fill = fill_choice_patch(Some(Color::rgb(1, 2, 3)), false);

        assert_eq!(
            default_fill,
            StylePatch {
                fill: Some(Some(Color::rgb(221, 214, 254))),
                ..StylePatch::default()
            }
        );
        assert_eq!(
            existing_fill,
            StylePatch {
                fill: Some(Some(Color::rgb(1, 2, 3))),
                ..StylePatch::default()
            }
        );
        assert_eq!(
            clear_fill,
            StylePatch {
                fill: Some(None),
                ..StylePatch::default()
            }
        );
    }

    #[test]
    fn dot_grid_keeps_a_readable_screen_spacing_when_zoom_changes() {
        let normal = grid_step_for_zoom(1.0);
        let zoomed_out = grid_step_for_zoom(0.05);
        let zoomed_in = grid_step_for_zoom(64.0);

        assert!((normal - 32.0).abs() < f32::EPSILON);
        assert!((zoomed_out * 0.05 - 25.6).abs() < f32::EPSILON);
        assert!((zoomed_in * 64.0 - 32.0).abs() < f32::EPSILON);
    }

    #[test]
    fn default_element_stroke_remains_two_pixels() {
        assert!((Style::default().stroke_width - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn group_selection_padding_stays_five_screen_pixels_wide() {
        let padded = padded_selection_bounds(
            canvas_core::Rect::new(Point::new(10.0, 20.0), canvas_core::Size::new(40.0, 30.0)),
            2.0,
        );

        assert_eq!(padded.min, Point::new(7.5, 17.5));
        assert_eq!(padded.size, canvas_core::Size::new(45.0, 35.0));
    }

    #[test]
    fn zoom_percent_is_clamped_for_toolbar_display() {
        assert_eq!(zoom_percent(0.05), "5%");
        assert_eq!(zoom_percent(1.0), "100%");
        assert_eq!(zoom_percent(64.0), "6400%");
        assert_eq!(zoom_percent(f32::NAN), "100%");
    }

    #[test]
    fn layer_actions_move_a_selected_group_by_one_layer() {
        let ordered = (1..=4).map(ElementId::from_u128).collect::<Vec<_>>();
        let selected = [ElementId::from_u128(2), ElementId::from_u128(3)]
            .into_iter()
            .collect();

        assert_eq!(
            reordered_layer_ids(&ordered, &selected, LayerAction::SendBackward),
            [2, 3, 1, 4]
                .into_iter()
                .map(ElementId::from_u128)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            reordered_layer_ids(&ordered, &selected, LayerAction::BringForward),
            [1, 4, 2, 3]
                .into_iter()
                .map(ElementId::from_u128)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            reordered_layer_ids(&ordered, &selected, LayerAction::SendToBack),
            [2, 3, 1, 4]
                .into_iter()
                .map(ElementId::from_u128)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            reordered_layer_ids(&ordered, &selected, LayerAction::BringToFront),
            [1, 4, 2, 3]
                .into_iter()
                .map(ElementId::from_u128)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn selection_drag_preserves_the_pointer_offset() {
        assert_eq!(
            selection_drag_position(
                Point::new(100.0, 80.0),
                Point::new(20.0, 15.0),
                Point::new(45.0, 55.0),
            ),
            Point::new(125.0, 120.0)
        );
    }

    #[test]
    fn resize_cursor_is_stricter_than_resize_start_hit_target() {
        let cursor = selection_handle_cursor_tolerance(2.0);
        let drag = selection_handle_drag_tolerance(2.0);

        assert!((cursor - 2.25).abs() < f32::EPSILON);
        assert!((drag - 3.5).abs() < f32::EPSILON);
        assert!(cursor < drag);
    }

    #[test]
    fn text_editor_commits_one_text_create_command() {
        let command = text_create_command(
            ElementId::from_u128(42),
            Point::new(10.0, 20.0),
            "Hello Sketchi",
            Style::default(),
        );
        assert!(matches!(
            &command,
            Some(canvas_core::EditorCommand::Create(element))
                if element.kind == ElementKind::Text
                    && element.text == "Hello Sketchi"
                    && element.transform.position == Point::new(10.0, 20.0)
                    && element.transform.size.width >= 40.0
        ));
    }

    #[test]
    fn text_editor_updates_existing_text_with_a_set_text_command() {
        let command = text_update_command(ElementId::from_u128(44), "Updated text");
        assert!(matches!(
            command,
            Some(canvas_core::EditorCommand::SetText(id, text))
                if id == ElementId::from_u128(44) && text == "Updated text"
        ));
        assert!(text_update_command(ElementId::from_u128(44), "  \n").is_none());
    }

    #[test]
    fn color_picker_transparent_choice_maps_stroke_and_fill_correctly() {
        let stroke = color_picker_patch(ColorPickerTarget::Stroke, Color32::TRANSPARENT);
        assert_eq!(stroke.stroke, Some(Color::rgba(0, 0, 0, 0)));
        assert_eq!(stroke.fill, None);

        let fill = color_picker_patch(ColorPickerTarget::Fill, Color32::TRANSPARENT);
        assert_eq!(fill.stroke, None);
        assert_eq!(fill.fill, Some(None));
    }

    #[test]
    fn new_elements_are_assigned_above_existing_layers() {
        let mut editor = Editor::new(ClientId::from_u128(45));
        let mut existing = Element::rectangle(
            ElementId::from_u128(46),
            Transform::new(Point::default(), Size::new(20.0, 20.0)),
        );
        existing.z_index = 7;
        assert!(editor.execute(EditorCommand::Create(existing)).is_ok());

        assert_eq!(next_z_index(&editor.document()), 8);
    }

    #[test]
    fn blank_text_is_not_committed() {
        assert!(
            text_create_command(
                ElementId::from_u128(43),
                Point::default(),
                "  \n",
                Style::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn sloppiness_levels_have_a_clear_visual_progression() {
        let architect = sloppiness_amplitude(canvas_core::Sloppiness::Architect, 2.0, 1.0);
        let artist = sloppiness_amplitude(canvas_core::Sloppiness::Artist, 2.0, 1.0);
        let cartoonist = sloppiness_amplitude(canvas_core::Sloppiness::Cartoonist, 2.0, 1.0);

        assert!(architect.abs() < f32::EPSILON);
        assert!(artist > 3.0);
        assert!(cartoonist > artist * 1.7);
    }

    #[test]
    fn sloppy_polygon_keeps_corners_anchored() {
        let corners = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(100.0, 80.0),
            Pos2::new(0.0, 80.0),
        ];
        let architect =
            sloppy_polyline(&corners, canvas_core::Sloppiness::Architect, 42, 2.0, true);
        let artist = sloppy_polyline(&corners, canvas_core::Sloppiness::Artist, 42, 2.0, true);
        let cartoonist =
            sloppy_polyline(&corners, canvas_core::Sloppiness::Cartoonist, 42, 2.0, true);

        assert_eq!(architect, corners);
        assert_ne!(artist, corners);
        assert_ne!(cartoonist, corners);
        assert_ne!(artist, cartoonist);
        assert_eq!(cartoonist.first().copied(), corners.first().copied());
        assert_eq!(cartoonist.get(12).copied(), corners.get(1).copied());
        assert_eq!(cartoonist.get(24).copied(), corners.get(2).copied());
        assert_eq!(cartoonist.get(36).copied(), corners.get(3).copied());
        assert!(
            artist
                .get(20)
                .zip(corners.get(2))
                .is_some_and(|(point, corner)| (point.y - corner.y).abs() > 0.25)
        );
        assert!(
            cartoonist
                .get(30)
                .zip(corners.get(2))
                .is_some_and(|(point, corner)| (point.y - corner.y).abs() > 0.5)
        );
    }

    #[test]
    fn queued_drop_files_clear_the_drop_target_and_preserve_order() {
        let mut ui = WorkspaceUi::default();
        ui.queue_dropped_file(std::path::PathBuf::from("first.png"));
        ui.queue_dropped_file(std::path::PathBuf::from("second.jpg"));

        assert!(ui.drop_hovered.is_none());
        assert_eq!(
            ui.pending_dropped_files,
            vec![
                std::path::PathBuf::from("first.png"),
                std::path::PathBuf::from("second.jpg")
            ]
        );
    }

    #[test]
    fn hovered_drop_keeps_a_preview_until_the_drag_is_cancelled() {
        let mut ui = WorkspaceUi::default();
        ui.set_drop_preview(std::path::PathBuf::from("preview.png"));

        assert!(ui.drop_hovered.is_some());
        assert_eq!(
            ui.drop_preview
                .as_ref()
                .map(|preview| preview.path.as_path()),
            Some(std::path::Path::new("preview.png"))
        );

        ui.set_drop_hovered(false);
        assert!(ui.drop_preview.is_none());
    }
}
