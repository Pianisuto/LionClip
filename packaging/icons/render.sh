#!/usr/bin/env bash
# Renders the LionClip icon into a hicolor icon theme directory.
#
# Usage: render.sh <source.svg> <icon-theme-dir> <icon-name>
#
# The SVG is the source of truth and is installed as the scalable icon; the
# PNGs exist so the small sizes stay crisp in the dock, the app grid and
# Alt+Tab, and so the icon still shows up when the SVG pixbuf loader is not
# installed.
set -euo pipefail

source_svg=${1:?missing source SVG}
theme_dir=${2:?missing icon theme directory}
icon_name=${3:?missing icon name}

sizes=(16 24 32 48 64 128 256)

render() {
    local size=$1 output=$2

    if command -v rsvg-convert >/dev/null 2>&1; then
        rsvg-convert --width "$size" --height "$size" --output "$output" "$source_svg"
    elif command -v gdk-pixbuf-thumbnailer >/dev/null 2>&1; then
        gdk-pixbuf-thumbnailer -s "$size" "$source_svg" "$output"
    else
        echo "render.sh: need rsvg-convert (librsvg2-bin) or gdk-pixbuf-thumbnailer" >&2
        exit 1
    fi
}

install -D -m 644 "$source_svg" "$theme_dir/scalable/apps/$icon_name.svg"

for size in "${sizes[@]}"; do
    output=$theme_dir/${size}x${size}/apps/$icon_name.png
    install -d -m 755 "$(dirname "$output")"
    render "$size" "$output"
    chmod 644 "$output"
done

echo "render.sh: installed $icon_name at scalable and ${sizes[*]} px into $theme_dir"
