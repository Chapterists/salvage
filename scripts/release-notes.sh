#!/usr/bin/env bash
# Writes release notes in Markdown to stdout: install steps, the macOS
# signing caveat, and the commits since the previous v* tag.
#
#   scripts/release-notes.sh <version> [<ref>]
#
# <ref> defaults to HEAD. For a tag, the previous tag is looked up from its parent.
set -euo pipefail
version="${1:?usage: $0 <version> [<ref>]}"
ref="${2:-HEAD}"

if previous="$(git describe --tags --abbrev=0 --match 'v*' "$ref^" 2>/dev/null)"; then
  range="$previous..$ref"
  since="Changes since $previous"
else
  range="$ref"
  since="Changes"
fi

cat <<NOTES
Download the archive for your machine, check it against \`SHA256SUMS\`, extract it, and put \`salvage\` somewhere on your \`PATH\`.

| Platform | Archive |
| --- | --- |
| macOS (Apple silicon and Intel, one universal binary) | \`salvage-$version-macos-universal.tar.gz\` |
| Windows x86_64 | \`salvage-$version-windows-x86_64.zip\` |
| Linux x86_64 (static, no dependencies) | \`salvage-$version-linux-x86_64.tar.gz\` |

\`\`\`sh
shasum -a 256 -c SHA256SUMS --ignore-missing   # sha256sum on Linux
salvage --version
\`\`\`

### macOS: not signed yet

The macOS binary isn't signed or notarized yet, so macOS blocks it the first time you run a copy downloaded through a browser. Clear the quarantine flag once, after extracting:

\`\`\`sh
xattr -d com.apple.quarantine ./salvage
\`\`\`

Downloads made with \`curl\` don't get the flag. Signing and notarization will come in a later release.

### $since

NOTES
git log --no-merges --reverse --format='- %s' "$range"
