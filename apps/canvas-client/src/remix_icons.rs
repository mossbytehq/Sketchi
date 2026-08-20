//! The small Remix Icon subset used by the desktop workspace.

/// Glyphs used by Sketchi's workspace controls.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum RemixIcon {
    New,
    Import,
    Save,
    Information,
    InputMethod,
    Router,
    Connector,
    Keyboard,
    ListSettings,
    Settings,
    Select,
    InputCursorMove,
    Freehand,
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
    LayerBringForward,
    LayerBringToFront,
    LayerSendBackward,
    LayerSendToBack,
    Duplicate,
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

impl RemixIcon {
    /// Returns the codepoint from the bundled Remix Icon font.
    pub(crate) const fn glyph(self) -> char {
        match self {
            Self::New => '\u{ea13}',
            Self::Import => '\u{f446}',
            Self::Save => '\u{f0b3}',
            Self::Information => '\u{ee59}',
            Self::InputMethod => '\u{ee60}',
            Self::Router => '\u{f09d}',
            Self::Connector => '\u{f69d}',
            Self::Keyboard => '\u{ee75}',
            Self::ListSettings => '\u{eebd}',
            Self::Settings => '\u{f0ee}',
            Self::Select => '\u{ec0a}',
            Self::InputCursorMove => '\u{ee5e}',
            Self::Freehand => '\u{efe0}',
            Self::PenNib => '\u{efde}',
            Self::QuillPen => '\u{f04a}',
            Self::Brush => '\u{eb01}',
            Self::Rectangle => '\u{f3d6}',
            Self::PokerDiamonds => '\u{f5c5}',
            Self::Rounded => '\u{f099}',
            Self::Ellipse => '\u{f3c1}',
            Self::Line => '\u{f1af}',
            Self::ArrowLeftDownLong => '\u{f5d3}',
            Self::ArrowDownS => '\u{ea4e}',
            Self::ArrowUpS => '\u{ea78}',
            Self::Pan => '\u{f444}',
            Self::Triangle => '\u{f3e4}',
            Self::Undo => '\u{ea58}',
            Self::Redo => '\u{ea5a}',
            Self::Sun => '\u{f1bf}',
            Self::ZoomOut => '\u{f2dd}',
            Self::ZoomIn => '\u{f2db}',
            Self::FillNone => '\u{ed93}',
            Self::FillSolid => '\u{eb7e}',
            Self::LayerBringForward => '\u{ea76}',
            Self::LayerBringToFront => '\u{f2eb}',
            Self::LayerSendBackward => '\u{ea4c}',
            Self::LayerSendToBack => '\u{f2e1}',
            Self::Duplicate => '\u{ecd5}',
            Self::Delete => '\u{ec2a}',
            Self::Link => '\u{eeb2}',
            Self::AlignItemBottom => '\u{f4b9}',
            Self::AlignItemHorizontalCenter => '\u{f4bb}',
            Self::AlignItemLeft => '\u{f4bd}',
            Self::AlignItemRight => '\u{f4bf}',
            Self::AlignItemTop => '\u{f4c1}',
            Self::AlignItemVerticalCenter => '\u{f4c3}',
            Self::TextAlignCenter => '\u{ea25}',
            Self::TextAlignLeft => '\u{ea27}',
            Self::TextAlignRight => '\u{ea28}',
            Self::Code => '\u{ebab}',
            Self::FontSans => '\u{ee03}',
            Self::CustomSize => '\u{f572}',
            Self::ContractUpDown => '\u{f303}',
            Self::ExpandUpDown => '\u{f327}',
        }
    }
}

pub(crate) const FONT_FAMILY: &str = "sketchi-remix-icons";
pub(crate) const UI_FONT_FAMILY: &str = "sketchi-ui";
pub(crate) const HANDWRITTEN_FONT_FAMILY: &str = "sketchi-handwritten";

/// Installs Sketchi's bundled UI, icon, and handwritten font families.
pub(crate) fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FONT_FAMILY.to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/remixicon.ttf")).into(),
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
    use super::RemixIcon;

    #[test]
    fn drawing_tool_icons_use_the_requested_remix_glyphs() {
        assert_eq!(RemixIcon::ArrowLeftDownLong.glyph(), '\u{f5d3}');
        assert_eq!(RemixIcon::PenNib.glyph(), '\u{efde}');
        assert_eq!(RemixIcon::QuillPen.glyph(), '\u{f04a}');
        assert_eq!(RemixIcon::Brush.glyph(), '\u{eb01}');
        assert_eq!(RemixIcon::Pan.glyph(), '\u{f444}');
        assert_eq!(RemixIcon::Triangle.glyph(), '\u{f3e4}');
    }

    #[test]
    fn settings_navigation_icons_use_the_requested_remix_glyphs() {
        assert_eq!(RemixIcon::ListSettings.glyph(), '\u{eebd}');
        assert_eq!(RemixIcon::Keyboard.glyph(), '\u{ee75}');
        assert_eq!(RemixIcon::InputMethod.glyph(), '\u{ee60}');
        assert_eq!(RemixIcon::Information.glyph(), '\u{ee59}');
    }

    #[test]
    fn collaboration_icons_use_the_requested_remix_glyphs() {
        assert_eq!(RemixIcon::Router.glyph(), '\u{f09d}');
        assert_eq!(RemixIcon::Connector.glyph(), '\u{f69d}');
    }

    #[test]
    fn document_import_uses_the_remix_import_line_glyph() {
        assert_eq!(RemixIcon::Import.glyph(), '\u{f446}');
    }
}
