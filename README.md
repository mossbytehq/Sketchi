# Sketchi

Sketchi is a fast, native whiteboard for Windows and Linux. Sketch shapes,
arrows, freehand drawings, text, and images on an infinite canvas, and invite
up to three other people to draw with you in real time.

Sketchi is in active development, so expect rough edges and frequent updates.

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
- **Local documents:** save drawings as plain JSON files, with optional
  autosave. Joining a room shares your canvas with that room.
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
| Fedora | `sketchi-<version>-1.x86_64.rpm` | `sudo dnf install ./sketchi-<version>-1.x86_64.rpm` |
| openSUSE | `sketchi-<version>-1.x86_64.rpm` | `sudo zypper install ./sketchi-<version>-1.x86_64.rpm` |
| Arch Linux | `sketchi-<version>-1-x86_64.pkg.tar.zst` | `sudo pacman -U sketchi-<version>-1-x86_64.pkg.tar.zst` |
| Linux (portable) | `Sketchi-<version>-linux-x86_64` | `chmod +x` the file, then run it. |

Only 64-bit (x86_64) systems are supported.

**Windows SmartScreen:** unsigned downloads may show "Windows protected your
PC". Confirm that the file came from this repository's Releases page before
choosing **More info → Run anyway**.

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
  or [host a standalone server](DEVELOPMENT.md#hosting-a-standalone-server).
- On Windows, allow Sketchi through the firewall if asked, or others will not
  be able to join your room.

## Updates

Open **Settings → About** to check for new versions and install an available
update. Sketchi restarts itself to finish the update, and reopens even if the
update fails.

There are two update channels:

- **Stable** receives full releases only.
- **Edge** also receives pre-releases.

Choose **Edge** if you want to try pre-releases as well as stable releases.

## Privacy and sharing

No account is needed. Documents, settings, and recovery data are stored on
your computer. When you join or host a room, canvas content, including
embedded images, is shared through the collaboration server with the room's
participants. Use invites only with people you trust.

Update checks contact GitHub, and installing an update downloads release
files from GitHub. The host receives the network information normally sent
with a request, including your public IP address.

## Help and feedback

If someone cannot join your room, check that they can reach the host, that
the firewall allows Sketchi, and that the invite has not expired. Keep a
saved copy of your drawing before trying a new release.

[Report a problem or request a feature](https://github.com/mossbytehq/Sketchi/issues).
Include your operating system, Sketchi version, and steps to reproduce the
problem. Remove room invites, capability tokens, private drawings, and other
personal information from screenshots and logs before posting.

For building, contributing, and releasing, see the
[development guide](DEVELOPMENT.md).

## License

Sketchi is released under the [MIT License](LICENSE.md).
