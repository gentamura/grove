#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
bundle_dir="$project_dir/target/release/bundle/macos/Grove.app"
contents_dir="$bundle_dir/Contents"

cd "$project_dir"
cargo build --release

mkdir -p "$contents_dir/MacOS" "$contents_dir/Resources"
install -m 755 "$project_dir/target/release/grove" "$contents_dir/MacOS/grove"
install -m 644 "$project_dir/packaging/macos/Info.plist" "$contents_dir/Info.plist"
install -m 644 "$project_dir/assets/icon.icns" "$contents_dir/Resources/icon.icns"

/usr/bin/codesign --force --deep --sign - "$bundle_dir"
/usr/bin/codesign --verify --deep --strict "$bundle_dir"

printf '%s\n' "$bundle_dir"
