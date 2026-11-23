# Releasing

Releases are tag-driven. Pushing a tag `vX.Y.Z` runs `.github/workflows/release.yml`.

## Checklist

1. Make sure `main` is green (tests on Linux, Windows and macOS, Clippy).
2. Optionally bump `version` under `[workspace.package]` in the root `Cargo.toml`, run `cargo build --workspace` so `Cargo.lock` updates, and commit both (`release: 0.2.0`). The app's own version comes from the tag either way: the workflow compiles it in as `A11Y_VERSION` (About, `a11y --version`, update checks) and fails if `--version` disagrees.
3. Tag and push:

   ```sh
   git tag -a v0.2.0 -m "a11y 0.2.0"
   git push origin v0.2.0
   ```

   A tag containing a hyphen (`v0.2.0-rc.1`) is published as a pre-release.
4. Watch the Release workflow. When all builds pass, a GitHub Release is created with auto-generated notes. Edit the notes if needed.

## What gets built

| Job | Runner | Output |
|---|---|---|
| `build` `x86_64-pc-windows-msvc` | `windows-latest` | `a11y-<version>-<target>.zip` (the exe has the icon and version info embedded by `crates/a11y-app/build.rs`) |
| `build` `x86_64-unknown-linux-gnu` | `ubuntu-latest` | `a11y-<version>-<target>.tar.gz` (adds `a11y.desktop`, `icons/`, `install.sh`) |
| `macos` | `macos-14` (builds `aarch64-apple-darwin` and cross-compiles `x86_64-apple-darwin`; `macos-13` is retired) | `a11y-<version>-macos.dmg` and `a11y-<version>-macos-universal.tar.gz` |

The Windows and Linux archives hold `a11y`, `a11y-diff`, `LICENSE-MIT`, `LICENSE-APACHE` and `README.md`. They are packaged by `scripts/package.sh <target> <version>`, which you can run locally after a release build.

The macOS job runs `scripts/package-macos.sh <version>`. It `lipo`s a universal `a11y` and `a11y-diff`, assembles `a11y.app` (bundle id `com.debajay3.a11y`, `Info.plist` from `packaging/macos/Info.plist.in`, `a11y.icns` built with `iconutil` from `assets/icon/a11y.iconset`), ad-hoc signs it, and writes the `.dmg` (app, `Applications` link, `Read me first.txt`) and the universal `.tar.gz` (app, licenses, README). It needs a Mac, so test changes to it with a dry run (below).

The release carries a combined `SHA256SUMS` covering every file (verify with `sha256sum -c SHA256SUMS`).

## App icons

The masters are `assets/icon/a11y-icon.svg` (full bleed, Windows and Linux) and `assets/icon/a11y-icon-macos.svg` (824 px tile on a 1024 canvas). All generated files are committed; regenerate them after editing a master with:

```sh
cargo run -p a11y-app --example gen_icons
```

This rewrites `assets/icon/png/`, `a11y.ico`, `a11y.iconset/` and `a11y.icns`. The window icon is the embedded 256 px PNG.

## Dry run

Run the workflow from the Actions tab (Release, Run workflow) on any branch. It builds and packages everything and uploads the archives as workflow artifacts, but creates no release. Run it after any change to `scripts/package-macos.sh` or the macOS plist, then open the downloaded `.dmg` on a Mac to check the icon, the drag-to-Applications layout and the Open Anyway flow. The version is stamped `0.0.0-dev.<sha>`.

## Update notifications

Release builds compare their stamped version with GitHub's release list (`/repos/debajay3/A11y/releases`; pre-releases are offered only to pre-release builds) and show a banner when a newer one is out. To preview the banner and dialog offline, stamp a build and point `A11Y_UPDATE_URL` at a file (or an `http://127.0.0.1` URL) in GitHub's release-list shape with a newer tag:

```sh
A11Y_VERSION=0.1.0-alpha.7 cargo build -p a11y-app
A11Y_UPDATE_URL=releases.json target/debug/a11y --fixture showcase --panel updater --screenshot update.png
```

`--panel updater-banner` shows the banner instead of the dialog.

## Not covered yet

- macOS: the app is ad-hoc signed but not signed with a Developer ID or notarized, so Gatekeeper asks for a one-time Open Anyway. Double-clicking a file in Finder does not pass it to a11y (winit 0.30 does not expose `application:openFiles:`); "Open With" lists a11y through `CFBundleDocumentTypes`.
- Windows binaries are unsigned (SmartScreen warning). There is no installer; the zip is the download.
- Linux binaries link the build runner's glibc; they run on that distribution or newer.
