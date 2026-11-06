<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/salvage-lockup-dark.svg">
  <img alt="Salvage" src="assets/brand/salvage-lockup-light.svg" width="440">
</picture>

### Your machine has more in it than you think.

RAM is expensive right now, and high-end games keep asking for more of it. Salvage is a set of open-source tools and engine fixes that get more out of the hardware people already have, starting with Unreal Engine.

We don't sell you a new ship. We get yours sailing.

## Why

A machine gets written off when a game or an editor wants more memory than it has. Upgrading is the usual answer, and right now it's an expensive one. We would rather find out how much of that memory the work really needs, and cut what it doesn't. A fix like that helps everyone: the person on an old laptop gets a game that runs, and the person on a big workstation gets their memory back.

## Two workstreams

**Lighter Unreal.** Make Unreal Engine games and the editor run on lower-powered machines, memory first. A lighter is a barge that takes cargo off a ship so it can float into shallow water. That's the job: lighten the load so big games fit small machines.

**Unreal on Mac.** Make developing with Unreal Engine on macOS fast and pleasant.

## How it's built

Standalone tools live in this repository. Fixes that belong inside the engine go upstream to Epic Games as pull requests, so they reach everyone who uses Unreal and not only people who find this project.

This repository never contains Unreal Engine source code or Epic content. That stays under Epic's license.

## Tools

Two tools are in the repository, both subcommands of one program, `salvage`. Neither changes anything on your machine: they read and report.

**`salvage doctor`** runs 12 checks on a Mac for the things that make Unreal Engine slow or broken: Xcode against the engine's supported range, the Metal Toolchain, Rosetta, Mac shader formats, free space, caches, RAM, swap and the debugger's formatters. Each problem comes with a fix. On Windows and Linux it runs the 6 checks that aren't Mac-specific.

```sh
salvage doctor                      # finds your engine on its own
salvage doctor --project ~/Projects/MyGame
salvage doctor --json
```

**`salvage soundings`** reads a memreport or LLM CSV from an Unreal Engine game, shows where the memory goes, and measures the run against 8, 12 and 16 GB machines. Give it two captures and it shows what changed. It measures memory; it doesn't save any by itself. [docs/benchmark-recipe.md](docs/benchmark-recipe.md) covers how to take captures worth comparing.

```sh
salvage soundings run.memreport                     # where the memory goes
salvage soundings before.memreport after.memreport  # what changed
salvage soundings run.memreport --require 8         # exit 1 unless it fits 8 GB (for CI)
```

## Get it

There's no release yet, so build from source. You need Git and a stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
git clone https://github.com/Chapterists/salvage.git
cd salvage
cargo install --locked --path crates/salvage   # puts salvage in ~/.cargo/bin
```

Or run it from the repository without installing: `cargo run -p salvage -- doctor`. To remove it: `cargo uninstall salvage`.

## Status

Early. The two tools above are how we find what to fix and check the fixes; neither one cuts memory use by itself. When a change does, it will be listed here with before-and-after measurements on real machines.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. This repository is public, and a few checks keep secrets, internal material and engine source out of it.

To report a security problem or a leaked secret, see [SECURITY.md](SECURITY.md). Please don't open a public issue for those.

## License

[MIT](LICENSE). Salvage is made by [Chapterists](https://chapterists.com).

Unreal and Unreal Engine are trademarks or registered trademarks of Epic Games, Inc. in the United States and elsewhere. Salvage is not affiliated with or endorsed by Epic Games.
