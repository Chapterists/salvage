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

## Status

Early. Nothing has shipped. We're measuring where Unreal spends memory and where Mac development slows down, and deciding what to build from what we find.

When a tool ships, it will be listed here with before-and-after measurements on real machines. Until then, this page won't describe anything as available.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. This repository is public, and a few checks keep secrets, internal material and engine source out of it.

To report a security problem or a leaked secret, see [SECURITY.md](SECURITY.md). Please don't open a public issue for those.

## License

[MIT](LICENSE). Salvage is made by [Chapterists](https://chapterists.com).

Unreal and Unreal Engine are trademarks or registered trademarks of Epic Games, Inc. in the United States and elsewhere. Salvage is not affiliated with or endorsed by Epic Games.
