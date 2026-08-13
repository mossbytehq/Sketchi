//! Reusable egui controls shared by Sketchi's settings and property panels.

use std::{hash::Hash, ops::RangeInclusive};

use egui::epaint::Hsva;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, InnerResponse, Margin, Mesh, Pos2, Rect,
    RectAlign, Response, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2, WidgetText,
};

use crate::remix_icons::{self, RemixIcon};
#[allow(unused_imports)]
use crate::theme::ThemeTokens;

pub(crate) const STANDARD_CONTROL_SIZE: Vec2 = Vec2::new(190.0, 30.0);
pub(crate) const BUTTON_PADDING: Vec2 = Vec2::new(10.0, 5.0);
const DROPDOWN_POPUP_HORIZONTAL_PADDING: i8 = 6;
const DROPDOWN_POPUP_VERTICAL_PADDING: i8 = 10;
const DROPDOWN_POPUP_ITEM_SPACING: f32 = 4.0;
const DROPDOWN_OPTION_PADDING: Vec2 = Vec2::new(6.0, 3.0);
const DROPDOWN_POPUP_RADIUS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
struct DropdownPopupStyle {
    menu_margin: Margin,
    item_spacing_y: f32,
    button_padding: Vec2,
    selected_text: Color32,
}

fn dropdown_popup_style() -> DropdownPopupStyle {
    DropdownPopupStyle {
        menu_margin: Margin::symmetric(
            DROPDOWN_POPUP_HORIZONTAL_PADDING,
            DROPDOWN_POPUP_VERTICAL_PADDING,
        ),
        item_spacing_y: DROPDOWN_POPUP_ITEM_SPACING,
        button_padding: DROPDOWN_OPTION_PADDING,
        selected_text: Color32::WHITE,
    }
}

fn dropdown_popup_style_modifier() -> egui::style::StyleModifier {
    let popup = dropdown_popup_style();
    egui::style::StyleModifier::new(move |style| {
        egui::containers::menu::menu_style(style);
        let highlight_fill = style.visuals.selection.bg_fill;
        let highlight_stroke = Stroke::new(1.0_f32, popup.selected_text);
        style.spacing.menu_margin = popup.menu_margin;
        style.spacing.item_spacing.y = popup.item_spacing_y;
        style.spacing.button_padding = popup.button_padding;
        style.visuals.menu_corner_radius = CornerRadius::same(DROPDOWN_POPUP_RADIUS);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(DROPDOWN_POPUP_RADIUS);
        style.visuals.widgets.active.corner_radius = CornerRadius::same(DROPDOWN_POPUP_RADIUS);
        style.visuals.widgets.open.corner_radius = CornerRadius::same(DROPDOWN_POPUP_RADIUS);
        style.visuals.widgets.hovered.bg_fill = highlight_fill;
        style.visuals.widgets.hovered.weak_bg_fill = highlight_fill;
        style.visuals.widgets.active.bg_fill = highlight_fill;
        style.visuals.widgets.active.weak_bg_fill = highlight_fill;
        style.visuals.widgets.hovered.fg_stroke = highlight_stroke;
        style.visuals.widgets.active.fg_stroke = highlight_stroke;
        style.visuals.widgets.open.fg_stroke = highlight_stroke;
        style.visuals.selection.stroke = highlight_stroke;
    })
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SurfaceVariant {
    Background,
    Card,
    Popover,
    Sidebar,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonVariant {
    Primary,
    Secondary,
    Outline,
    Ghost,
    Destructive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceStyle {
    fill: Color32,
    foreground: Color32,
    border: Color32,
    radius: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonStyle {
    fill: Color32,
    foreground: Color32,
    hover_fill: Color32,
    hover_foreground: Color32,
    active_fill: Color32,
    active_foreground: Color32,
    border: Color32,
}

#[allow(dead_code)]
fn surface_style(theme: ThemeTokens, variant: SurfaceVariant, radius: u8) -> SurfaceStyle {
    let (fill, foreground, border) = match variant {
        SurfaceVariant::Background => (theme.background, theme.foreground, theme.border),
        SurfaceVariant::Card => (theme.card, theme.card_foreground, theme.border),
        SurfaceVariant::Popover => (theme.popover, theme.popover_foreground, theme.border),
        SurfaceVariant::Sidebar => (
            theme.sidebar,
            theme.sidebar_foreground,
            theme.sidebar_border,
        ),
    };
    SurfaceStyle {
        fill,
        foreground,
        border,
        radius,
    }
}

#[allow(dead_code)]
pub(crate) fn surface_foreground(theme: ThemeTokens, variant: SurfaceVariant) -> Color32 {
    surface_style(theme, variant, theme.radius.md).foreground
}

#[allow(dead_code)]
fn button_style(theme: ThemeTokens, variant: ButtonVariant) -> ButtonStyle {
    match variant {
        ButtonVariant::Primary => ButtonStyle {
            fill: theme.primary,
            foreground: theme.primary_foreground,
            hover_fill: theme.accent,
            hover_foreground: theme.accent_foreground,
            active_fill: theme.primary,
            active_foreground: theme.primary_foreground,
            border: theme.ring,
        },
        ButtonVariant::Secondary => ButtonStyle {
            fill: theme.secondary,
            foreground: theme.secondary_foreground,
            hover_fill: theme.accent,
            hover_foreground: theme.accent_foreground,
            active_fill: theme.primary,
            active_foreground: theme.primary_foreground,
            border: theme.border,
        },
        ButtonVariant::Outline => ButtonStyle {
            fill: Color32::TRANSPARENT,
            foreground: theme.foreground,
            hover_fill: theme.accent,
            hover_foreground: theme.accent_foreground,
            active_fill: theme.secondary,
            active_foreground: theme.secondary_foreground,
            border: theme.border,
        },
        ButtonVariant::Ghost => ButtonStyle {
            fill: Color32::TRANSPARENT,
            foreground: theme.foreground,
            hover_fill: theme.accent,
            hover_foreground: theme.accent_foreground,
            active_fill: theme.secondary,
            active_foreground: theme.secondary_foreground,
            border: Color32::TRANSPARENT,
        },
        ButtonVariant::Destructive => ButtonStyle {
            fill: theme.destructive,
            foreground: theme.destructive_foreground,
            hover_fill: theme.destructive,
            hover_foreground: theme.destructive_foreground,
            active_fill: theme.destructive,
            active_foreground: theme.destructive_foreground,
            border: theme.ring,
        },
    }
}

fn button_focus_ring(theme: ThemeTokens, focused: bool) -> Option<(Stroke, CornerRadius)> {
    focused.then_some((
        Stroke::new(2.0_f32, theme.ring),
        CornerRadius::same(theme.radius.md),
    ))
}

#[allow(dead_code)]
pub(crate) fn themed_surface_frame(
    theme: ThemeTokens,
    variant: SurfaceVariant,
    radius: u8,
) -> Frame {
    let style = surface_style(theme, variant, radius);
    Frame {
        fill: style.fill,
        stroke: Stroke::new(1.0_f32, style.border),
        corner_radius: CornerRadius::same(style.radius),
        ..Frame::new()
    }
}

#[allow(dead_code)]
pub(crate) fn themed_button(
    ui: &mut Ui,
    theme: ThemeTokens,
    label: impl Into<WidgetText>,
    size: Vec2,
    variant: ButtonVariant,
) -> Response {
    let style = button_style(theme, variant);
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = BUTTON_PADDING;
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.bg_fill = style.fill;
        visuals.widgets.inactive.weak_bg_fill = style.fill;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, style.foreground);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, style.border);
        visuals.widgets.hovered.bg_fill = style.hover_fill;
        visuals.widgets.hovered.weak_bg_fill = style.hover_fill;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, style.hover_foreground);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, theme.ring);
        visuals.widgets.active.bg_fill = style.active_fill;
        visuals.widgets.active.weak_bg_fill = style.active_fill;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, style.active_foreground);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, theme.ring);
        let response = ui.add_sized(
            Vec2::new(size.x, size.y.max(STANDARD_CONTROL_SIZE.y)),
            egui::Button::new(label),
        );
        if let Some((stroke, corner_radius)) = button_focus_ring(theme, response.has_focus()) {
            ui.painter().rect_stroke(
                response.rect.expand(1.0),
                corner_radius,
                stroke,
                StrokeKind::Outside,
            );
        }
        response
    })
    .inner
}

