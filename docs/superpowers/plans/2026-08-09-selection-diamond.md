# Selection and Diamond Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add durable diamond drawing plus multi-selection, marquee selection, resize handles, rotation control, and group movement to the Sketchi desktop client.

**Architecture:** `canvas-core` adds only the durable diamond kind and constructor. `canvas-renderer` derives a diamond primitive and performs geometry-aware hit testing. `canvas-client` owns the ephemeral selection set and gesture state, translates completed gestures into ordinary editor commands, and paints selection affordances.

**Tech Stack:** Rust 1.97.1, `canvas-core`, `canvas-renderer`, `egui`, existing `EditorCommand`/CRDT operation path, and existing winit pointer input.

## Global Constraints

- `canvas-core` remains free of UI, GPU, async, networking, filesystem, and persistence dependencies.
- Every document mutation is applied through `CrdtDocument::apply` via `EditorCommand`.
- Selection state and gestures are ephemeral client state and never enter snapshots or protocol messages.
- Same-property operation ordering, duplicate operation idempotence, and snapshot determinism remain unchanged.
- New behavior is implemented test-first and verified with the locked workspace gate.

---

### Task 1: Add the durable diamond kind

**Files:**
- Modify: `crates/canvas-core/src/element.rs`
- Modify: `crates/canvas-core/tests/operations.rs`
- Modify: `crates/canvas-renderer/src/geometry.rs`
- Modify: `crates/canvas-renderer/tests/geometry.rs`

**Interfaces:** Add `ElementKind::Diamond`, `Element::diamond(ElementId, Transform)`, `RenderPrimitive::Diamond { id, rect, style, rotation }`, and diamond-aware `hit_test`.

- [ ] **Step 1: Write the failing core test**

```rust
#[test]
fn diamond_create_survives_crdt_and_snapshot_round_trip() {
    let id = ElementId::from_u128(18);
    let operation = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: Element::diamond(
                id,
                Transform::new(Point::new(10.0, 20.0), Size::new(80.0, 60.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();
    assert_eq!(document.document().element(id).unwrap().kind, ElementKind::Diamond);
    let restored = CrdtDocument::from_snapshot(document.snapshot()).unwrap();
    assert_eq!(restored.document(), document.document());
}
```

- [ ] **Step 2: Run the core test and confirm RED**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-core --test operations diamond_create_survives_crdt_and_snapshot_round_trip --offline`

Expected: compilation failure because `ElementKind::Diamond` and
`Element::diamond` do not exist.

- [ ] **Step 3: Implement the minimum core kind and constructor**

Add the enum variant and constructor using `Element::new(id,
ElementKind::Diamond, transform)`. No new operation kind or CRDT register is
needed because `kind` is already a durable register.

- [ ] **Step 4: Write the failing renderer test**

```rust
#[test]
fn renderer_extracts_and_hits_a_diamond() {
    let id = ElementId::from_u128(19);
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::diamond(
                id,
                Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 80.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();
    let scene = Renderer::new(Camera::new(Size::new(800.0, 600.0))).draw(&document.document());
    assert!(matches!(scene.primitives().next(), Some(RenderPrimitive::Diamond { id: found, .. }) if *found == id));
    assert_eq!(hit_test(&document.document(), Point::new(60.0, 60.0), 0.0), Some(id));
}
```

- [ ] **Step 5: Run the renderer test and confirm RED**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-renderer --test geometry renderer_extracts_and_hits_a_diamond --offline`

Expected: compilation failure because the renderer has no diamond primitive
or hit-test branch.

- [ ] **Step 6: Implement diamond scene extraction and hit testing**

Use the transform rectangle's four edge midpoints for rendering and a local
diamond inequality (`abs(x / half_width) + abs(y / half_height) <= 1`) for hit
testing, with the existing tolerance applied to the half extents. Preserve
the existing z-order traversal.

- [ ] **Step 7: Run focused core and renderer tests**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-core -p canvas-renderer --tests --offline`

Expected: all focused tests pass.

### Task 2: Add the diamond client tool

**Files:**
- Modify: `apps/canvas-client/src/tools.rs`
- Modify: `apps/canvas-client/src/ui.rs`
- Modify: `apps/canvas-client/tests/tools.rs`

**Interfaces:** Add `Tool::Diamond`, map it to `ElementKind::Diamond`, add the Remix diamond icon entry to the toolbar, and include it in tool naming/cursor behavior.

- [ ] **Step 1: Write the failing client tool test**

```rust
#[test]
fn diamond_tool_commits_a_diamond_element() {
    let id = ElementId::from_u128(20);
    let mut tools = ToolController::new(Tool::Diamond);
    tools.pointer_down(id, Point::new(100.0, 100.0));
    tools.pointer_move(Point::new(40.0, 20.0));
    let Some(ToolOutput::Command(EditorCommand::Create(element))) =
        tools.pointer_up(Point::new(40.0, 20.0))
    else {
        panic!("diamond tool did not create an element");
    };
    assert_eq!(element.kind, ElementKind::Diamond);
    assert_eq!(element.transform.position, Point::new(40.0, 20.0));
    assert_eq!(element.transform.size, canvas_core::Size::new(60.0, 80.0));
}
```

- [ ] **Step 2: Run the test and confirm RED**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --test tools diamond_tool_commits_a_diamond_element --offline`

Expected: compilation failure because `Tool::Diamond` does not exist.

- [ ] **Step 3: Implement the minimum tool and toolbar wiring**

Add the tool branch alongside rectangle and ellipse in `preview`, add a
diamond Remix icon mapping, and include the name in `tool_name` and
`canvas_cursor`.

- [ ] **Step 4: Run client tool tests**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --test tools --offline`

Expected: all client tool tests pass.

### Task 3: Build pure selection geometry and gesture state

**Files:**
- Create: `apps/canvas-client/src/selection.rs`
- Modify: `apps/canvas-client/src/lib.rs`
- Modify: `apps/canvas-client/src/ui.rs`
- Create: `apps/canvas-client/tests/selection.rs`

**Interfaces:** Define `SelectionBounds`, `SelectionHandle`, `Marquee`, `selection_bounds`, `marquee_intersects`, `handle_positions`, `rotation_handle_position`, and `resize_transform`. Keep these functions independent of egui painting so they can be tested with core geometry.

- [ ] **Step 1: Write failing selection geometry tests**

```rust
#[test]
fn marquee_intersection_uses_element_bounds() {
    let element = Element::rectangle(
        ElementId::from_u128(21),
        Transform::new(Point::new(40.0, 40.0), Size::new(30.0, 20.0)),
    );
    assert!(marquee_intersects(&element, Rect::new(Point::new(60.0, 50.0), Size::new(20.0, 20.0))));
    assert!(!marquee_intersects(&element, Rect::new(Point::new(100.0, 100.0), Size::new(20.0, 20.0))));
}

#[test]
fn resize_from_bottom_right_preserves_the_opposite_anchor() {
    let element = Element::rectangle(
        ElementId::from_u128(22),
        Transform::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0)),
    );
    let resized = resize_transform(&element, SelectionHandle::BottomRight, Point::new(80.0, 90.0));
    assert_eq!(resized.position, Point::new(10.0, 20.0));
    assert_eq!(resized.size, Size::new(70.0, 70.0));
}
```

- [ ] **Step 2: Run the tests and confirm RED**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --test selection --offline`

