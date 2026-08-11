# Selection and Diamond Design

## Goal

Add an Excalidraw-style selection interaction and a durable diamond drawing
tool without bypassing Sketchi's operation-based document model.

## Decisions

- `ElementKind::Diamond` is a core document kind. It uses the existing
  transform, style, rotation, operation, CRDT, snapshot, and protocol paths.
- A selection is an ordered-by-ID set of element IDs held only by the client.
  Click selects one element, Shift-click toggles one element, and clicking
  empty canvas clears the set unless Shift is held.
- Dragging empty canvas creates a world-space marquee. On release, elements
  whose selection bounds intersect the marquee are selected; without Shift the
  previous selection is replaced, and with Shift the matching IDs are toggled.
- Dragging any selected element previews and commits a common world-space
  translation for every selected element. Each final position is emitted as a
  normal `SetPosition` operation, so the existing editor history and sync path
  remain authoritative.
- A single selected element displays eight square resize handles and a
  rotation handle connected above the top edge. Resize and rotation are
  committed through `SetPosition`, `SetSize`, and `SetRotation` operations.
  Multi-selection displays one bounds box and supports movement only in this
  slice; group resizing/rotation is deliberately deferred.
- Rotation is around the element center. Bounded shapes (rectangle, ellipse,
  diamond, and image) render from rotated geometry, and hit testing transforms
  the query into the element's local frame. Text and path elements retain
  their existing visual behavior while still supporting selection movement.
- The diamond is defined by the midpoint of each transformed edge: top,
  right, bottom, and left. Its fill and stroke follow the current style.

## Verification

- Core tests validate diamond construction, operation acceptance, and snapshot
  round trips.
- Renderer tests validate diamond scene extraction and hit testing.
- Client tool tests validate diamond preview and commit behavior.
- Client pure selection-geometry tests validate marquee intersection, bounds,
  handle positions, rotation angles, and resize calculations.
- The final gate remains `cargo fmt --all -- --check`, workspace clippy with
  `-D warnings`, and the full locked workspace test suite.