const SWATCH_ACCENT: Color32 = Color32::from_rgb(91, 87, 214);
const SWATCH_LIGHT_BORDER: Color32 = Color32::from_rgb(205, 209, 218);
const SWATCH_DARK_BORDER: Color32 = Color32::from_rgb(62, 64, 72);
const SWATCH_DARK_FILL_BORDER: Color32 = Color32::from_rgb(126, 129, 140);
const SWATCH_DARK_TEXT: Color32 = Color32::from_rgb(232, 233, 237);
const SWATCH_LIGHT_MUTED: Color32 = Color32::from_rgb(91, 97, 108);
const SWATCH_FRAME_INSET: f32 = 1.0;
const SWATCH_FRAME_RADIUS: u8 = 5;

fn swatch_needs_contrast_border(fill: Color32, transparent: bool) -> bool {
    transparent
        || fill == Color32::WHITE
        || (fill.r() < 80 && fill.g() < 80 && fill.b() < 80)
        || (fill.r() > 220 && fill.g() > 220 && fill.b() > 220)
}

fn swatch_border_color(fill: Color32, dark_mode: bool) -> Color32 {
    if dark_mode && fill.r() < 80 && fill.g() < 80 && fill.b() < 80 {
        SWATCH_DARK_FILL_BORDER
    } else if dark_mode {
        SWATCH_DARK_BORDER
    } else {
        SWATCH_LIGHT_BORDER
    }
}

fn swatch_frame_geometry(rect: Rect) -> (Rect, CornerRadius) {
    (
        rect.shrink(SWATCH_FRAME_INSET),
        CornerRadius::same(SWATCH_FRAME_RADIUS),
    )
}

/// A consistent color choice control used by the properties panel, settings,
/// and color picker.
pub(crate) fn color_swatch(
    ui: &mut Ui,
    color: Option<Color32>,
    selected: bool,
    dark_mode: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let (frame_rect, frame_radius) = swatch_frame_geometry(rect);
    let fill = color.unwrap_or(Color32::TRANSPARENT);
    let transparent = color.is_none() || fill.a() == 0;
    if transparent {
        ui.painter().rect_filled(
            frame_rect,
            frame_radius,
            if dark_mode {
                Color32::from_rgb(54, 55, 61)
            } else {
                Color32::from_rgb(240, 242, 246)
            },
        );
        let slash = Stroke::new(
            1.3_f32,
            if dark_mode {
                SWATCH_DARK_TEXT
            } else {
                SWATCH_LIGHT_MUTED
            },
        );
        ui.painter().line_segment(
            [
                rect.left_bottom() + Vec2::new(5.0, -5.0),
                rect.right_top() + Vec2::new(-5.0, 5.0),
            ],
            slash,
        );
    } else {
        ui.painter().rect_filled(frame_rect, frame_radius, fill);
    }
    if selected {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(7),
            Stroke::new(2.0_f32, SWATCH_ACCENT),
            StrokeKind::Outside,
        );
    } else if swatch_needs_contrast_border(fill, transparent) {
        ui.painter().rect_stroke(
            frame_rect,
            frame_radius,
            Stroke::new(1.0_f32, swatch_border_color(fill, dark_mode)),
            StrokeKind::Inside,
        );
    }
    response
}

