//! The small Lucide icon subset used by the desktop workspace.

use lucide_icons::{Icon, LUCIDE_FONT_BYTES};

/// Glyphs used by Sketchi's workspace controls.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum LucideIcon {
    New,
    Import,
    Save,
    RefreshCcw,
    Information,
    InputMethod,
    Router,
    Connector,
    CircleStop,
    Keyboard,
    ListSettings,
    Settings,
    Select,
    InputCursorMove,
    Freehand,
    HandDrawn,
    PenNib,
    QuillPen,
    Brush,
    Rectangle,
    PokerDiamonds,
    Triangle,
    Rounded,
    Ellipse,
    Line,
    ArrowLeftDownLong,
    ArrowDownS,
    ArrowUpS,
    Pan,
    Undo,
    Redo,
    Sun,
    ZoomOut,
    ZoomIn,
    FillNone,
    FillSolid,
    FillHachure,
    FillCrossHatch,
    LayerBringForward,
    LayerBringToFront,
    LayerSendBackward,
    LayerSendToBack,
    Duplicate,
    Clipboard,
    Delete,
    Link,
    AlignItemBottom,
    AlignItemHorizontalCenter,
    AlignItemLeft,
    AlignItemRight,
    AlignItemTop,
    AlignItemVerticalCenter,
    TextAlignCenter,
    TextAlignLeft,
    TextAlignRight,
    Code,
    FontSans,
    CustomSize,
    ContractUpDown,
    ExpandUpDown,
}

impl LucideIcon {
    /// Returns the corresponding Lucide icon.
    fn icon(self) -> Icon {
        match self {
            Self::New => Icon::Plus,
            Self::Import => Icon::HardDriveUpload,
            Self::Save => Icon::HardDriveDownload,
            Self::RefreshCcw => Icon::RefreshCcw,
            Self::Information => Icon::Info,
            Self::InputMethod => Icon::SlidersHorizontal,
            Self::Router => Icon::Network,
            Self::Connector => Icon::Waypoints,
            Self::CircleStop => Icon::CircleStop,
            Self::Keyboard => Icon::Keyboard,
            Self::ListSettings => Icon::Settings2,
            Self::Settings => Icon::Settings,
            Self::Select => Icon::MousePointer2,
            Self::InputCursorMove | Self::FontSans => Icon::Type,
            Self::Freehand => Icon::PenLine,
            Self::HandDrawn => Icon::PencilLine,
            Self::PenNib => Icon::PenTool,
            Self::QuillPen => Icon::Feather,
            Self::Brush => Icon::Paintbrush,
            Self::Rectangle => Icon::Square,
            Self::PokerDiamonds => Icon::Diamond,
            Self::Rounded => Icon::SquareRoundCorner,
            Self::Ellipse => Icon::Circle,
            Self::Line => Icon::Slash,
            Self::ArrowLeftDownLong => Icon::MoveDownLeft,
            Self::ArrowDownS | Self::ExpandUpDown => Icon::ChevronDown,
            Self::ArrowUpS | Self::ContractUpDown => Icon::ChevronUp,
            Self::Pan => Icon::Hand,
            Self::Triangle => Icon::Triangle,
            Self::Undo => Icon::Undo2,
            Self::Redo => Icon::Redo2,
            Self::Sun => Icon::Sun,
            Self::ZoomOut => Icon::ZoomOut,
            Self::ZoomIn => Icon::ZoomIn,
            Self::FillNone => Icon::CircleOff,
            Self::FillSolid => Icon::PaintBucket,
            Self::FillHachure => Icon::Columns4,
            Self::FillCrossHatch => Icon::Grid3x3,
            Self::LayerBringForward => Icon::LayerArrowUp,
            Self::LayerBringToFront => Icon::LayersArrowUp,
            Self::LayerSendBackward => Icon::LayerArrowDown,
            Self::LayerSendToBack => Icon::LayersArrowDown,
            Self::Duplicate => Icon::Copy,
            Self::Clipboard => Icon::Clipboard,
            Self::Delete => Icon::Trash2,
            Self::Link => Icon::Link,
            Self::AlignItemBottom => Icon::AlignEndVertical,
            Self::AlignItemHorizontalCenter => Icon::AlignCenterHorizontal,
            Self::AlignItemLeft => Icon::AlignStartHorizontal,
            Self::AlignItemRight => Icon::AlignEndHorizontal,
            Self::AlignItemTop => Icon::AlignStartVertical,
            Self::AlignItemVerticalCenter => Icon::AlignCenterVertical,
            Self::TextAlignCenter => Icon::TextAlignCenter,
            Self::TextAlignLeft => Icon::TextAlignStart,
            Self::TextAlignRight => Icon::TextAlignEnd,
            Self::Code => Icon::CodeXml,
            Self::CustomSize => Icon::Maximize2,
        }
    }

