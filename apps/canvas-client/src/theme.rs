use egui::{Color32, Context, CornerRadius, Stroke, Visuals, style::WidgetVisuals};

// These are fixed sRGB conversions of the supplied OKLCH palette. Keeping the
// converted bytes here avoids a runtime color-space dependency.
const LIGHT_BACKGROUND: Color32 = Color32::from_rgb(255, 255, 255);
const LIGHT_FOREGROUND: Color32 = Color32::from_rgb(9, 9, 11);
const LIGHT_CARD: Color32 = Color32::WHITE;
const LIGHT_PRIMARY: Color32 = Color32::from_rgb(112, 8, 231);
const LIGHT_PRIMARY_FOREGROUND: Color32 = Color32::from_rgb(245, 243, 255);
const LIGHT_SECONDARY: Color32 = Color32::from_rgb(244, 244, 245);
const LIGHT_SECONDARY_FOREGROUND: Color32 = Color32::from_rgb(24, 24, 27);
const LIGHT_MUTED: Color32 = LIGHT_SECONDARY;
const LIGHT_MUTED_FOREGROUND: Color32 = Color32::from_rgb(113, 113, 123);
const LIGHT_ACCENT: Color32 = LIGHT_SECONDARY;
const LIGHT_ACCENT_FOREGROUND: Color32 = LIGHT_SECONDARY_FOREGROUND;
const LIGHT_DESTRUCTIVE: Color32 = Color32::from_rgb(231, 0, 11);
const LIGHT_BORDER: Color32 = Color32::from_rgb(228, 228, 231);
const LIGHT_RING: Color32 = Color32::from_rgb(159, 159, 169);
const LIGHT_SIDEBAR: Color32 = Color32::from_rgb(250, 250, 250);
const LIGHT_SIDEBAR_PRIMARY: Color32 = Color32::from_rgb(127, 34, 254);

const DARK_BACKGROUND: Color32 = Color32::from_rgb(9, 9, 11);
const DARK_FOREGROUND: Color32 = Color32::from_rgb(250, 250, 250);
const DARK_CARD: Color32 = Color32::from_rgb(24, 24, 27);
const DARK_PRIMARY: Color32 = Color32::from_rgb(93, 14, 192);
const DARK_PRIMARY_FOREGROUND: Color32 = Color32::from_rgb(245, 243, 255);
const DARK_SECONDARY: Color32 = Color32::from_rgb(39, 39, 42);
const DARK_SECONDARY_FOREGROUND: Color32 = DARK_FOREGROUND;
const DARK_MUTED: Color32 = DARK_SECONDARY;
const DARK_MUTED_FOREGROUND: Color32 = Color32::from_rgb(159, 159, 169);
const DARK_ACCENT: Color32 = DARK_SECONDARY;
const DARK_ACCENT_FOREGROUND: Color32 = DARK_FOREGROUND;
const DARK_DESTRUCTIVE: Color32 = Color32::from_rgb(255, 100, 103);
const DARK_BORDER: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 26);
const DARK_INPUT: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 38);
const DARK_RING: Color32 = Color32::from_rgb(113, 113, 123);
const DARK_SIDEBAR_PRIMARY: Color32 = Color32::from_rgb(142, 81, 255);

const RADIUS: RadiusScale = RadiusScale {
    sm: 4,
    md: 5,
    lg: 7,
    xl: 10,
};

