#!/usr/bin/env bash
# Writes release notes for one tool to stdout: its intro (with {{version}}
# filled in), then the commits since that tool's previous release that touch
# its files.
#
#   scripts/release/notes.sh <intro.md> <tag-prefix> <version> <ref> <path>...
#
# <ref> is the tag being released, or HEAD for a dry run. The previous release
# is the latest <tag-prefix>* tag before <ref>; with none, all history counts.
set -euo pipefail
usage="usage: $0 <intro.md> <tag-prefix> <version> <ref> <path>..."
intro="${1:?$usage}"
prefix="${2:?$usage}"
version="${3:?$usage}"
ref="${4:?$usage}"
shift 4
[ "$#" -ge 1 ] || { echo "notes: give the paths that belong to this tool" >&2; exit 2; }

if previous="$(git describe --tags --abbrev=0 --match "$prefix*" "$ref^" 2>/dev/null)"; then
  range="$previous..$ref"
  heading="Changes since $previous"
else
  range="$ref"
  heading="Changes"
fi

sed "s/{{version}}/$version/g" "$intro"
echo
echo "### $heading"
echo
changes="$(git log --no-merges --reverse --format='- %s' "$range" -- "$@")"
if [ -n "$changes" ]; then
  echo "$changes"
else
  echo "- No changes to this tool's files."
fi
