#!/usr/bin/env bash
# Package an already built upstream rootfs archive; never rewrite source.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"
tag=${1:?Supply the Thor release tag}
version=$(awk -F '"' '/^version = / {print $2; exit}' Cargo.toml)
prefix="v${version}-thor."
[[ "$tag" == "$prefix"* && "${tag#"$prefix"}" =~ ^[1-9][0-9]*$ ]] || exit 1
[[ -z $(git status --porcelain --untracked-files=no) ]] || {
  echo 'Release packaging requires clean committed source' >&2; exit 1;
}
pkgver=${tag#v}
pkgver=${pkgver//-/.}
archive=dist/inputplumber-aarch64.tar.gz
[[ -s "$archive" ]]
stage=build/thor-package
[[ ! -e "$stage" ]] || { echo "Existing staging directory: $stage" >&2; exit 1; }
mkdir -p "$stage"
tar -xzf "$archive" -C "$stage" --strip-components=1
[[ -x "$stage/usr/bin/inputplumber" ]]
[[ $(readelf -h "$stage/usr/bin/inputplumber" | awk '/Machine:/ {print $2}') == AArch64 ]]
docs="$stage/usr/share/doc/thorch-inputplumber"
mkdir -p "$docs"
cp pkg/archlinux/THOR-RELEASE.md "$docs/README.md"
licenses="$stage/usr/share/licenses/thorch-inputplumber"
mkdir -p "$licenses"
cp LICENSE "$licenses/LICENSE"
date=$(git show -s --format=%ct HEAD)
size=$(du -sb "$stage" | awk '{print $1}')
cat > "$stage/.PKGINFO" <<EOF
pkgname = thorch-inputplumber
pkgbase = thorch-inputplumber
pkgver = $pkgver-1
pkgdesc = InputPlumber with optional Thor SSC motion and rumble gain
url = $(git remote get-url origin)
builddate = $date
packager = Thor release workflow
size = $size
arch = aarch64
license = GPL-3.0-or-later
provides = inputplumber=$version
conflict = inputplumber
conflict = inputplumber-bin
conflict = inputplumber-git
depend = dbus
depend = glibc
depend = libevdev
depend = libgcc
depend = libiio
depend = polkit
depend = systemd-libs
optdepend = thorch-motion: optional Qualcomm SSC sensor runtime
EOF
package="dist/thorch-inputplumber-$pkgver-1-aarch64.pkg.tar.zst"
bsdtar --zstd -cf "$package" -C "$stage" .PKGINFO usr
bsdtar -xOf "$package" .PKGINFO
{
  printf 'SOURCE_REPOSITORY=%s\n' "$(git remote get-url origin)"
  printf 'SOURCE_REVISION=%s\n' "$(git rev-parse HEAD)"
  printf 'RELEASE_TAG=%s\n' "$tag"
  rustc --version
  sha256sum Cargo.lock
} > dist/PROVENANCE
cp pkg/archlinux/THOR-RELEASE.md dist/INSTALL.md
(cd dist && sha256sum inputplumber-*.tar.gz thorch-inputplumber-*.pkg.tar.zst PROVENANCE INSTALL.md > SHA256SUMS)
