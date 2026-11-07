<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/salvage-lockup-dark.svg">
  <img alt="Salvage" src="assets/brand/salvage-lockup-light.svg" width="440">
</picture>

### The tools we use to make our stories.

Salvage is Chapterists' open-source toolbox. Chapterists is a story company, and making stories takes a lot of tools. When one we've built is worth keeping, we haul it up, clean it off and share it here, under the MIT license.

It starts with Unreal Engine: lighter on low-RAM machines, better on a Mac.

## Why a toolbox

Every project leaves good tools behind: a script that fixed a build, a check that caught a problem before it cost a day, a setting that made a slow machine usable. Most of them stay buried in the project that needed them. Salvage is where we bring them up. A tool comes aboard when it's useful beyond the job it was made for.

Tools can be any shape: a command-line program, a Mac app, an Unreal plugin, an editor add-on. Each one is released on its own. More will join as the work on our stories needs them.

## First in the hold: Unreal Engine

Your machine has more in it than you think. RAM is expensive right now, and high-end games keep asking for more of it. A machine gets written off when a game or an editor wants more memory than it has, and upgrading is the usual answer. We would rather find out how much of that memory the work really needs, and cut what it doesn't. We don't sell you a new ship. We get yours sailing.

**Lighter Unreal.** Make Unreal Engine games and the editor run on lower-powered machines, memory first. A lighter is a barge that takes cargo off a ship so it can float into shallow water. That's the job: lighten the load so big games fit small machines.

**Unreal on Mac.** Make developing with Unreal Engine on macOS fast and pleasant.

Fixes that belong inside the engine go upstream to Epic Games as pull requests, so they reach everyone who uses Unreal, not only people who find this project. This repository never contains Unreal Engine source code or Epic content; that stays under Epic's license.

## What's in the box

### `salvage`, a command-line tool

One program with a subcommand for each job. It runs on macOS, Windows and Linux.

**`salvage doctor`** runs 12 checks on a Mac for the things that make Unreal Engine slow or broken: Xcode against the engine's supported range, the Metal Toolchain, Rosetta, Mac shader formats, free space, caches, RAM, swap and the debugger's formatters. Each problem comes with a fix. On Windows and Linux it runs the 6 checks that aren't Mac-specific. It changes nothing; it reads your setup and prints fixes for you to run.

```sh
salvage doctor                      # finds your engine on its own
salvage doctor --project ~/Projects/MyGame
salvage doctor --json
```

**`salvage soundings`** reads a memreport or LLM CSV from an Unreal Engine game, shows where the memory goes, and measures the run against 8, 12 and 16 GB machines. Give it two captures and it shows what changed. It only reads capture files, and it measures memory; it doesn't save any by itself. [docs/benchmark-recipe.md](docs/benchmark-recipe.md) covers how to take captures worth comparing.

```sh
salvage soundings run.memreport                     # where the memory goes
salvage soundings before.memreport after.memreport  # what changed
salvage soundings run.memreport --require 8         # exit 1 unless it fits 8 GB (for CI)
```

**`salvage trim`** writes memory tiers for 8, 12 and 16 GB machines into a project's config: a smaller Nanite streaming pool on all three, a smaller Lumen surface cache and earlier garbage collection on 12 and 8 GB, and fewer virtual shadow map pages and a smaller texture pool on 8 GB. The tiers use Unreal's own device profiles and memory buckets, so each machine size gets its own settings and machines with more than 16 GB keep stock ones. No tier pins a value above what a quality preset would use. Every value has its reason written next to it, with a link to Epic's page for the setting where there is one. `--editor` adds a low-RAM preset for the editor (fewer shader compile workers with a memory limit, static Content Browser thumbnails). It backs up each file before changing it, and `--undo` takes everything out again. [docs/soundings/trim-third-person-m4-16gb.md](docs/soundings/trim-third-person-m4-16gb.md) has before-and-after measurements on one Mac, including what didn't help.

```sh
salvage trim --dry-run   # which files would change, and what would be written; run in the project folder
salvage trim             # write the tiers
salvage trim --editor    # also the low-RAM editor preset
salvage trim --undo      # take everything out again
```