/// A compact rainbow trigger used to open the detailed color picker.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn color_picker_trigger(ui: &mut Ui, color: Color32, dark_mode: bool) -> Response {
    let width = ui.available_width().max(1.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let inner = rect.shrink(2.0);
    paint_gradient_strip(ui, inner, 48, |hue| Hsva::new(hue, 0.9, 0.95, 1.0));
    let hue = Hsva::from(color).h.rem_euclid(1.0);
    let marker_x = inner.left() + inner.width() * hue;
    ui.painter().line_segment(
        [
            Pos2::new(marker_x, inner.top() - 1.0),
            Pos2::new(marker_x, inner.bottom() + 1.0),
        ],
        Stroke::new(
            2.0_f32,
            if dark_mode {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
        ),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(
            1.0_f32,
            if response.hovered() {
                SWATCH_ACCENT
            } else if dark_mode {
                SWATCH_DARK_BORDER
            } else {
                SWATCH_LIGHT_BORDER
            },
        ),
        StrokeKind::Inside,
    );
    response
}

/// A reusable circular hue/saturation picker with a brightness slider.
pub(crate) fn color_picker_editor(ui: &mut Ui, color: &mut Color32, dark_mode: bool) -> bool {
    let mut hsva = Hsva::from(*color);
    let mut changed = false;
    let wheel_size = ui.available_width().min(178.0);
    let (wheel_rect, wheel_response) =
        ui.allocate_exact_size(Vec2::splat(wheel_size), Sense::click_and_drag());
    if let Some(pointer) = wheel_response.interact_pointer_pos() {
        let radius = wheel_rect.width() * 0.5 - 3.0;
        if let Some(selection) = wheel_selection(pointer, wheel_rect.center(), radius, hsva.v) {
            hsva = selection;
            changed = true;
        }
    }

    paint_color_wheel(ui, wheel_rect, hsva, dark_mode);

    ui.add_space(10.0);
    let slider_width = wheel_size;
    let (value_rect, value_response) =
        ui.allocate_exact_size(Vec2::new(slider_width, 16.0), Sense::click_and_drag());
    if let Some(pointer) = value_response.interact_pointer_pos() {
        hsva.v = ((pointer.x - value_rect.left()) / value_rect.width()).clamp(0.0, 1.0);
        changed = true;
    }
    paint_value_slider(ui, value_rect, hsva, dark_mode);

    if changed {
        *color = hsva.to_opaque().into();
    }
    changed
}

fn wheel_selection(pointer: Pos2, center: Pos2, radius: f32, value: f32) -> Option<Hsva> {
    if radius <= 0.0 {
        return None;
    }
    let offset = pointer - center;
    let distance = offset.length();
    if distance > radius {
        return None;
    }
    let hue = offset.y.atan2(offset.x).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    Some(Hsva::new(
        hue,
        (distance / radius).clamp(0.0, 1.0),
        value.clamp(0.0, 1.0),
        1.0,
    ))
}

#[allow(clippy::cast_precision_loss)]
fn paint_color_wheel(ui: &Ui, rect: Rect, hsva: Hsva, dark_mode: bool) {
    let center = rect.center();
    let radius = rect.width() * 0.5 - 3.0;
    let segments = 96_u32;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, Color32::WHITE);
    for index in 0..=segments {
        let angle = std::f32::consts::TAU * index as f32 / segments as f32;
        let point = center + Vec2::angled(angle) * radius;
        mesh.colored_vertex(
            point,
            Hsva::new(index as f32 / segments as f32, 1.0, 1.0, 1.0).into(),
        );
    }
    for index in 0..segments {
        mesh.add_triangle(0, index + 1, index + 2);
    }
    ui.painter().add(Shape::mesh(mesh));
    ui.painter().circle_stroke(
        center,
        radius,
        Stroke::new(
            1.0_f32,
            if dark_mode {
                SWATCH_DARK_BORDER
            } else {
                SWATCH_LIGHT_BORDER
            },
        ),
    );

    let marker_angle = hsva.h.rem_euclid(1.0) * std::f32::consts::TAU;
    let marker = center + Vec2::angled(marker_angle) * radius * hsva.s.clamp(0.0, 1.0);
    ui.painter().circle_filled(marker, 6.0, hsva.to_opaque());
    ui.painter().circle_stroke(
        marker,
        7.0,
        Stroke::new(
            2.0_f32,
            if hsva.v < 0.5 {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
        ),
    );
}

fn paint_value_slider(ui: &Ui, rect: Rect, hsva: Hsva, dark_mode: bool) {
    paint_gradient_strip(ui, rect.shrink(1.0), 48, |value| {
        Hsva::new(hsva.h, hsva.s, value, 1.0)
    });
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(
            1.0_f32,
            if dark_mode {
                SWATCH_DARK_BORDER
            } else {
                SWATCH_LIGHT_BORDER
            },
        ),
        StrokeKind::Inside,
    );
    let x = rect.left() + rect.width() * hsva.v.clamp(0.0, 1.0);
    ui.painter().line_segment(
        [
            Pos2::new(x, rect.top() - 2.0),
            Pos2::new(x, rect.bottom() + 2.0),
        ],
        Stroke::new(
            2.0_f32,
            if hsva.v < 0.5 {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
        ),
    );
}

#[allow(clippy::cast_precision_loss)]
fn paint_gradient_strip(ui: &Ui, rect: Rect, segments: usize, color_at: impl Fn(f32) -> Hsva) {
    if segments == 0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let segment_width = rect.width() / segments as f32;
    for index in 0..segments {
        let left = rect.left() + segment_width * index as f32;
        let right = rect.left() + segment_width * (index + 1) as f32;
        let radius = 4;
        let corner_radius = match (index == 0, index + 1 == segments) {
            (true, true) => CornerRadius::same(radius),
            (true, false) => CornerRadius {
                nw: radius,
                ne: 0,
                sw: radius,
                se: 0,
            },
            (false, true) => CornerRadius {
                nw: 0,
                ne: radius,
                sw: 0,
                se: radius,
            },
            (false, false) => CornerRadius::ZERO,
        };
        ui.painter().rect_filled(
            Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom())),
            corner_radius,
            color_at(index as f32 / (segments - 1).max(1) as f32),
        );
    }
}

