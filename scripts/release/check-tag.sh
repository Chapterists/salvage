#!/usr/bin/env bash
# Checks a release tag against the version being built, for any tool.
#
#   scripts/release/check-tag.sh <tag-prefix> <version>
#
# Prints <version>. When run for a tag (GITHUB_REF_TYPE=tag), fails unless the
# tag is exactly <tag-prefix><version>, e.g. cli-v0.1.0, so a release can never
# ship something that reports another version.
set -euo pipefail
prefix="${1:?usage: $0 <tag-prefix> <version>}"
version="${2:?usage: $0 <tag-prefix> <version>}"

if [ "${GITHUB_REF_TYPE:-}" = "tag" ] && [ "${GITHUB_REF_NAME:-}" != "$prefix$version" ]; then
  echo "check-tag: tag ${GITHUB_REF_NAME:-} doesn't match version $version (expected $prefix$version)" >&2
  exit 1
fi
echo "$version"
