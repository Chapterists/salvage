#!/usr/bin/env bash
# Guards this public repository against leaking internal files, internal links,
# credentials, AI credits, or Unreal Engine source (which is under Epic's EULA
# and must never be redistributed here; engine fixes go upstream to Epic).
#
#   scripts/check-public-safety.sh --staged   # files staged for commit (pre-commit hook)
#   scripts/check-public-safety.sh --all      # every tracked file (CI)
#   scripts/check-public-safety.sh --message <file>   # one commit message (commit-msg hook)
#   scripts/check-public-safety.sh --history  # every commit message in history (CI)
#
# Commit messages must be plain human-readable descriptions: no internal tracker
# references (issue IDs like SAL-12, Linear links), no tokens, and no AI credits.
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
  # A vendored or copied Unreal Engine tree (source, shaders, config, content...).
  # Salvage's own Unreal plugins live under unreal/Plugins/<Name>/, never under Engine/.
  '(^|/)Engine/(Source|Shaders|Config|Content|Plugins|Build|Binaries|Extras|Programs|Platforms|Restricted)/'
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
  # Epic's copyright header, carried by every engine source, shader, script and
  # build file, in current and older forms (optional "(c)" and year or range).
  # Epic's EULA attribution notice ("Copyright 1998 – <year>, ...") is allowed.
  'Copyright( \([cC]\)| ©)?( [0-9]{4}(-[0-9]{4})?)? Epic Games'
)

# Content that must never appear in commit messages or PR text (extended regex,
# case-insensitive). Issue IDs use the keys of every team in the internal Linear
# workspace.
blocked_message=(
  '(^|[^[:alnum:]_-])(SAL|CHA|OLV|BRK|LUN|LUP|AMZ)-[0-9]+'  # internal issue IDs
  'linear\.app'
  'lin_(api|oauth)_[A-Za-z0-9]{20,}'
)

# AI credits: Salvage commits and PRs carry no AI attribution. This catches
# co-author trailers naming an AI or its vendor, the vendor's noreply address,
# and "Generated with/by <tool>" footers. Human co-authors stay allowed.
ai_credit_message=(
  '^[[:space:]]*co-authored-by:.*(claude|anthropic|openai|chatgpt|gpt-?[0-9]|copilot|gemini|codex|cursor)'
  'noreply@anthropic\.com'
  'generated (with|by|using) \[?(claude|chatgpt|openai|copilot|gemini|codex|cursor)'
  'claude\.(com|ai)/(claude-)?code'
)

# Commits that predate the AI-credit rule and are kept as-is in history, by
# full SHA. Only the AI-credit rule is waived for them; every other rule still
# applies. Do not add to this list: rewrite unmerged commits instead.
#   8487fdf  "Add safeguards against leaking secrets and internal material"
#            (squash-merged before the rule existed; carries a co-author trailer)
ai_credit_exempt_commits=(
  8487fdfbcc863f660b2656c208674af090785a57
)

check_message() { # $1 = label, $2 = "exempt-ai" to waive the AI-credit rule; message on stdin
  local msg re bad=0
  local -a patterns=("${blocked_message[@]}")
  [ "${2:-}" = "exempt-ai" ] || patterns+=("${ai_credit_message[@]}")
  msg="$(grep -v '^#' || true)"  # ignore git's comment lines
  for re in "${patterns[@]}"; do
    # Here-string, not a pipe: see the SIGPIPE note on content_file below.
    if LC_ALL=C grep -Eiq -e "$re" <<< "$msg"; then
      echo "  ✗ $1 — matches /$re/" >&2; bad=1
    fi
  done
  return "$bad"
}

message_failed() {
  echo "" >&2
  echo "Commit messages and PR text are public: write a plain human-readable description," >&2
  echo "with no issue IDs (e.g. SAL-12), Linear links, tokens, or AI credits (no" >&2
  echo "Co-Authored-By trailers naming an AI, no \"Generated with ...\" footers)." >&2
  exit 1
}

is_ai_credit_exempt() { # $1 = full commit SHA
  local exempt
  for exempt in "${ai_credit_exempt_commits[@]}"; do
    [ "$1" = "$exempt" ] && return 0
  done
  return 1
}

case "$mode" in
  --message)
    check_message "commit message" < "${2:?usage: $0 --message <file>}" || message_failed
    exit 0 ;;
  --history)
    failed=0
    for sha in $(git rev-list --all); do
      waiver=""
      if is_ai_credit_exempt "$sha"; then waiver="exempt-ai"; fi
      git log -1 --format=%B "$sha" | check_message "commit ${sha:0:12}" "$waiver" || failed=1
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

# Content is grepped from a real file, never through a pipe: with pipefail, a
# `producer | grep -q` that matches early in a large file kills the producer
# with SIGPIPE and the pipeline reports failure, silently hiding the match.
tmp_blob="$(mktemp)"
trap 'rm -f "$tmp_blob"' EXIT
content_file() { # prints the path to scan for $1, or fails if unreadable
  if [ "$mode" = "--staged" ]; then
    git show ":$1" > "$tmp_blob" 2>/dev/null && echo "$tmp_blob"
  else
    [ -f "$1" ] && echo "$1"
  fi
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

  file="$(content_file "$path")" || continue
  for re in "${blocked_content[@]}"; do
    if LC_ALL=C grep -Eq -e "$re" -- "$file"; then
      report "$path — contains blocked content matching /$re/"
    fi
  done
done < <("${list_cmd[@]}")

if [ "$problems" -gt 0 ]; then
  echo "" >&2
  echo "Public-safety check failed: $problems problem(s). This repository is public." >&2
  echo "Remove the file(s) or content above (git rm --cached <path>) before committing." >&2
  echo "Unreal Engine source, headers, config and content are under Epic's EULA and" >&2
  echo "must not be copied here; describe engine changes instead and send fixes upstream." >&2
  exit 1
fi
