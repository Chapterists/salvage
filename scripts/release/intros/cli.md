Salvage is Chapterists' open-source toolbox: the tools we use to make our stories. This release is the `salvage` command, whose first tools are for Unreal Engine: `salvage doctor`, `salvage soundings` and `salvage lldb`.

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

The macOS binary isn't signed or notarized yet, so macOS blocks it the first time you run a copy downloaded through a browser. Clear the quarantine flag once, after extracting:

```sh
xattr -d com.apple.quarantine ./salvage
```

Downloads made with `curl` don't get the flag. Signing and notarization will come in a later release.
