//! Settings-window visual primitives kept separate from canvas interaction.

use super::{
    ACCENT, Color32, CornerRadius, DARK_MUTED, DARK_TEXT, LIGHT_BORDER, SETTINGS_CARD_BORDER_DARK,
    SETTINGS_CARD_DARK, SETTINGS_CONTROL_DARK, SETTINGS_CONTROL_HOVER_DARK,
    SETTINGS_CONTROL_RADIUS, SETTINGS_PALETTE_GAP, SETTINGS_PALETTE_LABEL_WIDTH,
    SETTINGS_ROOT_DARK, STANDARD_CONTROL_SIZE, Stroke, Vec2, color_swatch_preview, sketchi_visuals,
};

fn settings_widget_visuals(
    fill: Color32,
    foreground: Color32,
    stroke: Stroke,
    corner_radius: u8,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        weak_bg_fill: fill,
        bg_fill: fill,
        bg_stroke: stroke,
        fg_stroke: Stroke::new(1.0_f32, foreground),
        corner_radius: CornerRadius::same(corner_radius),
        expansion: 0.0,
    }
}

pub(super) fn settings_visuals(dark_mode: bool) -> egui::Visuals {
    let mut visuals = sketchi_visuals(dark_mode);
    let border = Stroke::new(
        1.0_f32,
        if dark_mode {
            SETTINGS_CARD_BORDER_DARK
        } else {
            LIGHT_BORDER
        },
    );
    let focus = Stroke::new(1.0_f32, ACCENT);
    visuals.window_stroke = border;
    visuals.window_corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.widgets.active.corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    visuals.widgets.open.corner_radius = CornerRadius::same(SETTINGS_CONTROL_RADIUS);
    if dark_mode {
        visuals.override_text_color = Some(DARK_TEXT);
        visuals.weak_text_color = Some(DARK_MUTED);
        visuals.hyperlink_color = ACCENT;
        visuals.faint_bg_color = SETTINGS_CARD_DARK;
        visuals.extreme_bg_color = SETTINGS_ROOT_DARK;
        visuals.text_edit_bg_color = Some(SETTINGS_CONTROL_DARK);
        visuals.window_fill = SETTINGS_CARD_DARK;
        visuals.panel_fill = SETTINGS_ROOT_DARK;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        visuals.widgets.noninteractive = settings_widget_visuals(
            SETTINGS_ROOT_DARK,
            DARK_TEXT,
            border,
            SETTINGS_CONTROL_RADIUS,
        );
        visuals.widgets.inactive = settings_widget_visuals(
            SETTINGS_CONTROL_DARK,
            DARK_TEXT,
            border,
            SETTINGS_CONTROL_RADIUS,
        );
        visuals.widgets.hovered = settings_widget_visuals(
            SETTINGS_CONTROL_HOVER_DARK,
            DARK_TEXT,
            focus,
            SETTINGS_CONTROL_RADIUS,
        );
        visuals.widgets.active =
            settings_widget_visuals(ACCENT, Color32::WHITE, focus, SETTINGS_CONTROL_RADIUS);
        visuals.widgets.open = settings_widget_visuals(
            SETTINGS_CARD_DARK,
            DARK_TEXT,
            border,
            SETTINGS_CONTROL_RADIUS,
        );
    } else {
        visuals.widgets.noninteractive.bg_stroke = border;
        visuals.widgets.inactive.bg_stroke = border;
        visuals.widgets.hovered.bg_stroke = focus;
        visuals.widgets.active.bg_stroke = focus;
        visuals.widgets.open.bg_stroke = focus;
    }
    visuals
}

pub(super) fn settings_palette_row(
    ui: &mut egui::Ui,
    label: &str,
    palette: &[Color32; 15],
    dark_mode: bool,
) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(width, STANDARD_CONTROL_SIZE.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = SETTINGS_PALETTE_GAP;
            ui.add_sized(
                Vec2::new(SETTINGS_PALETTE_LABEL_WIDTH, STANDARD_CONTROL_SIZE.y),
                egui::Label::new(label).truncate().halign(egui::Align::LEFT),
            );
            for index in 0..palette.len() {
                color_swatch_preview(ui, palette.get(index).copied(), false, dark_mode);
            }
        },
    );
}
