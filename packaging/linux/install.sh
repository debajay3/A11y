#!/usr/bin/env sh
# Installs a11y for the current user. No sudo needed.
#
#   ./install.sh            install under ~/.local
#   PREFIX=/some/dir ./install.sh   install elsewhere (bin/ and share/ below it)
#
# To remove: delete ~/.local/bin/a11y, ~/.local/bin/a11y-diff, ~/.local/bin/a11y-mcp,
# ~/.local/share/applications/a11y.desktop, the a11y.png files under
# ~/.local/share/icons/hicolor/*/apps/ and ~/.local/share/mime/packages/a11y.xml.
#
# A11Y_RESOURCES_ONLY=1 skips the binaries: the in-app updater has already swapped them in
# (atomically, while a11y was running) and only needs the desktop entry, icons and file types refreshed.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}

install -d "$prefix/bin" "$prefix/share/applications" "$prefix/share/mime/packages"
if [ -z "${A11Y_RESOURCES_ONLY:-}" ]; then
  install -m 755 "$here/a11y" "$prefix/bin/a11y"
  [ -f "$here/a11y-diff" ] && install -m 755 "$here/a11y-diff" "$prefix/bin/a11y-diff"
  [ -f "$here/a11y-mcp" ] && install -m 755 "$here/a11y-mcp" "$prefix/bin/a11y-mcp"
fi
# Point Exec at the installed binary so launchers work even when ~/.local/bin is not on their PATH.
sed "s|^Exec=a11y|Exec=$prefix/bin/a11y|" "$here/a11y.desktop" > "$prefix/share/applications/a11y.desktop"
chmod 644 "$prefix/share/applications/a11y.desktop"
[ -f "$here/a11y-mime.xml" ] && install -m 644 "$here/a11y-mime.xml" "$prefix/share/mime/packages/a11y.xml"

for png in "$here"/icons/*.png; do
  size=$(basename "$png" .png)   # 16, 32, ... 512
  d="$prefix/share/icons/hicolor/${size}x${size}/apps"
  install -d "$d"
  install -m 644 "$png" "$d/a11y.png"
done

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/share/applications" 2>/dev/null || true
command -v update-mime-database >/dev/null 2>&1 && update-mime-database "$prefix/share/mime" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true

echo "a11y installed to $prefix"
case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) echo "note: $prefix/bin is not on your PATH; add it to run 'a11y' from a terminal." ;;
esac