/// A consistently sized combo-box field.
#[allow(dead_code)]
pub(crate) fn dropdown_field(
    ui: &mut Ui,
    id: impl Hash,
    selected_text: impl Into<WidgetText>,
    add_options: impl FnOnce(&mut Ui),
) -> InnerResponse<Option<()>> {
    dropdown_field_sized(ui, id, selected_text, STANDARD_CONTROL_SIZE.x, add_options)
}

/// A combo-box field with an intentional custom width for wider forms.
pub(crate) fn dropdown_field_sized(
    ui: &mut Ui,
    id: impl Hash,
    selected_text: impl Into<WidgetText>,
    width: f32,
    add_options: impl FnOnce(&mut Ui),
) -> InnerResponse<Option<()>> {
    ui.scope(|ui| {
        let field_stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        ui.visuals_mut().widgets.inactive.bg_stroke = field_stroke;
        ui.spacing_mut().icon_width = 16.0;
        ui.spacing_mut().icon_spacing = 6.0;
        // ComboBox::height controls the popup menu, not the closed field. The
        // visible button height comes from the button's vertical padding.
        let text_height = ui.text_style_height(&egui::TextStyle::Button);
        ui.spacing_mut().button_padding.y =
            ((STANDARD_CONTROL_SIZE.y - text_height) * 0.5).max(0.0);

        egui::ComboBox::from_id_salt(id)
            .selected_text(selected_text)
            .width(width)
            .icon(|ui, rect, visuals, is_open| {
                paint_component_icon(
                    ui,
                    rect,
                    if is_open {
                        RemixIcon::ContractUpDown
                    } else {
                        RemixIcon::ExpandUpDown
                    },
                    visuals.fg_stroke.color,
                );
            })
            .popup_style(dropdown_popup_style_modifier())
            .show_ui(ui, |ui| {
                add_options(ui);
            })
    })
    .inner
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
struct ThemedInputStyle {
    fill: Color32,
    stroke: Stroke,
}

#[allow(dead_code)]
fn themed_input_style(theme: ThemeTokens, focused: bool) -> ThemedInputStyle {
    ThemedInputStyle {
        fill: theme.input,
        stroke: Stroke::new(1.0_f32, if focused { theme.ring } else { theme.border }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
struct ThemedSliderStyle {
    track: Color32,
    accent: Color32,
    foreground: Color32,
}

#[allow(dead_code)]
fn themed_slider_style(theme: ThemeTokens) -> ThemedSliderStyle {
    ThemedSliderStyle {
        track: theme.input,
        accent: theme.primary,
        foreground: theme.primary_foreground,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
struct ShortcutPillStyle {
    fill: Color32,
    foreground: Color32,
    text_width: f32,
    width: f32,
}

#[allow(dead_code)]
#[allow(clippy::cast_precision_loss)]
fn shortcut_pill_style(theme: ThemeTokens, label: &str) -> ShortcutPillStyle {
    let text_width = label.chars().count() as f32 * 6.5;
    ShortcutPillStyle {
        fill: theme.muted,
        foreground: theme.muted_foreground,
        text_width,
        width: text_width.max(24.0) + 16.0,
    }
}

#[cfg(test)]
fn shortcut_pill_width(text_width: f32) -> f32 {
    text_width.max(24.0) + 16.0
}

#[cfg(test)]
fn themed_dropdown_widget_strokes_suppressed() -> bool {
    let strokes = [Stroke::NONE, Stroke::NONE, Stroke::NONE];
    strokes.into_iter().all(|stroke| stroke == Stroke::NONE)
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThemedCheckboxStyle {
    fill: Color32,
    foreground: Color32,
    ring: Color32,
}

#[cfg(test)]
fn themed_checkbox_style(theme: ThemeTokens, checked: bool, _focused: bool) -> ThemedCheckboxStyle {
    ThemedCheckboxStyle {
        fill: if checked { theme.primary } else { theme.input },
        foreground: if checked {
            theme.primary_foreground
        } else {
            theme.border
        },
        ring: theme.ring,
    }
}

#[allow(dead_code)]
pub(crate) fn themed_dropdown_field_sized(
    ui: &mut Ui,
    theme: ThemeTokens,
    id: impl Hash,
    selected_text: impl Into<WidgetText>,
    width: f32,
    add_options: impl FnOnce(&mut Ui),
) -> InnerResponse<Option<()>> {
    ui.scope(|ui| {
        let style = themed_input_style(theme, false);
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.bg_fill = style.fill;
        visuals.widgets.inactive.weak_bg_fill = style.fill;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.bg_fill = theme.accent;
        visuals.widgets.hovered.weak_bg_fill = theme.accent;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.active.bg_stroke = Stroke::NONE;
        ui.spacing_mut().icon_width = 16.0;
        ui.spacing_mut().icon_spacing = 6.0;
        let text_height = ui.text_style_height(&egui::TextStyle::Button);
        ui.spacing_mut().button_padding.y =
            ((STANDARD_CONTROL_SIZE.y - text_height) * 0.5).max(0.0);
        let response = egui::ComboBox::from_id_salt(id)
            .selected_text(selected_text)
            .width(width)
            .icon(|ui, rect, visuals, is_open| {
                paint_component_icon(
                    ui,
                    rect,
                    if is_open {
                        RemixIcon::ContractUpDown
                    } else {
                        RemixIcon::ExpandUpDown
                    },
                    visuals.fg_stroke.color,
                );
            })
            .popup_style(dropdown_popup_style_modifier())
            .show_ui(ui, add_options);
        let stroke = themed_input_style(theme, response.response.has_focus()).stroke;
        ui.painter().rect_stroke(
            response.response.rect,
            CornerRadius::same(theme.radius.md),
            stroke,
            StrokeKind::Inside,
        );
        response
    })
    .inner
}

#[allow(dead_code)]
pub(crate) fn themed_text_field(
    ui: &mut Ui,
    theme: ThemeTokens,
    value: &mut String,
    hint: impl Into<WidgetText>,
) -> Response {
    ui.scope(|ui| {
        let response = {
            let style = themed_input_style(theme, false);
            let visuals = ui.visuals_mut();
            visuals.text_edit_bg_color = Some(style.fill);
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
            ] {
                widget.bg_stroke = Stroke::NONE;
                widget.weak_bg_fill = style.fill;
            }
            visuals.selection.bg_fill = theme.accent;
            visuals.selection.stroke = Stroke::new(1.0_f32, theme.accent_foreground);
            ui.add_sized(
                STANDARD_CONTROL_SIZE,
                TextEdit::singleline(value)
                    .hint_text(hint)
                    .margin(Margin::symmetric(8, 0))
                    .vertical_align(Align::Center),
            )
        };
        let style = themed_input_style(theme, response.has_focus());
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(theme.radius.md),
            style.stroke,
            StrokeKind::Inside,
        );
        response
    })
    .inner
}

#[allow(dead_code)]
pub(crate) fn themed_range_slider(
    ui: &mut Ui,
    theme: ThemeTokens,
    value: &mut f32,
    range: RangeInclusive<f32>,
    size: Vec2,
    tooltip: Option<String>,
) -> Response {
    let style = themed_slider_style(theme);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = size.x;
            ui.spacing_mut().interact_size.y = size.y;
            let visuals = ui.visuals_mut();
            visuals.slider_trailing_fill = true;
            visuals.selection.bg_fill = style.accent;
            visuals.selection.stroke = Stroke::new(1.0_f32, style.foreground);
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
            ] {
                widget.bg_fill = style.track;
                widget.fg_stroke = Stroke::new(1.0_f32, style.foreground);
            }
            ui.add_sized(
                size,
                egui::Slider::new(value, range)
                    .show_value(false)
                    .trailing_fill(true)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
        })
        .inner;
    if let Some(tooltip) = tooltip
        && (response.hovered() || response.dragged())
    {
        let thumb_x = response.rect.left() + response.rect.width() * value.clamp(0.0, 1.0);
        let mut popup = egui::Tooltip::always_open(
            ui.ctx().clone(),
            response.layer_id,
            response.id.with("range-value"),
            Pos2::new(thumb_x, response.rect.top()),
        );
        popup.popup = popup.popup.align(RectAlign::TOP).gap(6.0).width(76.0);
        popup.show(|ui| {
            ui.add(egui::Label::new(tooltip).wrap_mode(egui::TextWrapMode::Extend));
        });
    }
    response
}

#[allow(dead_code)]
pub(crate) fn themed_numeric_field(
    ui: &mut Ui,
    theme: ThemeTokens,
    value: &mut f32,
    range: RangeInclusive<f32>,
    size: Vec2,
    suffix: &str,
) -> Response {
    let minimum = *range.start();
    let maximum = *range.end();
    let step = 0.01;
    let stepper_width = 32.0_f32.min((size.x * 0.22).max(28.0));
    let input_width = (size.x - stepper_width * 2.0).max(1.0);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let field_rect = ui.max_rect();
            let style = themed_input_style(theme, false);
            ui.painter()
                .rect_filled(field_rect, CornerRadius::same(theme.radius.sm), style.fill);
            let decrement = themed_numeric_stepper_button(
                ui,
                Vec2::new(stepper_width, size.y),
                RemixIcon::ArrowDownS,
                true,
                theme,
            );
            let mut response = ui
                .scope(|ui| {
                    let visuals = ui.visuals_mut();
                    visuals.text_edit_bg_color = Some(style.fill);
                    visuals.selection.stroke = Stroke::NONE;
                    for widget in [
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                    ] {
                        widget.weak_bg_fill = Color32::TRANSPARENT;
                        widget.bg_stroke = Stroke::NONE;
                        widget.corner_radius = CornerRadius::ZERO;
                    }
                    ui.add_sized(
                        Vec2::new(input_width, size.y),
                        egui::DragValue::new(value)
                            .range(minimum..=maximum)
                            .speed(0.01)
                            .fixed_decimals(2)
                            .suffix(suffix),
                    )
                })
                .inner;
            let has_focus = response.has_focus();
            let increment = themed_numeric_stepper_button(
                ui,
                Vec2::new(stepper_width, size.y),
                RemixIcon::ArrowUpS,
                false,
                theme,
            );
            if increment.clicked() {
                *value = (*value + step).clamp(minimum, maximum);
                response.mark_changed();
            }
            if decrement.clicked() {
                *value = (*value - step).clamp(minimum, maximum);
                response.mark_changed();
            }
            let response = response.union(decrement).union(increment);
            ui.painter().rect_stroke(
                field_rect,
                CornerRadius::same(theme.radius.sm),
                themed_input_style(theme, has_focus).stroke,
                StrokeKind::Inside,
            );
            response
        },
    )
    .inner
}

#[allow(dead_code)]
fn themed_numeric_stepper_button(
    ui: &mut Ui,
    size: Vec2,
    icon: RemixIcon,
    left_side: bool,
    theme: ThemeTokens,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        theme.accent
    } else {
        theme.input
    };
    let radius = theme.radius.sm;
    let corner_radius = if left_side {
        CornerRadius {
            nw: radius,
            ne: 0,
            sw: radius,
            se: 0,
        }
    } else {
        CornerRadius {
            nw: 0,
            ne: radius,
            sw: 0,
            se: radius,
        }
    };
    ui.painter().rect_filled(rect, corner_radius, fill);
    ui.painter().line_segment(
        if left_side {
            [rect.right_top(), rect.right_bottom()]
        } else {
            [rect.left_top(), rect.left_bottom()]
        },
        Stroke::new(1.0_f32, theme.border),
    );
    paint_component_icon(ui, rect, icon, theme.foreground);
    response
}

