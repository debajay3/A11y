# Installing a11y

Download the file for your platform from the
[latest release](https://github.com/debajay3/A11y/releases):

| Platform | File |
|---|---|
| macOS (Apple Silicon and Intel) | `a11y-<version>-macos.dmg` (one universal file for both) |
| Windows (x64), installer | `a11y-<version>-x86_64-windows-setup.exe` (recommended) |
| Windows (x64), portable | `a11y-<version>-x86_64-pc-windows-msvc.zip` |
| Linux (x64) | `a11y-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS, for scripts and CLI use | `a11y-<version>-macos-universal.tar.gz` |

The Windows installer needs no administrator rights. The macOS disk image holds `a11y.app` (the editor, with the `a11y-diff` tool inside). The
Windows and Linux archives hold the `a11y` editor, the `a11y-diff` tool, the licenses and the
README. `SHA256SUMS` on the release page lets you check the download:

```sh
sha256sum -c SHA256SUMS --ignore-missing      # Linux
shasum -a 256 -c SHA256SUMS --ignore-missing  # macOS
```

## The first-launch warning

a11y's release builds are **not code-signed yet**, so Windows and macOS warn
the first time you open them. The warning is about the missing publisher
signature, not about anything the app does: the source is public and every
release is built by GitHub Actions from a tagged commit.

### Windows

**Installer (recommended).**

1. Download `a11y-<version>-x86_64-windows-setup.exe` and double-click it.
2. SmartScreen shows **"Windows protected your PC"**. Click **More info**, then **Run anyway**.
3. Follow the wizard. By default a11y installs for your user only, into
   `%LOCALAPPDATA%\Programs\a11y`, with no administrator prompt. Choose "Install for all
   users" when the wizard asks to put it in `C:\Program Files\a11y` instead.
4. Tick **Create a desktop shortcut** and **Open .a11y files with a11y** if you want them,
   and leave **Launch a11y** ticked to start it right away.

Running a newer installer later upgrades the existing install in place. To uninstall, use
**Settings → Apps → Installed apps → a11y → Uninstall**. Your design files are never touched.
For a silent install, run the setup with `/VERYSILENT /CURRENTUSER`. (The in-app update uses
`/SILENT` and adds `/RELAUNCH=1`, which makes the installer start a11y again when it is done.)

**Portable zip.** Unzip the archive anywhere and double-click `a11y.exe` (it carries the
a11y icon and version info). The SmartScreen steps are the same. Delete the folder to remove it.

**If Windows Defender quarantines the download or the exe.** Unsigned builds are occasionally
flagged by mistake. Open **Windows Security → Virus & threat protection → Protection history**,
restore the item and choose **Allow on device**, then check the file against `SHA256SUMS`
(`certutil -hashfile <file> SHA256`) before running it.

### macOS

One `.dmg` works on both Apple Silicon and Intel Macs (macOS 11 or newer).

1. Download `a11y-<version>-macos.dmg` from the release page.
2. Double-click the `.dmg` to open it.
3. Drag **a11y** onto the **Applications** shortcut in that window.
4. Open **Applications** and double-click **a11y** (you can eject the disk image now).
5. The first time, macOS says it cannot verify that a11y is free of malware. Click **Done**.
6. Open **System Settings → Privacy & Security** and scroll down to the message that
   "a11y" was blocked. Click **Open Anyway**.
7. Confirm with your password or Touch ID, then click **Open** in the last dialog.

That is only needed once; afterwards a11y opens like any other app. The same steps are
in `Read me first.txt` inside the disk image.

If you prefer Terminal, this replaces steps 5 to 7:

```sh
xattr -dr com.apple.quarantine /Applications/a11y.app
```

The app is ad-hoc signed but not notarized by Apple yet, which is why the one-time confirmation
is needed.

**Command-line tool.** `a11y-diff` lives inside the app:

```sh
/Applications/a11y.app/Contents/MacOS/a11y-diff --help
```

**Opening files.** `.a11y` and `.fig` files list a11y under **Open With** in Finder.
Double-clicking a file in Finder does not hand it to a running a11y yet (the windowing
library does not expose macOS "open file" events); use **File → Open**, drag the file onto the
window, or run `/Applications/a11y.app/Contents/MacOS/a11y path/to/file.a11y`.

`a11y-<version>-macos-universal.tar.gz` holds the same `a11y.app` plus the licenses, for
scripting; unpack it and run `xattr -dr com.apple.quarantine a11y.app` once.

### Linux

No warning. Unpack, make sure the system libraries are installed (most desktops
have them already), and run it:

```sh
tar -xzf a11y-<version>-x86_64-unknown-linux-gnu.tar.gz
cd a11y-<version>-x86_64-unknown-linux-gnu
./a11y
```

To add a11y to your application menu with its icon, run the bundled installer. It needs no
sudo and installs under `~/.local` (binary, `.desktop` entry, icons and file types):

```sh
./install.sh
```

On a minimal system install the libraries the editor links against, for
example on Debian or Ubuntu:

```sh
sudo apt install libxkbcommon0 libwayland-client0 libx11-6 libxcursor1 libxrandr2 libxi6 libgl1 libfontconfig1
```

## GPU rendering

The canvas draws with the GPU (Vulkan, Metal, DirectX 12 or OpenGL) and falls
back to the CPU renderer when no suitable adapter is found. If you see drawing
problems, turn off **Preferences → Use GPU rendering**.

## Updates

A few seconds after launch, and then once a day while it runs, a11y asks GitHub's
release list (one request to `api.github.com`, with the user agent `a11y/<version>`)
whether a newer version is out. If there is one, a banner offers the release notes and
**Update now**. Nothing is downloaded before you press it. **Skip this version** stops the
reminders until the next release.

**Update now** downloads the file for your install from the release, with a progress bar and
a Cancel button, and checks its SHA-256 against the release's `SHA256SUMS` (a mismatch or a
missing entry stops the update and deletes the download). Then the dialog offers **Restart to
update**. If you are sharing or in a live session, a11y asks "Stop sharing?" / "Leave the
session?" first, and if the document has unsaved changes it asks Save / Discard / Cancel. After
that a11y closes, installs the new version and starts it again. Downloads only come from
this repository's release files on `github.com` (and GitHub's release file hosts it redirects
to); nothing else is fetched.

What happens per install:

| Install | What the update does |
|---|---|
| Windows installer | runs the new `setup.exe` silently (`/SILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /RELAUNCH=1`) in the same per-user or all-users mode; the installer starts a11y again |
| Windows portable zip | unpacks the new `a11y.exe` (and `a11y-diff.exe` if it sits beside it) next to the old one, renames the running exe to `a11y.old.exe`, moves the new one in and starts it; the old file is deleted at the next start |
| macOS | unpacks the new `a11y.app` from `a11y-<version>-macos-universal.tar.gz` beside the old one; a small shell script waits for a11y to quit, swaps the bundles, clears the quarantine flag and opens the app |
| Linux tarball | renames the new `a11y` (and `a11y-diff`) over the old files, which is safe while the old one runs; if you installed with `install.sh`, the desktop entry, icons and file types are refreshed too |

When a11y cannot update itself it says why and keeps **Open release page** as a link, so you
can install the new version by hand as before. That is the case when the folder is not writable
by you (for example `/Applications` without administrator rights, or a copy that macOS runs
from a read-only location: move it to Applications first), for Linux packages that your
package manager owns (`/usr/bin`, `/opt`, snaps, Flatpak) and AppImages, and for builds from
source.

Turn the automatic check off with **Preferences → Check for updates automatically**, or set
`A11Y_NO_UPDATE_CHECK=1`. **Help → Check for updates…** still works, and Help also shows the
version you are running. Builds from source never check and never update themselves.

## Building from source

```sh
git clone https://github.com/debajay3/A11y
cd a11y
cargo run --release -p a11y-app
```

The pinned Rust toolchain installs automatically through `rust-toolchain.toml`.
See [CONTRIBUTING.md](../CONTRIBUTING.md) for the Linux build packages and the
test workflow.

## Signed builds

Signing is planned: Windows binaries through a code-signing certificate, the macOS
app signed with a Developer ID and notarized. Once that lands, these warnings go away and
this page will say so.
