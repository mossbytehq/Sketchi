# Settings Border Hierarchy

## Goal

Make the native settings window read as one coherent desktop surface, matching the reference image:

- one rounded outer settings shell;
- rounded settings cards with restrained inner borders;
- one intentional sidebar divider;
- no duplicate or full-height borders created by nested frames.

## Scope

This change is limited to the settings window presentation in `canvas-client`.
It does not change settings state, persistence, controls, navigation, scrollbar behavior, native window decorations, or footer actions.

## Design

### Root shell

`settings_window_frame` owns the settings client-area surface. It will provide the background fill, a single subtle border, and the outer corner radius. Its content margin remains zero so the sidebar/content layout can control its own insets.

### Cards

`settings_group_frame` remains the reusable card surface for page sections. Cards keep rounded corners and use a lower-contrast border than the root shell, so they are visibly grouped without looking like nested windows.

### Sidebar divider

The sidebar/content boundary will be rendered as one explicit, inset vertical divider. Layout spacing will provide the gutter; no additional full-height separator or frame stroke will be used for that boundary.

### Rendering ownership

Each visible border has one owner:

| Surface | Owner | Border |
| --- | --- | --- |
| Settings client area | `settings_window_frame` | One rounded outer border |
| Settings section | `settings_group_frame` | One softer rounded card border |
| Sidebar/content boundary | Settings layout | One inset vertical divider |
| Controls | Shared components / egui widget visuals | Control-specific border only |

## Error handling

No new runtime failure path is introduced. The change only adjusts egui frame and painter configuration.

## Verification

- Add a focused helper-level assertion where practical for root/card frame styling.
- Run `cargo fmt --all --check`.
- Run `git diff --check`.
- Run `cargo test -p canvas-client`.
- Run `cargo clippy --workspace --all-targets -- -D warnings`.

