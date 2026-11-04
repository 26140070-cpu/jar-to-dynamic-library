#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo "Usage: $0 /path/to/jar-to-dynamic-library-gui [icon.png]" >&2
    exit 2
fi

application="$(realpath "$1")"
if [[ ! -x "$application" ]]; then
    echo "Application is not executable: $application" >&2
    exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
icon="${2:-$script_dir/../assets/images/app-icon.png}"
if [[ ! -f "$icon" ]]; then
    echo "Icon file not found: $icon" >&2
    exit 1
fi
icon="$(realpath "$icon")"

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
applications_dir="$data_home/applications"
icons_dir="$data_home/icons/hicolor/512x512/apps"
mkdir -p "$applications_dir" "$icons_dir"
cp "$icon" "$icons_dir/jar-to-dynamic-library-gui.png"

escaped_application="${application//\\/\\\\}"
escaped_application="${escaped_application//\"/\\\"}"
cat > "$applications_dir/jar-to-dynamic-library-gui.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=JAR to Dynamic Library
Exec="$escaped_application"
TryExec="$escaped_application"
Icon=jar-to-dynamic-library-gui
StartupWMClass=jar-to-dynamic-library-gui
Categories=Utility;
Terminal=false
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications_dir"
fi

echo "Installed Wayland/X11 desktop entry and icon for $application"