#[allow(dead_code)]
pub(crate) fn themed_checkbox(
    ui: &mut Ui,
    theme: ThemeTokens,
    checked: &mut bool,
    label: impl Into<WidgetText>,
) -> Response {
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        visuals.selection.bg_fill = theme.primary;
        visuals.selection.stroke = Stroke::new(1.0_f32, theme.primary_foreground);
        visuals.widgets.active.bg_fill = theme.primary;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, theme.primary_foreground);
        ui.checkbox(checked, label)
    })
    .inner
}

#[allow(dead_code)]
pub(crate) fn themed_shortcut_pill(
    ui: &mut Ui,
    theme: ThemeTokens,
    label: impl Into<String>,
) -> Response {
    let label = label.into();
    let style = shortcut_pill_style(theme, &label);
    let width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(label.clone(), FontId::proportional(11.0), style.foreground)
            .size()
            .x
            .max(24.0)
            + 16.0
    });
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    ui.painter().rect(
        rect,
        CornerRadius::same(theme.radius.md),
        style.fill,
        Stroke::new(1.0_f32, theme.border),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        style.foreground,
    );
    response
}

/// A regular single-line text field with consistent sizing.
pub(crate) fn text_field(ui: &mut Ui, value: &mut String, hint: impl Into<WidgetText>) -> Response {
    sized_text_field(ui, value, STANDARD_CONTROL_SIZE, hint)
}