pub(crate) const CONTROL_CORNER_RADIUS: u8 = RADIUS.md;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RadiusScale {
    pub(crate) sm: u8,
    pub(crate) md: u8,
    pub(crate) lg: u8,
    pub(crate) xl: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ThemeTokens {
    dark_mode: bool,
    pub(crate) background: Color32,
    pub(crate) foreground: Color32,
    pub(crate) card: Color32,
    pub(crate) card_foreground: Color32,
    pub(crate) popover: Color32,
    pub(crate) popover_foreground: Color32,
    pub(crate) primary: Color32,
    pub(crate) primary_foreground: Color32,
    pub(crate) secondary: Color32,
    pub(crate) secondary_foreground: Color32,
    pub(crate) muted: Color32,
    pub(crate) muted_foreground: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_foreground: Color32,
    pub(crate) destructive: Color32,
    pub(crate) destructive_foreground: Color32,
    pub(crate) border: Color32,
    pub(crate) input: Color32,
    pub(crate) ring: Color32,
    pub(crate) sidebar: Color32,
    pub(crate) sidebar_foreground: Color32,
    pub(crate) sidebar_primary: Color32,
    pub(crate) sidebar_primary_foreground: Color32,
    pub(crate) sidebar_accent: Color32,
    pub(crate) sidebar_accent_foreground: Color32,
    pub(crate) sidebar_border: Color32,
    pub(crate) sidebar_ring: Color32,
    pub(crate) radius: RadiusScale,
}

impl ThemeTokens {
    pub(crate) const fn light() -> Self {
        Self {
            dark_mode: false,
            background: LIGHT_BACKGROUND,
            foreground: LIGHT_FOREGROUND,
            card: LIGHT_CARD,
            card_foreground: LIGHT_FOREGROUND,
            popover: LIGHT_CARD,
            popover_foreground: LIGHT_FOREGROUND,
            primary: LIGHT_PRIMARY,
            primary_foreground: LIGHT_PRIMARY_FOREGROUND,
            secondary: LIGHT_SECONDARY,
            secondary_foreground: LIGHT_SECONDARY_FOREGROUND,
            muted: LIGHT_MUTED,
            muted_foreground: LIGHT_MUTED_FOREGROUND,
            accent: LIGHT_ACCENT,
            accent_foreground: LIGHT_ACCENT_FOREGROUND,
            destructive: LIGHT_DESTRUCTIVE,
            // The supplied palette has no destructive-foreground role; use
            // its contrast-safe primary-foreground role for destructive fills.
            destructive_foreground: LIGHT_PRIMARY_FOREGROUND,
            border: LIGHT_BORDER,
            input: LIGHT_BORDER,
            ring: LIGHT_RING,
            sidebar: LIGHT_SIDEBAR,
            sidebar_foreground: LIGHT_FOREGROUND,
            sidebar_primary: LIGHT_SIDEBAR_PRIMARY,
            sidebar_primary_foreground: LIGHT_PRIMARY_FOREGROUND,
            sidebar_accent: LIGHT_ACCENT,
            sidebar_accent_foreground: LIGHT_ACCENT_FOREGROUND,
            sidebar_border: LIGHT_BORDER,
            sidebar_ring: LIGHT_RING,
            radius: RADIUS,
        }
    }

    pub(crate) const fn dark() -> Self {
        Self {
            dark_mode: true,
            background: DARK_BACKGROUND,
            foreground: DARK_FOREGROUND,
            card: DARK_CARD,
            card_foreground: DARK_FOREGROUND,
            popover: DARK_CARD,
            popover_foreground: DARK_FOREGROUND,
            primary: DARK_PRIMARY,
            primary_foreground: DARK_PRIMARY_FOREGROUND,
            secondary: DARK_SECONDARY,
            secondary_foreground: DARK_SECONDARY_FOREGROUND,
            muted: DARK_MUTED,
            muted_foreground: DARK_MUTED_FOREGROUND,
            accent: DARK_ACCENT,
            accent_foreground: DARK_ACCENT_FOREGROUND,
            destructive: DARK_DESTRUCTIVE,
            // The supplied palette has no destructive-foreground role; use
            // its contrast-safe primary-foreground role for destructive fills.
            destructive_foreground: DARK_PRIMARY_FOREGROUND,
            border: DARK_BORDER,
            input: DARK_INPUT,
            ring: DARK_RING,
            sidebar: DARK_CARD,
            sidebar_foreground: DARK_FOREGROUND,
            sidebar_primary: DARK_SIDEBAR_PRIMARY,
            sidebar_primary_foreground: DARK_PRIMARY_FOREGROUND,
            sidebar_accent: DARK_ACCENT,
            sidebar_accent_foreground: DARK_ACCENT_FOREGROUND,
            sidebar_border: DARK_BORDER,
            sidebar_ring: DARK_RING,
            radius: RADIUS,
        }
    }

    pub(crate) const fn for_dark_mode(dark_mode: bool) -> Self {
        if dark_mode {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub(crate) fn apply_to_context(self, context: &Context) {
        let mut visuals = if self.dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.dark_mode = self.dark_mode;
        visuals.override_text_color = Some(self.foreground);
        visuals.weak_text_color = Some(self.muted_foreground);
        visuals.hyperlink_color = self.primary;
        visuals.faint_bg_color = self.muted;
        visuals.extreme_bg_color = self.muted;
        visuals.text_edit_bg_color = Some(self.input);
        visuals.window_fill = self.popover;
        visuals.window_stroke = Stroke::new(1.0_f32, self.border);
        visuals.panel_fill = self.background;
        visuals.selection.bg_fill = self.accent;
        visuals.selection.stroke = Stroke::new(1.0_f32, self.accent_foreground);

        let normal = widget(self.input, self.foreground, self.border, self.radius.md);
        let hovered = widget(
            self.accent,
            self.accent_foreground,
            self.ring,
            self.radius.md,
        );
        let active = widget(
            self.primary,
            self.primary_foreground,
            self.ring,
            self.radius.md,
        );
        visuals.widgets.noninteractive = widget(
            self.background,
            self.foreground,
            self.border,
            self.radius.md,
        );
        visuals.widgets.inactive = normal;
        visuals.widgets.hovered = hovered;
        visuals.widgets.active = active;
        visuals.widgets.open = hovered;
        context.set_visuals(visuals);
    }
}

fn widget(fill: Color32, foreground: Color32, border: Color32, radius: u8) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0_f32, border),
        corner_radius: CornerRadius::same(radius),
        fg_stroke: Stroke::new(1.0_f32, foreground),
        expansion: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Context, CornerRadius};

    use super::{RadiusScale, ThemeTokens, widget};

    #[test]
    fn light_and_dark_backgrounds_differ() {
        assert_ne!(
            ThemeTokens::light().background,
            ThemeTokens::dark().background
        );
    }

    #[test]
    fn primary_foreground_differs_from_primary_fill() {
        assert_ne!(
            ThemeTokens::light().primary,
            ThemeTokens::light().primary_foreground
        );
        assert_ne!(
            ThemeTokens::dark().primary,
            ThemeTokens::dark().primary_foreground
        );
    }

    #[test]
    fn dark_mode_selector_matches_named_themes() {
        assert_eq!(ThemeTokens::for_dark_mode(false), ThemeTokens::light());
        assert_eq!(ThemeTokens::for_dark_mode(true), ThemeTokens::dark());
    }

    #[test]
    fn radius_values_are_strictly_ordered() {
        let radius = ThemeTokens::light().radius;
        assert!(radius.sm < radius.md);
        assert!(radius.md < radius.lg);
        assert!(radius.lg < radius.xl);
        assert_eq!(
            radius,
            RadiusScale {
                sm: 4,
                md: 5,
                lg: 7,
                xl: 10
            }
        );
    }

    #[test]
    fn light_palette_uses_supplied_oklch_conversions() {
        let theme = ThemeTokens::light();
        assert_eq!(theme.background, Color32::from_rgb(255, 255, 255));
        assert_eq!(theme.foreground, Color32::from_rgb(9, 9, 11));
        assert_eq!(theme.primary, Color32::from_rgb(112, 8, 231));
        assert_eq!(theme.primary_foreground, Color32::from_rgb(245, 243, 255));
        assert_eq!(theme.destructive, Color32::from_rgb(231, 0, 11));
        assert_eq!(theme.sidebar, Color32::from_rgb(250, 250, 250));
        assert_eq!(theme.sidebar_primary, Color32::from_rgb(127, 34, 254));
    }

    #[test]
    fn dark_palette_preserves_alpha_roles() {
        let theme = ThemeTokens::dark();
        assert_eq!(theme.background, Color32::from_rgb(9, 9, 11));
        assert_eq!(theme.primary, Color32::from_rgb(93, 14, 192));
        assert_eq!(theme.destructive, Color32::from_rgb(255, 100, 103));
        assert_eq!(
            theme.border,
            Color32::from_rgba_unmultiplied(255, 255, 255, 26)
        );
        assert_eq!(
            theme.input,
            Color32::from_rgba_unmultiplied(255, 255, 255, 38)
        );
        assert_eq!(theme.destructive_foreground, theme.primary_foreground);
    }

    #[test]
    fn named_themes_carry_their_source_mode() {
        assert!(!ThemeTokens::light().dark_mode);
        assert!(ThemeTokens::dark().dark_mode);
    }

    #[test]
    fn widget_uses_the_radius_supplied_by_the_theme() {
        let visuals = widget(Color32::WHITE, Color32::BLACK, Color32::GRAY, 7);
        assert_eq!(visuals.corner_radius, CornerRadius::same(7));
    }

    #[test]
    fn apply_to_context_uses_theme_mode_and_radius() {
        let mut theme = ThemeTokens::light();
        theme.background = Color32::from_rgb(9, 9, 11);
        let context = Context::default();

        theme.apply_to_context(&context);

        let visuals = context.style().visuals.clone();
        assert!(!visuals.dark_mode);
        for widget in [
            visuals.widgets.noninteractive,
            visuals.widgets.inactive,
            visuals.widgets.hovered,
            visuals.widgets.active,
            visuals.widgets.open,
        ] {
            assert_eq!(widget.corner_radius, CornerRadius::same(theme.radius.md));
        }
    }
}
