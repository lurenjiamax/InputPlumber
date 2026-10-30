#!/usr/bin/env bash
# Build a self-contained artifact from this checkout; no Thorch source needed.
set -euo pipefail
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
output=$(realpath -m "${1:?Supply output directory outside the source checkout}")
case "$output/" in "$root/"*) echo 'Output must be outside source' >&2; exit 1;; esac
target=${TARGET_ARCH:-aarch64-unknown-linux-gnu}
[[ "$target" == aarch64-unknown-linux-gnu ]] || exit 1
cargo build --manifest-path "$root/Cargo.toml" --locked --release --target "$target" --bin inputplumber
binary="${CARGO_TARGET_DIR:-$root/target}/$target/release/inputplumber"
readelf -h "$binary" | grep -q AArch64
stage="$output/package-root"
[[ ! -e "$stage" ]] || { echo "Existing stage: $stage" >&2; exit 1; }
mkdir -p "$stage"
cp -a "$root/rootfs/." "$stage/"
cp -a "$root/pkg/archlinux/thorch/payload/." "$stage/"
install -Dm755 "$binary" "$stage/usr/bin/inputplumber"
install -Dm644 "$root/pkg/archlinux/thorch/inputplumber-restart.conf" \
  "$stage/usr/lib/systemd/system/inputplumber.service.d/90-restart.conf"
record="$stage/usr/share/licenses/thorch-inputplumber"
mkdir -p "$record"
install -m644 "$root/LICENSE" "$record/LICENSE"
git -C "$root" rev-parse HEAD > "$record/SOURCE_COMMIT"
git -C "$root" diff HEAD --binary > "$record/SOURCE_CHANGES.patch"
version=$(awk -F '"' '/^version = / {print $2; exit}' "$root/Cargo.toml")
archive="inputplumber-thorch-$version.tar.gz"
tar -czf "$output/$archive" -C "$output" package-root
(cd "$output"; sha256sum "$archive" > SHA256SUMS)
