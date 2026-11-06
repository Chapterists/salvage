#!/usr/bin/env bash
# Packages built binaries into a release archive under dist/.
#
#   scripts/package-release.sh <name> <version> <target>...
#
# macos-*   lipo the targets into one universal binary, archive as .tar.gz
# windows-* the single target's salvage.exe, archive as .zip
# other     the single target's salvage, archive as .tar.gz
#
# Each archive holds salvage, LICENSE and README.md in a folder named after it.
set -euo pipefail
name="${1:?usage: $0 <name> <version> <target>...}"
version="${2:?usage: $0 <name> <version> <target>...}"
shift 2
[ "$#" -ge 1 ] || { echo "package-release: no targets given" >&2; exit 2; }

root="$(cd "$(dirname "$0")/.." && pwd)"
base="salvage-$version-$name"
stage="$root/dist/$base"
rm -rf "$stage"
mkdir -p "$stage"

case "$name" in
  macos-*)
    bins=()
    for target in "$@"; do bins+=("$root/target/$target/release/salvage"); done
    lipo -create -output "$stage/salvage" "${bins[@]}"
    lipo -info "$stage/salvage"
    ;;
  windows-*)
    cp "$root/target/$1/release/salvage.exe" "$stage/"
    ;;
  *)
    cp "$root/target/$1/release/salvage" "$stage/"
    ;;
esac
cp "$root/LICENSE" "$root/README.md" "$stage/"

cd "$root/dist"
case "$name" in
  windows-*)
    archive="$base.zip"
    7z a -tzip "$archive" "$base" > /dev/null
    ;;
  *)
    archive="$base.tar.gz"
    # Keep macOS extended attributes (._ files) out of the archive.
    COPYFILE_DISABLE=1 tar -czf "$archive" "$base"
    ;;
esac
rm -rf "$stage"
echo "dist/$archive"