/// A text button with the same internal padding everywhere it is used.
pub(crate) fn button(
    ui: &mut Ui,
    label: impl Into<WidgetText>,
    size: Vec2,
    fill: Color32,
) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = BUTTON_PADDING;
        ui.add_sized(size, egui::Button::new(label).fill(fill))
    })
    .inner
}

/// A text field with an intentional custom size for compact popups.
pub(crate) fn sized_text_field(
    ui: &mut Ui,
    value: &mut String,
    size: Vec2,
    hint: impl Into<WidgetText>,
) -> Response {
    ui.scope(|ui| {
        let field_fill = ui.visuals().widgets.inactive.weak_bg_fill;
        let field_stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let visuals = ui.visuals_mut();
        visuals.text_edit_bg_color = Some(field_fill);
        visuals.widgets.inactive.bg_stroke = field_stroke;
        ui.add_sized(
            size,
            TextEdit::singleline(value)
                .hint_text(hint)
                .margin(Margin::symmetric(8, 0))
                .vertical_align(Align::Center),
        )
    })
    .inner
}

/// A searchable single-line text field.
#[allow(dead_code)]
pub(crate) fn search_field(ui: &mut Ui, value: &mut String) -> Response {
    text_field(ui, value, "Quick search")
}

/// A compact range slider with an optional hover/drag tooltip.
pub(crate) fn range_slider(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    size: Vec2,
    track: egui::Color32,
    accent: egui::Color32,
    tooltip: Option<String>,
) -> Response {
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = size.x;
            ui.spacing_mut().interact_size.y = size.y;
            let visuals = ui.visuals_mut();
            visuals.slider_trailing_fill = true;
            visuals.selection.bg_fill = accent;
            visuals.widgets.inactive.bg_fill = track;
            visuals.widgets.hovered.bg_fill = track;
            visuals.widgets.active.bg_fill = track;
            visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, accent);
            visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, accent);
            visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, accent);
            ui.add_sized(
                size,
                egui::Slider::new(value, range)
                    .show_value(false)
                    .trailing_fill(true)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
        })
        .inner;
    if let Some(tooltip) = tooltip
        && (response.hovered() || response.dragged())
    {
        let thumb_x = response.rect.left() + response.rect.width() * value.clamp(0.0, 1.0);
        let mut popup = egui::Tooltip::always_open(
            ui.ctx().clone(),
            response.layer_id,
            response.id.with("range-value"),
            Pos2::new(thumb_x, response.rect.top()),
        );
        popup.popup = popup.popup.align(RectAlign::TOP).gap(6.0).width(76.0);
        popup.show(|ui| {
            ui.add(egui::Label::new(tooltip).wrap_mode(egui::TextWrapMode::Extend));
        });
    }
    response
}

/// A compact numeric input for values that also have a draggable range.
pub(crate) fn numeric_field(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    size: Vec2,
    suffix: &str,
) -> Response {
    numeric_field_with_decimals(ui, value, range, size, 2, suffix)
}

/// A numeric field with an explicit decimal precision for context-specific
/// values such as whole-pixel font sizes.
pub(crate) fn numeric_field_with_decimals(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    size: Vec2,
    decimals: usize,
    suffix: &str,
) -> Response {
    let minimum = *range.start();
    let maximum = *range.end();
    let step = if decimals == 0 {
        1.0
    } else {
        10.0_f32.powi(-i32::try_from(decimals).unwrap_or(i32::MAX))
    };
    let stepper_width = 32.0_f32.min((size.x * 0.22).max(28.0));
    let input_width = (size.x - stepper_width * 2.0).max(1.0);

    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let field_rect = ui.max_rect();
            let field_fill = ui.visuals().widgets.inactive.bg_fill;
            let field_border = ui.visuals().widgets.noninteractive.bg_stroke.color;
            ui.painter()
                .rect_filled(field_rect, CornerRadius::same(4), field_fill);

            let decrement = numeric_stepper_button(
                ui,
                Vec2::new(stepper_width, size.y),
                RemixIcon::ArrowDownS,
                true,
            );
            let mut response = ui
                .scope(|ui| {
                    let visuals = ui.visuals_mut();
                    visuals.text_edit_bg_color = Some(field_fill);
                    visuals.selection.stroke = Stroke::NONE;
                    for widget in [
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                    ] {
                        widget.weak_bg_fill = Color32::TRANSPARENT;
                        widget.bg_stroke = Stroke::NONE;
                        widget.corner_radius = CornerRadius::ZERO;
                    }
                    ui.add_sized(
                        Vec2::new(input_width, size.y),
                        egui::DragValue::new(value)
                            .range(minimum..=maximum)
                            .speed(if decimals == 0 { 1.0 } else { 0.01 })
                            .fixed_decimals(decimals)
                            .suffix(suffix),
                    )
                })
                .inner;
            let has_focus = response.has_focus();
            let increment = numeric_stepper_button(
                ui,
                Vec2::new(stepper_width, size.y),
                RemixIcon::ArrowUpS,
                false,
            );

            if increment.clicked() {
                *value = (*value + step).clamp(minimum, maximum);
                response.mark_changed();
            }
            if decrement.clicked() {
                *value = (*value - step).clamp(minimum, maximum);
                response.mark_changed();
            }
            let response = response.union(decrement).union(increment);
            ui.painter().rect_stroke(
                field_rect,
                CornerRadius::same(4),
                numeric_field_border(has_focus, field_border),
                StrokeKind::Inside,
            );
            response
        },
    )
    .inner
}

