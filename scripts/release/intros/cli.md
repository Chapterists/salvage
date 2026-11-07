Salvage is Chapterists' open-source toolbox: the tools we use to make our stories. This release is the `salvage` command, for Unreal Engine work on macOS, Windows and Linux. It has six jobs:

- `salvage doctor` checks a machine for the things that make Unreal slow or broken, and says how to fix each one. It runs 12 checks on a Mac and 6 on Windows and Linux.
- `salvage soundings` reads a memreport or LLM CSV from a game, shows where the memory goes, and measures it against 8, 12 and 16 GB machines. Give it two captures to see what changed.
- `salvage trim` writes memory tiers for 8, 12 and 16 GB machines into a project's config, with the reason for every value. `--undo` takes them out again.
- `salvage refit` finds out why a Windows-only plugin won't work on a Mac, lets Mac through its allow lists, rebuilds it, and checks the build really made Mac binaries. Porting code is still up to you.
- `salvage lldb` makes Xcode and LLDB show Unreal types such as `FString`, `TArray` and `TMap` as values instead of raw memory.
- `salvage clangd` sets up clangd for a project's C++, so VS Code or Neovim can complete and check Unreal code without Xcode's indexer.

Run `salvage <job> --help` for the details of each, and see the [README](https://github.com/Chapterists/salvage#readme) for what each one changes and how to undo it.

Download the archive for your machine, check it against `SHA256SUMS`, extract it, and put `salvage` somewhere on your `PATH`.

| Platform | Archive |
| --- | --- |
| macOS (Apple silicon and Intel, one universal binary) | `salvage-{{version}}-macos-universal.tar.gz` |
| Windows x86_64 | `salvage-{{version}}-windows-x86_64.zip` |
| Linux x86_64 (static, no dependencies) | `salvage-{{version}}-linux-x86_64.tar.gz` |

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing   # sha256sum on Linux
salvage --version
```

### macOS: not signed yet

The macOS binary isn't signed or notarized yet, so macOS blocks it the first time you run a copy downloaded through a browser. Clear the quarantine flag once, in the folder you extracted:

```sh
xattr -d com.apple.quarantine ./salvage
```

Downloads made with `curl` don't get the flag. Signing and notarization will come in a later release.
