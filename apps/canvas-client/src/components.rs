//! Reusable egui controls shared by Sketchi's settings and property panels.

use std::{hash::Hash, ops::RangeInclusive};

use egui::epaint::Hsva;
use egui::{
    Align2, Color32, CornerRadius, FontId, InnerResponse, Mesh, Pos2, Rect, RectAlign, Response,
    Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2, WidgetText,
};

use crate::remix_icons::{self, RemixIcon};

pub(crate) const STANDARD_CONTROL_SIZE: Vec2 = Vec2::new(190.0, 30.0);
pub(crate) const BUTTON_PADDING: Vec2 = Vec2::new(10.0, 5.0);

const SWATCH_ACCENT: Color32 = Color32::from_rgb(91, 87, 214);
const SWATCH_LIGHT_BORDER: Color32 = Color32::from_rgb(229, 231, 235);
const SWATCH_DARK_BORDER: Color32 = Color32::from_rgb(62, 64, 72);
const SWATCH_DARK_TEXT: Color32 = Color32::from_rgb(232, 233, 237);
const SWATCH_LIGHT_MUTED: Color32 = Color32::from_rgb(107, 111, 122);

/// A consistent color choice control used by the properties panel, settings,
/// and color picker.
pub(crate) fn color_swatch(
    ui: &mut Ui,
    color: Option<Color32>,
    selected: bool,
    dark_mode: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let fill = color.unwrap_or(Color32::TRANSPARENT);
    let transparent = color.is_none() || fill.a() == 0;
    if transparent {
        ui.painter().rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(4),
            if dark_mode {
                Color32::from_rgb(54, 55, 61)
            } else {
                Color32::from_rgb(245, 246, 248)
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
        ui.painter()
            .rect_filled(rect.shrink(2.0), CornerRadius::same(4), fill);
    }
    if selected {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(7),
            Stroke::new(2.0_f32, SWATCH_ACCENT),
            StrokeKind::Outside,
        );
    } else if transparent
        || fill == Color32::WHITE
        || (fill.r() < 80 && fill.g() < 80 && fill.b() < 80)
    {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(5),
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
pub(crate) fn dropdown_field(
    ui: &mut Ui,
    id: impl Hash,
    selected_text: impl Into<WidgetText>,
    add_options: impl FnOnce(&mut Ui),
) -> InnerResponse<Option<()>> {
    ui.scope(|ui| {
        // ComboBox::height controls the popup menu, not the closed field. The
        // visible button height comes from the button's vertical padding.
        let text_height = ui.text_style_height(&egui::TextStyle::Button);
        ui.spacing_mut().button_padding.y =
            ((STANDARD_CONTROL_SIZE.y - text_height) * 0.5).max(0.0);

        egui::ComboBox::from_id_salt(id)
            .selected_text(selected_text)
            .width(STANDARD_CONTROL_SIZE.x)
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
            .show_ui(ui, add_options)
    })
    .inner
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
    ui.add_sized(size, TextEdit::singleline(value).hint_text(hint))
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
    use super::*;

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
