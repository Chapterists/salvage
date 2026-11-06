#!/usr/bin/env bash
# Prints the salvage command's version: the workspace version in Cargo.toml.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
# tr: Windows checkouts may have CRLF line endings.
version="$(tr -d '\r' < "$root/Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n 1)"
if [ -z "$version" ]; then
  echo "cargo-version: no version in Cargo.toml" >&2
  exit 1
fi
echo "$version"
