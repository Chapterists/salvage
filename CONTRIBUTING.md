# Contributing to Salvage

Thanks for helping. Salvage is early: one tool is in the repository and there's no release yet, so the most useful contributions right now are measurements, bug reports from real machines, and small focused fixes.

Before anything else: **this repository is public.** Every file, commit message, branch name, pull request title and pull request description you push can be read by anyone, and it stays in the history even after you delete it. The rules below exist so nothing private ends up there by accident.

## Set up the checks

Clone the repository, then turn on its git hooks:

```sh
git config core.hooksPath .githooks
```

The hooks need [gitleaks](https://github.com/gitleaks/gitleaks), which scans for secrets:

```sh
brew install gitleaks        # macOS
```

On Linux or Windows, download a release from the [gitleaks releases page](https://github.com/gitleaks/gitleaks/releases) and put it on your `PATH`. Check it works with `gitleaks version`.

With the hooks on, every commit runs two checks:

- **pre-commit** runs `scripts/check-public-safety.sh --staged` on the files you're committing, then gitleaks on the same files. It refuses the commit if gitleaks isn't installed.
- **commit-msg** checks your commit message (see below).

You can run the same checks yourself at any time:

```sh
scripts/check-public-safety.sh --all       # every tracked file
scripts/check-public-safety.sh --history   # every commit message
scripts/test-public-safety.sh              # the guard's own tests
```

CI runs these checks plus a gitleaks scan of the full history on every push and pull request, and a pull request can't merge until they pass. So if a hook stops you, fix the content. Skipping the hooks with `--no-verify` only moves the failure to CI.

## What never goes in the repository

**Credentials.** No passwords, API keys, tokens, private keys, certificates, or `.env` files. If a tool needs configuration, commit a `.env.example` with placeholder values and keep the real `.env` local. `.gitignore` already covers the common names. If you pushed a secret by mistake, revoke or rotate it first, then report it privately as described in [SECURITY.md](SECURITY.md). Deleting the file doesn't remove it from history.

**Unreal Engine source or Epic content.** Unreal Engine is licensed under Epic's EULA, which doesn't allow redistributing its source here. That includes engine source files, headers, shaders, config files and content, copied whole or in part. The guard rejects files carrying Epic's copyright header and paths that look like a copied engine tree, such as `Engine/Source/`. Salvage's own Unreal plugins live under `unreal/Plugins/<PluginName>/`.

If a fix belongs in the engine itself, send it upstream to Epic as a pull request on their Unreal Engine repository. Here, describe the change in your own words and link to the upstream pull request.

**Internal material.** No private notes, plans, or exports from internal tools. Paths such as `internal/`, `private/` and `notes/` are blocked for that reason.

## Commit messages and pull requests

Write them for a stranger reading the history a year from now: a plain description of what changed and why.

- No issue IDs or links from private trackers. The guard rejects them.
- Co-author trailers must name real people. Credit lines for tools ("Generated with ...") are rejected.
- No tokens or secrets, obviously, and no pasted logs that might contain them.

The same rules apply to pull request titles and descriptions. CI checks them, and checks again whenever they're edited.

## Where things live

Salvage is a toolbox, so the repository holds many tools of different kinds. Each kind has its place:

| Path | What goes there |
| --- | --- |
| `crates/` | Rust: the `salvage` command (`crates/salvage`) and any shared Rust libraries, in one Cargo workspace. |
| `apps/<app>/` | Standalone apps with a user interface, such as a native Mac app. |
| `unreal/Plugins/<PluginName>/` | Unreal plugins, ready to drop into a project's `Plugins/` folder. Never engine code. |
| `addons/<host>/<addon>/` | Add-ons for other tools, such as `addons/blender/<addon>/`. |
| `tools/<tool>/` | Standalone scripts and small programs in any other language. |
| `docs/` | Guides that span more than one tool. |
| `site/` | The website. |
| `scripts/` | Maintaining the repository itself, never tools for users. |

Each tool owns its folder, and that folder has:

- **A README** that says what the tool is and how to install and uninstall it.
- **Its own CI**, filtered to its own paths, so a change to one tool doesn't run every other tool's checks.
- **Its own releases.** Each tool has a `release-<tool>.yml` workflow and an intro at `scripts/release/intros/<tool>.md`. Tags are `<tool>-v<version>`, using just the tool's name. The `salvage` command is the one exception: its tags are `cli-v<version>`. [scripts/release/README.md](scripts/release/README.md) has the details.

## Making a change

1. Branch off an up-to-date `main` and give the branch a descriptive name, like `fix-shader-cache-path`.
2. Keep each pull request to one change. Small pull requests get reviewed faster.
3. In the description, say what changed, why, and how you checked it. If the change is about memory or speed, include numbers from before and after, and the machine you measured on.
4. Make sure the **Public safety** check passes.

## Questions

Open an issue. For anything security-related, use [SECURITY.md](SECURITY.md) instead.