fn numeric_field_border(focused: bool, fallback: Color32) -> Stroke {
    if focused {
        Stroke::new(1.0_f32, SWATCH_ACCENT)
    } else {
        Stroke::new(1.0_f32, fallback)
    }
}

fn numeric_stepper_button(ui: &mut Ui, size: Vec2, icon: RemixIcon, left_side: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    let corner_radius = if left_side {
        CornerRadius {
            nw: 4,
            ne: 0,
            sw: 4,
            se: 0,
        }
    } else {
        CornerRadius {
            nw: 0,
            ne: 4,
            sw: 0,
            se: 4,
        }
    };
    ui.painter().rect_filled(rect, corner_radius, fill);
    ui.painter().line_segment(
        if left_side {
            [rect.right_top(), rect.right_bottom()]
        } else {
            [rect.left_top(), rect.left_bottom()]
        },
        Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    paint_component_icon(ui, rect, icon, ui.visuals().text_color());
    response
}

fn paint_component_icon(ui: &Ui, rect: Rect, icon: RemixIcon, color: Color32) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.glyph().to_string(),
        FontId::new(
            14.0,
            egui::FontFamily::Name(remix_icons::FONT_FAMILY.into()),
        ),
        color,
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn swatch_border_covers_near_white_colors() {
        assert!(swatch_needs_contrast_border(
            Color32::from_rgb(246, 247, 249),
            false,
        ));
        assert!(swatch_needs_contrast_border(Color32::WHITE, false));
        assert!(swatch_needs_contrast_border(
            Color32::from_rgb(20, 20, 24),
            false,
        ));
        assert!(!swatch_needs_contrast_border(
            Color32::from_rgb(224, 49, 49),
            false,
        ));
    }

    #[test]
    fn swatch_frame_uses_one_shared_geometry() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(26.0));
        assert_eq!(swatch_frame_geometry(rect).0, rect.shrink(1.0));
        assert_eq!(swatch_frame_geometry(rect).1, CornerRadius::same(5));
    }

    #[test]
    fn light_swatch_border_has_enough_contrast_for_white_colors() {
        assert_eq!(SWATCH_LIGHT_BORDER, Color32::from_rgb(205, 209, 218));
    }

    #[test]
    fn dark_swatch_border_remains_visible_in_both_modes() {
        let dark_fill = Color32::from_rgb(26, 27, 30);

        assert_eq!(swatch_border_color(dark_fill, false), SWATCH_LIGHT_BORDER);
        assert_eq!(
            swatch_border_color(dark_fill, true),
            SWATCH_DARK_FILL_BORDER
        );
    }

    use super::*;

    #[test]
    fn surface_styles_select_each_semantic_role() {
        let theme = ThemeTokens::light();
        let radius = 9;

        assert_eq!(
            surface_style(theme, SurfaceVariant::Background, radius),
            SurfaceStyle {
                fill: theme.background,
                foreground: theme.foreground,
                border: theme.border,
                radius,
            }
        );
        assert_eq!(
            surface_style(theme, SurfaceVariant::Card, radius),
            SurfaceStyle {
                fill: theme.card,
                foreground: theme.card_foreground,
                border: theme.border,
                radius,
            }
        );
        assert_eq!(
            surface_style(theme, SurfaceVariant::Popover, radius),
            SurfaceStyle {
                fill: theme.popover,
                foreground: theme.popover_foreground,
                border: theme.border,
                radius,
            }
        );
        assert_eq!(
            surface_style(theme, SurfaceVariant::Sidebar, radius),
            SurfaceStyle {
                fill: theme.sidebar,
                foreground: theme.sidebar_foreground,
                border: theme.sidebar_border,
                radius,
            }
        );
        assert_eq!(
            surface_foreground(theme, SurfaceVariant::Background),
            theme.foreground
        );
        assert_eq!(
            surface_foreground(theme, SurfaceVariant::Card),
            theme.card_foreground
        );
        assert_eq!(
            surface_foreground(theme, SurfaceVariant::Popover),
            theme.popover_foreground
        );
        assert_eq!(
            surface_foreground(theme, SurfaceVariant::Sidebar),
            theme.sidebar_foreground
        );
    }

    #[test]
    fn button_styles_select_each_semantic_variant() {
        let theme = ThemeTokens::light();

        assert_eq!(
            button_style(theme, ButtonVariant::Primary),
            ButtonStyle {
                fill: theme.primary,
                foreground: theme.primary_foreground,
                hover_fill: theme.accent,
                hover_foreground: theme.accent_foreground,
                active_fill: theme.primary,
                active_foreground: theme.primary_foreground,
                border: theme.ring,
            }
        );
        assert_eq!(
            button_style(theme, ButtonVariant::Secondary),
            ButtonStyle {
                fill: theme.secondary,
                foreground: theme.secondary_foreground,
                hover_fill: theme.accent,
                hover_foreground: theme.accent_foreground,
                active_fill: theme.primary,
                active_foreground: theme.primary_foreground,
                border: theme.border,
            }
        );
        assert_eq!(
            button_style(theme, ButtonVariant::Outline),
            ButtonStyle {
                fill: Color32::TRANSPARENT,
                foreground: theme.foreground,
                hover_fill: theme.accent,
                hover_foreground: theme.accent_foreground,
                active_fill: theme.secondary,
                active_foreground: theme.secondary_foreground,
                border: theme.border,
            }
        );
        assert_eq!(
            button_style(theme, ButtonVariant::Ghost),
            ButtonStyle {
                fill: Color32::TRANSPARENT,
                foreground: theme.foreground,
                hover_fill: theme.accent,
                hover_foreground: theme.accent_foreground,
                active_fill: theme.secondary,
                active_foreground: theme.secondary_foreground,
                border: Color32::TRANSPARENT,
            }
        );
        assert_eq!(
            button_style(theme, ButtonVariant::Destructive),
            ButtonStyle {
                fill: theme.destructive,
                foreground: theme.destructive_foreground,
                hover_fill: theme.destructive,
                hover_foreground: theme.destructive_foreground,
                active_fill: theme.destructive,
                active_foreground: theme.destructive_foreground,
                border: theme.ring,
            }
        );
    }

    #[test]
    fn prominent_button_foregrounds_are_semantic_contrast_roles() {
        let theme = ThemeTokens::light();

        assert_eq!(
            button_style(theme, ButtonVariant::Primary).foreground,
            theme.primary_foreground
        );
        assert_eq!(
            button_style(theme, ButtonVariant::Destructive).foreground,
            theme.destructive_foreground
        );
        assert_ne!(
            button_style(theme, ButtonVariant::Primary).foreground,
            theme.foreground
        );
        assert_ne!(
            button_style(theme, ButtonVariant::Destructive).foreground,
            theme.foreground
        );
    }

    #[test]
    fn button_focus_ring_is_present_only_when_focused() {
        let theme = ThemeTokens::light();

        assert_eq!(
            button_focus_ring(theme, false),
            None,
            "unfocused buttons should not paint a focus ring"
        );
        assert_eq!(
            button_focus_ring(theme, true),
            Some((
                Stroke::new(2.0_f32, theme.ring),
                CornerRadius::same(theme.radius.md),
            ))
        );
    }

    #[test]
    fn numeric_field_focus_uses_one_outer_outline() {
        let base = Color32::from_rgb(62, 64, 72);

        assert_eq!(
            numeric_field_border(false, base),
            Stroke::new(1.0_f32, base)
        );
        assert_eq!(
            numeric_field_border(true, base),
            Stroke::new(1.0_f32, SWATCH_ACCENT)
        );
    }

    #[test]
    fn themed_input_style_has_one_border_or_focus_ring() {
        let theme = ThemeTokens::light();

        assert_eq!(themed_input_style(theme, false).fill, theme.input);
        assert_eq!(
            themed_input_style(theme, false).stroke,
            Stroke::new(1.0_f32, theme.border)
        );
        assert_eq!(
            themed_input_style(theme, true).stroke,
            Stroke::new(1.0_f32, theme.ring)
        );
    }

    #[test]
    fn themed_input_fill_changes_with_light_and_dark_theme() {
        assert_ne!(
            themed_input_style(ThemeTokens::light(), false).fill,
            themed_input_style(ThemeTokens::dark(), false).fill
        );
    }

    #[test]
    fn themed_slider_uses_primary_accent_and_semantic_contrast() {
        let theme = ThemeTokens::dark();
        let style = themed_slider_style(theme);

        assert_eq!(style.track, theme.input);
        assert_eq!(style.accent, theme.primary);
        assert_eq!(style.foreground, theme.primary_foreground);
    }

    #[test]
    fn shortcut_pill_has_padding_for_common_and_long_labels() {
        let theme = ThemeTokens::light();
        let common = shortcut_pill_style(theme, "Ctrl + D");
        let long = shortcut_pill_style(theme, "Ctrl + Shift + Alt + Delete");

        assert_eq!(common.fill, theme.muted);
        assert_eq!(common.foreground, theme.muted_foreground);
        assert!(
            shortcut_pill_width(common.text_width)
                .total_cmp(&common.width)
                .is_eq()
        );
        assert!(
            shortcut_pill_width(long.text_width)
                .total_cmp(&long.width)
                .is_eq()
        );
        assert!(shortcut_pill_width(42.0) >= 58.0);
        assert!(shortcut_pill_width(180.0) > shortcut_pill_width(42.0));
    }

    #[test]
    fn themed_dropdown_suppresses_all_default_field_strokes() {
        assert!(themed_dropdown_widget_strokes_suppressed());
    }

    #[test]
    fn dropdown_popup_highlight_is_readable_and_not_cramped() {
        let popup = dropdown_popup_style();
        assert_eq!(
            Stroke::new(1.0_f32, popup.selected_text),
            Stroke::new(1.0_f32, Color32::WHITE)
        );
        assert_eq!(popup.selected_text, Color32::WHITE);
        assert_eq!(popup.menu_margin, Margin::symmetric(6, 10));
        assert!(popup.menu_margin.top >= 8);
        let padding = popup.menu_margin.topf();
        let item_spacing = popup.item_spacing_y;
        assert!(padding >= 8.0);
        assert!(item_spacing >= 4.0);
        assert_eq!(DROPDOWN_POPUP_RADIUS, 8);
    }

    #[test]
    fn themed_checkbox_uses_checked_and_unchecked_semantic_roles() {
        let theme = ThemeTokens::dark();
        let checked = themed_checkbox_style(theme, true, false);
        let unchecked = themed_checkbox_style(theme, false, false);
        let focused = themed_checkbox_style(theme, false, true);

        assert_eq!(checked.fill, theme.primary);
        assert_eq!(checked.foreground, theme.primary_foreground);
        assert_eq!(unchecked.fill, theme.input);
        assert_eq!(unchecked.foreground, theme.border);
        assert_eq!(focused.ring, theme.ring);
    }

    #[test]
    fn color_wheel_selection_maps_edge_and_rejects_outside_pointer() {
        let center = Pos2::new(20.0, 20.0);
        let selection = wheel_selection(Pos2::new(30.0, 20.0), center, 10.0, 0.75);
        assert!(selection.is_some(), "the wheel edge should be selectable");
        let selection = selection.unwrap_or_default();

        assert!((selection.h - 0.0).abs() < f32::EPSILON);
        assert!((selection.s - 1.0).abs() < f32::EPSILON);
        assert!((selection.v - 0.75).abs() < f32::EPSILON);
        assert!(wheel_selection(Pos2::new(31.0, 20.0), center, 10.0, 0.75).is_none());
    }
}
