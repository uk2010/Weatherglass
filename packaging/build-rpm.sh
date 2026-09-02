#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
case "$(uname -m)" in
  x86_64) rpm_arch="x86_64" ;;
  aarch64|arm64) rpm_arch="aarch64" ;;
  *) echo "Unsupported package architecture: $(uname -m)" >&2; exit 1 ;;
esac
if ! cargo generate-rpm --version >/dev/null 2>&1; then
  echo "cargo-generate-rpm is required: cargo install cargo-generate-rpm --locked" >&2
  exit 1
fi
cargo build --release --locked
cargo generate-rpm
mkdir -p dist
rpm_file="$(find target/generate-rpm -maxdepth 1 -type f -name '*.rpm' | head -n 1)"
test -n "$rpm_file"
cp "$rpm_file" "dist/weatherglass-0.0.1-1.${rpm_arch}.rpm"
echo "Created $project_dir/dist/weatherglass-0.0.1-1.${rpm_arch}.rpm"
