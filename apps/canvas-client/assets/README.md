# Lucide Icons

Sketchi uses the Lucide icon font supplied by the pinned `lucide-icons` crate
for its native toolbar and properties controls.

- Project: <https://github.com/lucide-icons/lucide>
- Website: <https://lucide.dev/>
- License: <https://github.com/lucide-icons/lucide/blob/main/LICENSE>

The application selects only the glyphs it needs through the semantic icon
mapping in `src/lucide_icons.rs`.

`Virgil.ttf` is the open-source handwriting font from
<https://github.com/excalidraw/virgil>, licensed under the SIL Open Font
License 1.1. It is used for the Handwritten text style.

`sketchi.png` is the application logo. The native desktop client embeds it for
the window icon; platform packaging keeps resized Linux and Windows variants
under `packaging/`.