Expected: compilation failure because the selection module and helpers do not
exist.

- [ ] **Step 3: Implement deterministic selection helpers**

Normalize marquee rectangles, use inclusive AABB intersection, clamp resized
dimensions to 4 world units, and return handle positions in the stable order
top-left, top, top-right, right, bottom-right, bottom, bottom-left, left.

- [ ] **Step 4: Add client selection state**

Replace `selected: Option<ElementId>` with `BTreeSet<ElementId>`. Add
`SelectionGesture` variants for `Marquee`, `Move`, `Resize`, and `Rotate`, each
holding the original selected elements and pointer anchor needed to produce a
preview. On release, emit one normal editor command per changed element.

- [ ] **Step 5: Run selection helper tests**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --test selection --offline`

Expected: all selection helper tests pass.

### Task 4: Paint and wire the Excalidraw-style selection UI

**Files:**
- Modify: `apps/canvas-client/src/ui.rs`
- Modify: `apps/canvas-client/src/remix_icons.rs`

**Interfaces:** Paint selected-element bounds, eight square handles, a connector line, rotation handle, and a translucent marquee; route pointer modifiers and drag gestures through the selection state.

- [ ] **Step 1: Write failing pure UI tests for rotation and group translation**

```rust
#[test]
fn rotation_delta_is_measured_from_the_selection_center() {
    let center = Point::new(50.0, 50.0);
    assert!((rotation_delta(center, Point::new(70.0, 50.0), Point::new(50.0, 70.0))
        - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
}

#[test]
fn group_translation_moves_each_original_element_by_the_same_delta() {
    let moved = translate_element(
        &Element::rectangle(ElementId::from_u128(23), Transform::new(Point::new(10.0, 20.0), Size::new(10.0, 10.0))),
        Point::new(5.0, -3.0),
    );
    assert_eq!(moved.transform.position, Point::new(15.0, 17.0));
}
```

- [ ] **Step 2: Run the tests and confirm RED**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --lib rotation_delta_is_measured_from_the_selection_center --offline`

Expected: compilation failure because the helpers do not exist.

- [ ] **Step 3: Implement selection painting and pointer priority**

Check rotation handle and resize handles before element hit testing, then
selected elements, then marquee start. Use Shift from `ui.input` for toggles.
Paint the marquee in accent color with a low-alpha fill and use square,
non-rounded handles consistent with Excalidraw.

- [ ] **Step 4: Implement move, resize, and rotate commits**

Preview all selected elements during a drag. On release, skip unchanged
properties and execute `SetPosition`, `SetSize`, and `SetRotation` commands
through `Editor`. Restore the selection after each command and keep style
properties tied to the single selected element only.

- [ ] **Step 5: Run the client library tests**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p canvas-client --lib --offline`

Expected: all UI, image, selection, and helper tests pass.

### Task 5: Verify and relaunch

**Files:**
- Modify: `docs/protocol.md` only if the public element/tool description needs an update.

- [ ] **Step 1: Format and lint**

Run: `RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check` and
`RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`.

- [ ] **Step 2: Run the full test suite**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace --all-targets --locked --offline`.

If the WebSocket suite cannot bind loopback inside the sandbox, rerun the
same command with host networking permission and record that result.

- [ ] **Step 3: Relaunch the final application**

Run: `RUST_LOG=info cargo run -p canvas-client --locked` from the workspace
root with GUI access, then verify the window starts and the terminal reports
the first GPU frame.
