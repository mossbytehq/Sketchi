# Sketchi

Sketchi is a fast, native whiteboard for Windows and Linux. Sketch shapes,
arrows, freehand drawings, text, and images on an infinite canvas, and invite
up to three other people to draw with you in real time.

> Sketchi is in active development. Every release is currently published as a
> pre-release, so expect rough edges and frequent updates.

## Features

- **Drawing tools:** freehand pen, rectangles, diamonds, triangles, pentagons,
  hexagons, ellipses, straight and curved lines, straight and curved arrows,
  and text.
- **Images:** drag a PNG or JPEG onto the canvas, or paste one from the
  clipboard.
- **Styling:** stroke and fill colours, fill styles, stroke width and style,
  sloppiness, rounded edges, opacity, and fonts.
- **Editing:** select, move, resize, rotate, copy, paste, duplicate, and undo
  and redo (up to 64 steps).
- **Live collaboration:** see other people's edits, cursors, and selections as
  they happen.
- **Your files stay local:** documents are saved as plain JSON files, with
  optional autosave.
- **Built-in updates:** Sketchi can check for, download, and install new
  versions for you.

## Download and install

Download the latest version from the
[Releases page](https://github.com/mossbytehq/Sketchi/releases). Each release
contains a `SHA256SUMS` file if you want to verify your download.

| Platform | File | Notes |
| --- | --- | --- |
| Windows (recommended) | `Sketchi-<version>-windows-x86_64-setup.exe` | Installs for all users and adds a Start menu shortcut. |
| Windows (MSI) | `Sketchi-<version>-windows-x86_64-setup.msi` | For managed or scripted installs. |
| Windows (portable) | `Sketchi-<version>-windows-x86_64.zip` | Unzip anywhere and run `Sketchi.exe`. No installation needed. |
| Debian / Ubuntu | `sketchi_<version>_amd64.deb` | `sudo apt install ./sketchi_<version>_amd64.deb` |
| Fedora / openSUSE | `sketchi-<version>-1.x86_64.rpm` | `sudo dnf install ./sketchi-<version>-1.x86_64.rpm` |
| Arch Linux | `sketchi-<version>-1-x86_64.pkg.tar.zst` | `sudo pacman -U sketchi-<version>-1-x86_64.pkg.tar.zst` |
| Any Linux (portable) | `Sketchi-<version>-linux-x86_64` | `chmod +x` the file, then run it. |

Only 64-bit (x86_64) systems are supported.

**Windows SmartScreen:** releases are not code-signed yet, so Windows may show
"Windows protected your PC" the first time you run the installer. Choose
**More info → Run anyway** to continue.

## Drawing

Pick a tool from the toolbar or press its shortcut, then drag on the canvas.
Scroll to zoom. To move around, drag with the middle mouse button or use the
hand tool.

| Action | Shortcut |
| --- | --- |
| Select | `V` |
| Text | `T` |
| Freehand pen | `P` |
| Rectangle / Diamond / Ellipse | `R` / `D` / `O` |
| Triangle / Pentagon / Hexagon | `Y` / `G` / `B` |
| Line / Arrow | `L` / `A` |
| Curved line (clockwise / counter-clockwise) | `J` / `K` |
| Curved arrow (clockwise / counter-clockwise) | `C` / `X` |
| Hand (pan) | `H` |
| Select all / Copy / Paste / Duplicate | `Ctrl+A` / `Ctrl+C` / `Ctrl+V` / `Ctrl+D` |
| Delete selection | `Backspace` |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Y` |
| New / Save / Save as | `Ctrl+N` / `Ctrl+S` / `Ctrl+Shift+S` |
| Settings | `Ctrl+,` |

Every shortcut can be changed under **Settings**.

## Saving your work

Use **Save** or **Save as** to keep a drawing as a `.json` document, and
**Open** to load it again. Sketchi also autosaves a recovery copy of your
canvas; you can change how often (or turn it off) in **Settings**.

## Drawing together

Sketchi includes its own collaboration server, so one person can host a room
directly from the app.

### Host a room

1. Click the **Create room** button in the top-right toolbar and enter your
   display name.
2. Once the room is ready, click the copy button to copy the invite.
3. Send the invite to the people you want to draw with.

Your current canvas becomes the room's starting canvas. Use the cancel button
on the room panel to end the room for everyone.

### Join a room

1. Click **Join room** and enter your display name.
2. Paste the invite you received and click **Join room**.

If your own canvas already has drawings on it, Sketchi asks before joining,
because your drawings will be added to the room and everyone in it will be able
to see and edit them. Start a new whiteboard first (`Ctrl+N`) if you only want
to see the room's canvas.

### Good to know

- A room holds up to **4 people**, including the host.
- An invite admits new people for **2 hours**. Anyone who has already joined,
  and the host, can keep reconnecting after that.
- If your connection drops, Sketchi reconnects automatically and catches up on
  anything you missed.
- **Everyone must be able to reach the host's computer.** Out of the box this
  means the same home, office, or school network. To collaborate from
  different places, connect everyone to a shared virtual network such as
  [Tailscale](https://tailscale.com/) or [ZeroTier](https://www.zerotier.com/),
  or run a standalone server (see below).
- On Windows, allow Sketchi through the firewall if asked, or others will not
  be able to join your room.

### Hosting a standalone server

For collaborators in different locations, run the server on a machine everyone
can reach, such as a VPS. It needs a TLS certificate and private key in PEM
format:

```sh
sketchi-server --bind 0.0.0.0:3210 --certificate cert.pem --private-key key.pem
```

On Windows the server is `Sketchi-server.exe`, installed next to the app; you
can also build it from source (see below). When creating or joining a room,
open **Advanced** and enter the server's `wss://<host>:3210/ws` address and the
SHA-256 fingerprint of its certificate.

## Updates

Open **Settings → About** to check for new versions and install them with one
click. Sketchi restarts itself to finish the update, and reopens even if the
update fails.

There are two update channels:

- **Stable** receives full releases only.
- **Edge** also receives pre-releases.

While Sketchi is in development, every release is a pre-release, so choose
**Edge** to receive updates.

## Building from source

Sketchi is written in Rust. Install [rustup](https://rustup.rs/); the
repository pins the exact toolchain it needs, so rustup installs it
automatically on first build.

```sh
git clone https://github.com/mossbytehq/Sketchi.git
cd Sketchi
cargo sketchi
```

`cargo sketchi` builds and starts the desktop app. To run the collaboration
server on its own:

```sh
cargo run --package canvas-server --bin sketchi-server
```

Set `RUST_LOG=info` to see startup and runtime diagnostics.

### For contributors

Run the full check before opening a pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

The workspace is split into small crates:

```text
crates/canvas-core       document model and operation-based CRDT
crates/canvas-protocol   versioned collaboration messages
crates/canvas-renderer   camera and geometry/rendering support
apps/canvas-client       Sketchi desktop app
bin/canvas-server        collaboration server
xtask                    packaging and release checks
```

Releases are built by the GitHub workflows in `.github/workflows`. To change the
version, run `cargo xtask set-version --workspace <version>`. Add `--d` to make
the existing GitHub release a draft, `--rc` to mark it as a pre-release, or
`--r` to publish it as the latest stable release.

## License

Sketchi is released under the [MIT License](LICENSE.md).
