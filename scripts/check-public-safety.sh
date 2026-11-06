#!/usr/bin/env bash
# Guards this public repository against leaking internal files, internal links,
# or credentials.
#
#   scripts/check-public-safety.sh --staged   # files staged for commit (pre-commit hook)
#   scripts/check-public-safety.sh --all      # every tracked file (CI)
#   scripts/check-public-safety.sh --message <file>   # one commit message (commit-msg hook)
#   scripts/check-public-safety.sh --history  # every commit message in history (CI)
#
# Commit messages must be plain human-readable descriptions: no internal tracker
# references (issue IDs like SAL-12, Linear links) and no tokens.
#
# Exits non-zero and lists every problem found. Secret detection itself is done
# by gitleaks (run separately by the hook and CI); this script covers paths and
# internal references that a generic secret scanner would not flag.
set -euo pipefail

mode="${1:---staged}"

# Paths that must never be committed (extended regex, matched against repo path).
blocked_paths=(
  '(^|/)\.env$'
  '(^|/)\.env\.[^/]+$'
  '\.(pem|key|p8|p12|pfx|jks|keystore|mobileprovision|provisionprofile)$'
  '(^|/)id_(rsa|ed25519|ecdsa|dsa)[^/]*$'
  '(^|/)\.netrc$'
  '(^|/)credentials\.json$'
  '(^|/)service-account[^/]*\.json$'
  '(^|/)secrets/'
  '(^|/)(internal|private|\.internal|\.private|\.linear|notes)/'
  '\.linear\.md$'
  '(^|/)CLAUDE\.local\.md$'
  '(^|/)\.claude/settings\.local\.json$'
  '(^|/)\.claude/memory/'
  '(^|/)\.orca/'
)
# Explicitly allowed despite matching the above.
allowed_paths=(
  '(^|/)\.env\.(example|sample)$'
)

# Content that must never appear in committed files (extended regex).
blocked_content=(
  'linear\.app/chapterists'          # links into the private Linear workspace
  'lin_(api|oauth)_[A-Za-z0-9]{20,}' # Linear API / OAuth tokens
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
)

# Content that must never appear in commit messages (extended regex, case-insensitive).
# Issue IDs use the keys of every team in the internal Linear workspace.
blocked_message=(
  '(^|[^[:alnum:]_-])(SAL|CHA|OLV|BRK|LUN|LUP|AMZ)-[0-9]+'  # internal issue IDs
  'linear\.app'
  'lin_(api|oauth)_[A-Za-z0-9]{20,}'
)

check_message() { # $1 = label, message on stdin
  local msg re bad=0
  msg="$(grep -v '^#' || true)"  # ignore git's comment lines
  for re in "${blocked_message[@]}"; do
    if printf '%s\n' "$msg" | LC_ALL=C grep -Eiq -e "$re"; then
      echo "  ✗ $1 — matches /$re/" >&2; bad=1
    fi
  done
  return "$bad"
}

message_failed() {
  echo "" >&2
  echo "Commit messages are public: write a plain human-readable description, with no" >&2
  echo "issue IDs (e.g. SAL-12), Linear links, or tokens." >&2
  exit 1
}

case "$mode" in
  --message)
    check_message "commit message" < "${2:?usage: $0 --message <file>}" || message_failed
    exit 0 ;;
  --history)
    failed=0
    for sha in $(git rev-list --all); do
      git log -1 --format=%B "$sha" | check_message "commit ${sha:0:12}" || failed=1
    done
    [ "$failed" -eq 0 ] || message_failed
    exit 0 ;;
  --text) # arbitrary public text on stdin, e.g. a PR title/body
    check_message "${2:-text}" || message_failed
    exit 0 ;;
  --staged) list_cmd=(git diff --cached --name-only --diff-filter=ACMR -z) ;;
  --all)    list_cmd=(git ls-files -z) ;;
  *) echo "usage: $0 [--staged|--all|--message <file>|--history|--text <label>]" >&2; exit 2 ;;
esac

read_file() {
  if [ "$mode" = "--staged" ]; then git show ":$1"; else cat -- "$1"; fi
}

problems=0
report() { echo "  ✗ $1" >&2; problems=$((problems + 1)); }

while IFS= read -r -d '' path; do
  allowed=0
  for re in "${allowed_paths[@]}"; do
    if [[ "$path" =~ $re ]]; then allowed=1; break; fi
  done
  if [ "$allowed" -eq 0 ]; then
    for re in "${blocked_paths[@]}"; do
      if [[ "$path" =~ $re ]]; then report "$path — internal or credential file; must not be committed"; break; fi
    done
  fi

  for re in "${blocked_content[@]}"; do
    if read_file "$path" 2>/dev/null | LC_ALL=C grep -Eq -e "$re"; then
      report "$path — contains blocked content matching /$re/"
    fi
  done
done < <("${list_cmd[@]}")

if [ "$problems" -gt 0 ]; then
  echo "" >&2
  echo "Public-safety check failed: $problems problem(s). This repository is public." >&2
  echo "Remove the file(s) or content above (git rm --cached <path>) before committing." >&2
  exit 1
fi
