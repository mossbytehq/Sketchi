//! Short-lived, stacked notifications for the desktop workspace.

use std::time::{Duration, Instant};

use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Layout, Margin, Order, RichText, Stroke,
    TextWrapMode, Vec2,
};

use crate::{
    components::close_icon_button,
    lucide_icons::{self, LucideIcon as Icon},
    theme::ThemeTokens,
};

const TOAST_DURATION: Duration = Duration::from_secs(5);
const TOAST_WIDTH: f32 = 360.0;
const TOAST_OFFSET: Vec2 = Vec2::new(-16.0, -68.0);
const TOAST_GAP: f32 = 8.0;

/// Visual urgency used by a toast notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToastKind {
    /// A completed action or successful state transition.
    Success,
    /// A useful update that does not require immediate action.
    Info,
    /// A non-fatal condition that deserves attention.
    Warning,
    /// An action or background operation failed.
    Error,
}

impl ToastKind {
    fn accent(self, tokens: ThemeTokens) -> Color32 {
        match self {
            Self::Success => Color32::from_rgb(43, 145, 112),
            Self::Info => tokens.primary,
            Self::Warning => Color32::from_rgb(202, 132, 42),
            Self::Error => tokens.destructive,
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::Success => Icon::Success,
            Self::Info => Icon::Information,
            Self::Warning => Icon::Warning,
            Self::Error => Icon::Error,
        }
    }
}

#[derive(Clone, Debug)]
struct Toast {
    id: u64,
    kind: ToastKind,
    title: String,
    message: String,
    expires_at: Instant,
}

/// Owns active notifications and removes them after their display duration.
#[derive(Debug)]
pub(crate) struct ToastQueue {
    next_id: u64,
    toasts: Vec<Toast>,
}

impl Default for ToastQueue {
    fn default() -> Self {
        Self {
            next_id: 1,
            toasts: Vec::new(),
        }
    }
}

impl ToastQueue {
    /// Adds a notification and returns its stable dismissal ID.
    pub(crate) fn push(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> u64 {
        let title = title.into();
        let message = message.into();
        if let Some(existing) = self
            .toasts
            .iter()
            .find(|toast| toast.kind == kind && toast.title == title && toast.message == message)
        {
            return existing.id;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        self.toasts.push(Toast {
            id,
            kind,
            title,
            message,
            expires_at: Instant::now() + TOAST_DURATION,
        });
        id
    }

    /// Draws the active stack and schedules repainting for expiration.
    pub(crate) fn show(&mut self, context: &egui::Context, dark_mode: bool) {
        let now = Instant::now();
        self.toasts.retain(|toast| toast.expires_at > now);
        if let Some(next_expiration) = self
            .toasts
            .iter()
            .map(|toast| toast.expires_at.saturating_duration_since(now))
            .min()
        {
            context.request_repaint_after(next_expiration);
        }
        if self.toasts.is_empty() {
            return;
        }

        let tokens = ThemeTokens::for_dark_mode(dark_mode);
        let mut dismissed = None;
        egui::Area::new(Id::new("sketchi.toasts"))
            .anchor(Align2::RIGHT_BOTTOM, TOAST_OFFSET)
            .order(Order::Foreground)
            .interactable(true)
            .show(context, |ui| {
                // Keep the area stable while messages of different lengths are
                // added and removed, avoiding the shrinking behavior of egui's
                // auto-sized areas.
                ui.set_width(TOAST_WIDTH);
                ui.spacing_mut().item_spacing.y = TOAST_GAP;
                for toast in &self.toasts {
                    let accent = toast.kind.accent(tokens);
                    egui::Frame::new()
                        .fill(tokens.card)
                        .stroke(Stroke::new(1.0, tokens.border))
                        .corner_radius(CornerRadius::same(tokens.radius.lg))
                        .inner_margin(Margin::symmetric(12, 10))
                        .shadow(egui::Shadow {
                            offset: [0, 4],
                            blur: 16,
                            spread: 1,
                            color: Color32::from_rgba_unmultiplied(
                                0,
                                0,
                                0,
                                if dark_mode { 72 } else { 32 },
                            ),
                        })
                        .show(ui, |ui| {
                            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 9.0;
                                paint_toast_badge(ui, toast.kind, accent, tokens, dark_mode);
                                ui.vertical(|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&toast.title)
                                                .strong()
                                                .color(tokens.foreground),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if close_icon_button(
                                                    ui,
                                                    18.0,
                                                    tokens.muted_foreground,
                                                    tokens.destructive,
                                                )
                                                .clicked()
                                                {
                                                    dismissed = Some(toast.id);
                                                }
                                            },
                                        );
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&toast.message)
                                                .small()
                                                .color(tokens.muted_foreground),
                                        )
                                        .wrap_mode(TextWrapMode::Wrap),
                                    );
                                });
                            });
                        });
                }
            });
        if let Some(id) = dismissed {
            self.toasts.retain(|toast| toast.id != id);
        }
    }

    #[cfg(test)]
    fn retain_active_at(&mut self, now: Instant) {
        self.toasts.retain(|toast| toast.expires_at > now);
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.toasts.len()
    }
}

fn paint_toast_badge(
    ui: &mut egui::Ui,
    kind: ToastKind,
    accent: Color32,
    tokens: ThemeTokens,
    dark_mode: bool,
) {
    let badge_size = Vec2::splat(26.0);
    let (badge_rect, _) = ui.allocate_exact_size(badge_size, egui::Sense::hover());
    ui.painter().rect_filled(
        badge_rect,
        CornerRadius::same(tokens.radius.md),
        Color32::from_rgba_unmultiplied(
            accent.r(),
            accent.g(),
            accent.b(),
            if dark_mode { 36 } else { 24 },
        ),
    );
    ui.painter().text(
        badge_rect.center(),
        Align2::CENTER_CENTER,
        kind.icon().glyph().to_string(),
        FontId::new(
            15.0,
            egui::FontFamily::Name(lucide_icons::FONT_FAMILY.into()),
        ),
        accent,
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{TOAST_DURATION, ToastKind, ToastQueue};

    #[test]
    fn keeps_multiple_notifications_until_their_expiry() {
        let mut queue = ToastQueue::default();
        queue.push(ToastKind::Success, "Room created", "Ready to collaborate.");
        queue.push(
            ToastKind::Info,
            "Update available",
            "Version 1.2.0 is ready.",
        );

        assert_eq!(queue.len(), 2);
        queue.retain_active_at(std::time::Instant::now() + TOAST_DURATION);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn retains_notifications_before_their_expiry() {
        let mut queue = ToastQueue::default();
        queue.push(ToastKind::Warning, "Warning", "This needs attention.");

        queue.retain_active_at(std::time::Instant::now() + Duration::from_secs(1));

        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn deduplicates_identical_active_notifications() {
        let mut queue = ToastQueue::default();
        let first = queue.push(ToastKind::Error, "Error", "Something failed.");
        let second = queue.push(ToastKind::Error, "Error", "Something failed.");

        assert_eq!(first, second);
        assert_eq!(queue.len(), 1);
    }
}
