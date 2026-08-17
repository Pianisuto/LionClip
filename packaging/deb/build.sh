#!/usr/bin/env bash
# Builds the LionClip .deb for the architecture of the machine running it.
#
#   packaging/deb/build.sh [output-directory]
#
# Everything the package contains comes from the repository or from
# `cargo build --release`; the runtime dependencies come from dpkg-shlibdeps
# reading the built binary, so they cannot drift from what LionClip links
# against. Set SKIP_BUILD=1 to package a release binary that is already built.
set -euo pipefail

readonly APPLICATION_ID=io.github.Pianisuto.LionClip
readonly PACKAGE=lionclip
readonly MAINTAINER='Leonardo Vulczak <leonardovulczak@gmail.com>'

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
packaging=$repo_root/packaging
output_dir=${1:-$repo_root/target/deb}

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)
arch=$(dpkg --print-architecture)
staging=$repo_root/target/deb-staging/${PACKAGE}_${version}_${arch}

if [ -z "$version" ]; then
    echo "build.sh: could not read the version from Cargo.toml" >&2
    exit 1
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
    (cd "$repo_root" && cargo build --release --locked)
fi

binary=$repo_root/target/release/$PACKAGE
if [ ! -x "$binary" ]; then
    echo "build.sh: $binary is missing; run without SKIP_BUILD=1" >&2
    exit 1
fi

rm -rf "$staging"
install -D -m 755 "$binary" "$staging/usr/bin/$PACKAGE"
install -D -m 755 "$packaging/scripts/lionclip-shortcut" "$staging/usr/bin/lionclip-shortcut"
install -D -m 644 "$packaging/desktop/$APPLICATION_ID.desktop" \
    "$staging/usr/share/applications/$APPLICATION_ID.desktop"
install -D -m 644 "$packaging/autostart/$APPLICATION_ID.desktop" \
    "$staging/etc/xdg/autostart/$APPLICATION_ID.desktop"
install -D -m 644 "$packaging/metainfo/$APPLICATION_ID.metainfo.xml" \
    "$staging/usr/share/metainfo/$APPLICATION_ID.metainfo.xml"
install -D -m 644 "$packaging/deb/copyright" "$staging/usr/share/doc/$PACKAGE/copyright"
install -D -m 644 "$packaging/deb/README.Debian" "$staging/usr/share/doc/$PACKAGE/README.Debian"
gzip -9nc "$packaging/deb/changelog" >"$staging/usr/share/doc/$PACKAGE/changelog.Debian.gz"
chmod 644 "$staging/usr/share/doc/$PACKAGE/changelog.Debian.gz"

"$packaging/icons/render.sh" \
    "$packaging/icons/$APPLICATION_ID.svg" \
    "$staging/usr/share/icons/hicolor" \
    "$APPLICATION_ID"

# dpkg-shlibdeps resolves the shared libraries the binary actually needs into
# versioned package dependencies. It insists on a debian/control next to the
# working directory, so it gets a throwaway one.
shlibdeps_work=$repo_root/target/deb-staging/shlibdeps
rm -rf "$shlibdeps_work"
mkdir -p "$shlibdeps_work/debian"
cat >"$shlibdeps_work/debian/control" <<EOF
Source: $PACKAGE

Package: $PACKAGE
Architecture: $arch
EOF
depends=$(
    cd "$shlibdeps_work" &&
        dpkg-shlibdeps -O --warnings=0 "$staging/usr/bin/$PACKAGE" |
        sed 's/^shlibs:Depends=//'
)
if [ -z "$depends" ]; then
    echo "build.sh: dpkg-shlibdeps produced no dependencies" >&2
    exit 1
fi

installed_size=$(du -ks "$staging" | cut -f1)

install -d -m 755 "$staging/DEBIAN"
cat >"$staging/DEBIAN/control" <<EOF
Package: $PACKAGE
Version: $version
Architecture: $arch
Maintainer: $MAINTAINER
Installed-Size: $installed_size
Depends: $depends, hicolor-icon-theme
Section: utils
Priority: optional
Homepage: https://github.com/Pianisuto/LionClip
Description: clipboard history for GNOME and Zorin OS
 LionClip records the text and images you copy and puts them back on the
 clipboard from a small popup that opens near the pointer. Search as you type,
 pick with the keyboard or the mouse, pin what you want to keep.
 .
 The history is stored locally under the user's XDG data directory. LionClip
 has no accounts, no sync and no network access.
EOF

# The autostart entry is deliberately not a conffile. dpkg keeps conffiles on
# remove and only deletes them on purge, which would leave an autostart entry
# asking the session to run a /usr/bin/lionclip that remove had just deleted.
# It holds no user configuration to preserve either: switching autostart off is
# a per-user copy in ~/.config/autostart, which is where GNOME's own tools write
# it and which no package operation touches.
install -m 755 "$packaging/deb/postinst" "$staging/DEBIAN/postinst"
install -m 755 "$packaging/deb/postrm" "$staging/DEBIAN/postrm"

mkdir -p "$output_dir"
package_path=$output_dir/${PACKAGE}_${version}_${arch}.deb
dpkg-deb --root-owner-group --build "$staging" "$package_path" >/dev/null

echo "built $package_path"
