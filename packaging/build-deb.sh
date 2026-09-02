#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1788249600}"
cargo build --release --locked
case "$(uname -m)" in
  x86_64) deb_arch="amd64" ;;
  aarch64|arm64) deb_arch="arm64" ;;
  *) echo "Unsupported package architecture: $(uname -m)" >&2; exit 1 ;;
esac
stage_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir"' EXIT
package_root="$stage_dir/weatherglass_0.0.1_${deb_arch}"
install -Dm755 target/release/weatherglass "$package_root/usr/bin/weatherglass"
install -Dm644 data/io.github.weatherglass.Weatherglass.desktop "$package_root/usr/share/applications/io.github.weatherglass.Weatherglass.desktop"
install -Dm644 data/io.github.weatherglass.Weatherglass.metainfo.xml "$package_root/usr/share/metainfo/io.github.weatherglass.Weatherglass.metainfo.xml"
for size in 48 64 128 256 1024; do
  install -Dm644 "data/icons/hicolor/${size}x${size}/apps/io.github.weatherglass.Weatherglass.png" "$package_root/usr/share/icons/hicolor/${size}x${size}/apps/io.github.weatherglass.Weatherglass.png"
done
install -Dm644 README.md "$package_root/usr/share/doc/weatherglass/README.md"
install -Dm644 docs/SECURITY.md "$package_root/usr/share/doc/weatherglass/SECURITY.md"
install -Dm644 docs/THIRD_PARTY.md "$package_root/usr/share/doc/weatherglass/THIRD_PARTY.md"
install -Dm644 packaging/copyright "$package_root/usr/share/doc/weatherglass/copyright"
mkdir -p "$package_root/DEBIAN"
sed "s/ARCHITECTURE/$deb_arch/" packaging/control > "$package_root/DEBIAN/control"
find "$package_root" -print0 | xargs -0 touch --date="@$SOURCE_DATE_EPOCH"
mkdir -p dist
dpkg-deb --root-owner-group --build "$package_root" "dist/weatherglass_0.0.1_${deb_arch}.deb"
echo "Created $project_dir/dist/weatherglass_0.0.1_${deb_arch}.deb"
