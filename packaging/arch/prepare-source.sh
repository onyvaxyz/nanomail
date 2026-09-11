#!/usr/bin/env bash
# Archiviert den aktuellen Arbeitsstand, nicht nur veröffentlichte Commits.
set -euo pipefail
repo=$(realpath "$(dirname "$0")/../..")
output=$(realpath -m "${1:-$repo/.amp/arch-build}")
version=$(sed -n 's/^pkgver=//p' "$repo/packaging/arch/PKGBUILD")
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || exit 1
mkdir -p "$output"
archive="$output/nanomail-$version.tar.gz"
# Nur explizite Build-Eingaben: keine Zugangsdaten, Git-Metadaten,
# node_modules, generierten Schemas oder bereits gebauten Binärdateien.
tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner \
  --transform="s,^,nanomail-$version/," -C "$repo" -cf - \
  README.md ui src-tauri/Cargo.toml src-tauri/Cargo.lock \
  src-tauri/build.rs src-tauri/tauri.conf.json src-tauri/src \
  src-tauri/capabilities src-tauri/icons packaging/arch | gzip -n > "$archive"
checksum=$(sha256sum "$archive" | cut -d ' ' -f1)
sed "s/@SOURCE_SHA256@/$checksum/" "$repo/packaging/arch/PKGBUILD" > "$output/PKGBUILD"
printf 'Quellen und PKGBUILD: %s\nSHA-256: %s\n' "$output" "$checksum"
printf 'Auf Arch Linux: cd %q && makepkg -si\n' "$output"