**`salvage refit`** gets Windows-only Unreal plugins running on a Mac, where the source allows it. Windows plugin binaries can't load in the Mac editor, so the way through is rebuilding from source. `scan` says, per plugin, whether it works on Mac and what's in the way: no Mac binaries, allow lists that leave Mac out, prebuilt Windows-only libraries, Windows API calls in the source, or no source at all, with files and lines. `patch` adds Mac to the allow lists on a copy inside your project (never in the engine install), backing up the `.uplugin`, and `--undo` puts it back. `build` runs BuildPlugin for Mac and then checks that every module really came out as a Mac binary, because a plugin that leaves Mac out "builds" on a Mac without compiling anything. [docs/soundings/refit-ue58-mac.md](docs/soundings/refit-ue58-mac.md) shows it on Epic's PSDImporter and across every plugin in UE 5.8.

```sh
salvage refit scan                          # the project's Plugins folder; run in the project folder
salvage refit scan --engine-plugins         # every plugin in the engine, including Fab ones installed there
salvage refit scan Plugins/SomePlugin       # one plugin, with everything in its way
salvage refit patch "/path/to/SomePlugin"   # copy into this project and let Mac through its allow lists
salvage refit build Plugins/SomePlugin --install   # build for Mac, check the binaries, put them in place
salvage refit patch Plugins/SomePlugin --undo      # put the .uplugin back
```

**`salvage lldb`** makes Xcode and LLDB show Unreal types as values instead of raw memory. This one does change your machine: `install` writes the formatter script to Salvage's data folder and adds one marked line to `~/.lldbinit` (and to `~/.lldbinit-Xcode`, if you have one), after backing each file up. `uninstall` takes both out again.

```sh
salvage lldb install     # adds one marked line to ~/.lldbinit, after backing it up
salvage lldb uninstall   # takes it out again
```

It covers `FString`, `FUtf8String`, `FName`, `FText`, `TStringView`, `TArray`, `TArrayView`, `TSharedPtr`, `TSharedRef`, `TWeakPtr`, `TUniquePtr`, `TObjectPtr`, `FSoftObjectPath`, `TSoftObjectPtr`, `TSoftClassPtr`, `TOptional`, `TVariant`, `FGuid`, `FGameplayTag`, and the math types `FVector`, `FVector2D`, `FVector4`, `FRotator`, `FQuat`, `FTransform`, `FIntPoint`, `FIntVector`, `FColor` and `FLinearColor`. The formatters were written from scratch; [crates/salvage/lldb/README.md](crates/salvage/lldb/README.md) has the details.

**`salvage clangd`** sets up clangd for a project's C++, so VS Code or Neovim can complete, navigate and check Unreal code without Xcode's indexer. It runs UnrealBuildTool to write `compile_commands.json` into the project and adds a `.clangd` tuned for Unreal. With `--vscode` or `--neovim` it also writes that editor's settings, backing up anything it replaces. `--refresh` only reruns UnrealBuildTool when build rules or the set of source files changed. Everything it writes is inside the project; delete the files (or restore the backups) to undo it. It's been tried on macOS with Epic's C++ templates. Windows and Linux are covered by unit tests only, and it hasn't been tried on a large project yet.

```sh
salvage clangd --vscode            # or --neovim; run in the project folder
salvage clangd --vscode --refresh  # after adding source files or changing Build.cs
```

#### Get it

There's no release yet, so build it from source. You need Git and a stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone https://github.com/Chapterists/salvage.git
cd salvage
cargo install --locked --path crates/salvage   # puts salvage in ~/.cargo/bin
```

Or run it from the repository without installing: `cargo run -p salvage -- doctor`. To remove it, run `salvage lldb uninstall` if you installed the formatters, then `cargo uninstall salvage`.

Once there's a release, prebuilt binaries for macOS, Windows and Linux will be on the [Releases page](https://github.com/Chapterists/salvage/releases), tagged `cli-v<version>`, with checksums. The macOS binary won't be signed at first; the release notes show how to clear the quarantine flag.

## Status

Early. One tool is in the box, with no release yet. `doctor` and `soundings` are how we find what to fix and check the fixes; neither cuts memory use by itself. `trim` is the first tool that does, and its measurements so far are in [docs/soundings](docs/soundings/).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. This repository is public, and a few checks keep secrets, internal material and engine source out of it.

To report a security problem or a leaked secret, see [SECURITY.md](SECURITY.md). Please don't open a public issue for those.

## License

[MIT](LICENSE). Salvage is made by [Chapterists](https://chapterists.com).

Unreal and Unreal Engine are trademarks or registered trademarks of Epic Games, Inc. in the United States and elsewhere. Salvage is not affiliated with or endorsed by Epic Games.