    /// Returns the codepoint from the bundled Lucide font.
    pub(crate) fn glyph(self) -> char {
        self.icon().unicode()
    }
}

pub(crate) const FONT_FAMILY: &str = "sketchi-lucide-icons";
pub(crate) const UI_FONT_FAMILY: &str = "sketchi-ui";
pub(crate) const HANDWRITTEN_FONT_FAMILY: &str = "sketchi-handwritten";

/// Installs Sketchi's bundled UI, Lucide icon, and handwritten font families.
pub(crate) fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FONT_FAMILY.to_owned(),
        egui::FontData::from_static(LUCIDE_FONT_BYTES).into(),
    );
    fonts.font_data.insert(
        UI_FONT_FAMILY.to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Inter.ttf")).into(),
    );
    fonts.font_data.insert(
        HANDWRITTEN_FONT_FAMILY.to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Virgil.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Name(FONT_FAMILY.into()))
        .or_default()
        .insert(0, FONT_FAMILY.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, UI_FONT_FAMILY.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Name(HANDWRITTEN_FONT_FAMILY.into()))
        .or_default()
        .insert(0, HANDWRITTEN_FONT_FAMILY.to_owned());
    context.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use lucide_icons::Icon;

    use super::LucideIcon;

    #[test]
    fn drawing_tool_icons_use_lucide_glyphs() {
        assert_eq!(
            LucideIcon::ArrowLeftDownLong.glyph(),
            Icon::MoveDownLeft.unicode()
        );
        assert_eq!(LucideIcon::Freehand.glyph(), Icon::PenLine.unicode());
        assert_eq!(LucideIcon::Line.glyph(), Icon::Slash.unicode());
        assert_eq!(LucideIcon::PenNib.glyph(), Icon::PenTool.unicode());
        assert_eq!(LucideIcon::QuillPen.glyph(), Icon::Feather.unicode());
        assert_eq!(LucideIcon::Brush.glyph(), Icon::Paintbrush.unicode());
        assert_eq!(LucideIcon::Pan.glyph(), Icon::Hand.unicode());
        assert_eq!(LucideIcon::Triangle.glyph(), Icon::Triangle.unicode());
    }

    #[test]
    fn settings_navigation_icons_use_lucide_glyphs() {
        assert_eq!(LucideIcon::ListSettings.glyph(), Icon::Settings2.unicode());
        assert_eq!(LucideIcon::Keyboard.glyph(), Icon::Keyboard.unicode());
        assert_eq!(
            LucideIcon::InputMethod.glyph(),
            Icon::SlidersHorizontal.unicode()
        );
        assert_eq!(LucideIcon::Information.glyph(), Icon::Info.unicode());
    }

    #[test]
    fn collaboration_icons_use_lucide_glyphs() {
        assert_eq!(LucideIcon::Router.glyph(), Icon::Network.unicode());
        assert_eq!(LucideIcon::Connector.glyph(), Icon::Waypoints.unicode());
    }

    #[test]
    fn clipboard_action_uses_the_clipboard_glyph() {
        assert_eq!(LucideIcon::Clipboard.glyph(), Icon::Clipboard.unicode());
    }

    #[test]
    fn layer_and_link_actions_use_directional_glyphs() {
        assert_eq!(
            LucideIcon::LayerSendToBack.glyph(),
            Icon::LayersArrowDown.unicode()
        );
        assert_eq!(
            LucideIcon::LayerSendBackward.glyph(),
            Icon::LayerArrowDown.unicode()
        );
        assert_eq!(
            LucideIcon::LayerBringForward.glyph(),
            Icon::LayerArrowUp.unicode()
        );
        assert_eq!(
            LucideIcon::LayerBringToFront.glyph(),
            Icon::LayersArrowUp.unicode()
        );
        assert_eq!(LucideIcon::Link.glyph(), Icon::Link.unicode());
    }

    #[test]
    fn document_actions_use_disk_transfer_glyphs() {
        assert_eq!(LucideIcon::New.glyph(), Icon::Plus.unicode());
        assert_eq!(LucideIcon::Import.glyph(), Icon::HardDriveUpload.unicode());
        assert_eq!(LucideIcon::Save.glyph(), Icon::HardDriveDownload.unicode());
        assert_eq!(LucideIcon::RefreshCcw.glyph(), Icon::RefreshCcw.unicode());
        assert_eq!(LucideIcon::InputCursorMove.glyph(), Icon::Type.unicode());
        assert_eq!(LucideIcon::HandDrawn.glyph(), Icon::PencilLine.unicode());
        assert_eq!(LucideIcon::Code.glyph(), Icon::CodeXml.unicode());
    }
}
