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

**`salvage lldb`** makes Xcode and LLDB show Unreal types as values instead of raw memory. This one does change your machine: `install` writes the formatter script to Salvage's data folder and adds one marked line to `~/.lldbinit` (and to `~/.lldbinit-Xcode`, if you have one), after backing each file up. `uninstall` takes both out again.

```sh
salvage lldb install     # adds one marked line to ~/.lldbinit, after backing it up
salvage lldb uninstall   # takes it out again
```

It covers `FString`, `FUtf8String`, `FName`, `FText`, `TStringView`, `TArray`, `TArrayView`, `TSharedPtr`, `TSharedRef`, `TWeakPtr`, `TUniquePtr`, `TObjectPtr`, `FSoftObjectPath`, `TSoftObjectPtr`, `TSoftClassPtr`, `TOptional`, `TVariant`, `FGuid`, `FGameplayTag`, and the math types `FVector`, `FVector2D`, `FVector4`, `FRotator`, `FQuat`, `FTransform`, `FIntPoint`, `FIntVector`, `FColor` and `FLinearColor`. The formatters were written from scratch; [crates/salvage/lldb/README.md](crates/salvage/lldb/README.md) has the details.

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

Early. One tool is in the box, with no release yet. `doctor` and `soundings` are how we find what to fix and check the fixes; neither cuts memory use by itself. When a change does, it will be listed here with before-and-after measurements on real machines.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. This repository is public, and a few checks keep secrets, internal material and engine source out of it.

To report a security problem or a leaked secret, see [SECURITY.md](SECURITY.md). Please don't open a public issue for those.

## License

[MIT](LICENSE). Salvage is made by [Chapterists](https://chapterists.com).

Unreal and Unreal Engine are trademarks or registered trademarks of Epic Games, Inc. in the United States and elsewhere. Salvage is not affiliated with or endorsed by Epic Games.
