#!/usr/bin/env bash
# Prints the version being released: the workspace version from Cargo.toml.
# When run for a tag (GITHUB_REF_TYPE=tag), fails unless the tag is exactly
# "v<version>", so a release can never ship a binary that reports another version.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"

# tr: Windows checkouts may have CRLF line endings.
version="$(tr -d '\r' < "$root/Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n 1)"
if [ -z "$version" ]; then
  echo "release-version: no version in Cargo.toml" >&2
  exit 1
fi

if [ "${GITHUB_REF_TYPE:-}" = "tag" ] && [ "${GITHUB_REF_NAME:-}" != "v$version" ]; then
  echo "release-version: tag ${GITHUB_REF_NAME:-} doesn't match Cargo.toml version $version (expected v$version)" >&2
  exit 1
fi
echo "$version"
