#!/usr/bin/env bash
# Self-test for scripts/check-public-safety.sh. Each case builds a throwaway git
# repo containing a copy of the guard plus one fixture (a file, commit message,
# or PR text), then checks that the guard blocks or allows it in every mode that
# sees that fixture.
#
#   scripts/test-public-safety.sh
#
# Fixtures that must be blocked are assembled at runtime, so this file never
# contains the blocked text itself and passes the guard it is testing.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
guard="$root/scripts/check-public-safety.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

epic_header="Copy""right Epic Games, Inc. All Rights Reserved."
private_key="-----BEGIN RSA PRIVATE"" KEY-----"

failures=0
cases=0

new_repo() { # creates an empty repo with the guard installed; prints its path
  local dir
  dir="$(mktemp -d "$work/case.XXXXXX")"
  git -C "$dir" init -q
  mkdir -p "$dir/scripts"
  cp "$guard" "$dir/scripts/check-public-safety.sh"
  git -C "$dir" add scripts/check-public-safety.sh
  echo "$dir"
}

# Bulk text so a match near the top is followed by well over a pipe buffer.
filler() { local i; for ((i = 0; i < 20000; i++)); do echo "int filler_$i = $i;"; done; }

# record <want> <got> <mode> <description>
record() {
  cases=$((cases + 1))
  if [ "$2" = "$1" ]; then
    echo "  ok    $1  $3  $4"
  else
    echo "  FAIL  expected $1, got $2  $3  $4"
    failures=$((failures + 1))
  fi
}

# run_guard <dir> <args...> ; prints "allowed" or "blocked"
run_guard() {
  local dir="$1"; shift
  if (cd "$dir" && scripts/check-public-safety.sh "$@" >/dev/null 2>&1); then echo allowed; else echo blocked; fi
}

# expect <blocked|allowed> <description> <path> ; fixture content on stdin
expect() {
  local want="$1" desc="$2" path="$3" dir m
  dir="$(new_repo)"
  mkdir -p "$dir/$(dirname "$path")"
  cat > "$dir/$path"
  git -C "$dir" add -- "$path"
  for m in --staged --all; do
    record "$want" "$(run_guard "$dir" "$m")" "$m" "$desc"
  done
}

# expect_text <blocked|allowed> <description> ; commit message / PR text on stdin
expect_text() {
  local got
  if "$guard" --text test >/dev/null 2>&1; then got=allowed; else got=blocked; fi
  record "$1" "$got" "--text" "$2"
}

# expect_commit <blocked|allowed> <description> ; commit message on stdin.
# Checks the message both as the commit-msg hook sees it and, once committed,
# in --history.
expect_commit() {
  local want="$1" desc="$2" dir
  dir="$(new_repo)"
  cat > "$dir/.msg"
  record "$want" "$(run_guard "$dir" --message .msg)" "--message" "$desc"
  git -C "$dir" -c core.hooksPath=/dev/null -c user.name=test -c user.email=test@example.com \
    commit -q -F .msg
  record "$want" "$(run_guard "$dir" --history)" "--history" "$desc"
}

echo "Unreal Engine source:"
expect blocked "C++ header with Epic copyright" Source/Foo/Public/Foo.h \
  < <(printf '// %s\n\n#pragma once\n' "$epic_header")
expect blocked "shell script with Epic copyright" tools/build.sh \
  < <(printf '#!/bin/sh\n# %s\necho hi\n' "$epic_header")
expect blocked "older header with year range" Source/Old.cpp \
  < <(printf '// Copy''right 1998-2019 Epic Games, Inc. All Rights Reserved.\n')
expect blocked "header with (c)" Shaders/Common.ush \
  < <(printf '// Copy''right (c) Epic Games, Inc.\n')
expect blocked "large file with header at top" Source/Big.cpp \
  < <({ printf '// %s\n' "$epic_header"; filler; })
expect blocked "large file with header at bottom" Source/BigTail.cpp \
  < <({ filler; printf '// %s\n' "$epic_header"; })
expect blocked "vendored engine source tree" Engine/Source/Runtime/Core/Public/CoreMinimal.h \
  < <(printf '#pragma once\n')
expect blocked "vendored engine config" Engine/Config/BaseEngine.ini \
  < <(printf '[Core.System]\n')
expect blocked "engine tree nested in a subdirectory" third_party/UE/Engine/Shaders/Private/Common.ush \
  < <(printf 'float4 x;\n')

echo "Allowed Salvage content:"
expect allowed "Salvage plugin Build.cs" Plugins/SalvageMem/Source/SalvageMem/SalvageMem.Build.cs \
  < <(printf '// Copyright Salvage contributors. MIT License.\nusing UnrealBuildTool;\n')
expect allowed "project-level config" Config/DefaultEngine.ini \
  < <(printf '[/Script/Engine.RendererSettings]\nr.Streaming.PoolSize=1000\n')
expect allowed "README mentioning Epic Games" README.md \
  < <(printf 'Unreal Engine is a trademark of Epic Games, Inc. Salvage is not affiliated with Epic Games.\n')
expect allowed "Epic EULA attribution notice" NOTICE \
  < <(printf 'Unreal® Engine, Copy''right 1998 – 2026, Epic Games, Inc. All rights reserved.\n')
expect allowed "path mentioning engine dirs in text" docs/engine.md \
  < <(printf 'Salvage engine notes: see Source/Runtime in the engine checkout.\n')
expect allowed "an Engine directory that is not an engine tree" docs/Engine/overview.md \
  < <(printf 'docs\n')

echo "Existing rules:"
expect blocked "large file with private key at top" keys/big.txt \
  < <({ printf '%s\n' "$private_key"; filler; })
expect blocked ".env file" .env \
  < <(printf 'TOKEN=x\n')
expect allowed ".env.example" .env.example \
  < <(printf 'TOKEN=placeholder\n')
expect_text allowed "plain commit message" < <(printf 'Add config loader\n')
expect_text blocked "message with an issue ID" < <(printf 'Fix loader (SAL''-12)\n')

echo "AI credits:"
claude_trailer="Co-Authored-By: Claude Opus 5.5 <noreply@anth""ropic.com>"
expect_commit blocked "AI co-author trailer" < <(printf 'Add config loader\n\n%s\n' "$claude_trailer")
expect_commit blocked "lowercase Copilot trailer" \
  < <(printf 'Add loader\n\nco-authored-by: Copilot <copilot@users.noreply.github.com>\n')
expect_commit blocked "OpenAI address in a trailer" \
  < <(printf 'Add loader\n\nCo-authored-by: Codex <codex@open''ai.com>\n')
expect_commit allowed "human co-author trailer" \
  < <(printf 'Add loader\n\nCo-authored-by: Jane Doe <jane@example.com>\n')
expect_commit allowed "message that names a Claude file" < <(printf 'Ignore CLAUDE.local.md files\n')
expect_text blocked "PR footer with tool link" \
  < <(printf 'Summary\n\n\xf0\x9f\xa4\x96 Generated with [Claude Code](https://claude.com/claude''-code)\n')
expect_text blocked "plain generated-with footer" < <(printf 'Summary\n\nGenerated by Chat''GPT\n')
expect_text blocked "vendor noreply address alone" < <(printf 'Thanks noreply@anth''ropic.com\n')
expect_text allowed "generated config wording" < <(printf 'Generated with the low-spec config tool\n')

echo "This repository:"
cases=$((cases + 1))
if (cd "$root" && "$guard" --history >/dev/null 2>&1); then
  echo "  ok    allowed  --history  every commit (one exempt pre-rule commit has an AI credit)"
else
  echo "  FAIL  the repository's own history is blocked by the guard"
  failures=$((failures + 1))
fi
cases=$((cases + 1))
if (cd "$root" && "$guard" --all >/dev/null 2>&1); then
  echo "  ok    allowed  --all  every tracked file, including the guard and this test"
else
  echo "  FAIL  the repository's own tracked files are blocked by the guard"
  failures=$((failures + 1))
fi

echo ""
if [ "$failures" -gt 0 ]; then
  echo "$failures of $cases checks failed." >&2
  exit 1
fi
echo "All $cases checks passed."
