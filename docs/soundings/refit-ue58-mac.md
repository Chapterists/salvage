# salvage refit on Unreal Engine 5.8: PSDImporter and every engine plugin

What `salvage refit` found and did on one Mac: a full scan of the engine's plugins, and Epic's Windows-only PSDImporter rebuilt to run in the Mac editor. This page reports what worked, what didn't, and where.

## Setup

| | |
| --- | --- |
| Machine | Apple M4, 16 GB, macOS 27.2 |
| Engine | Unreal Engine 5.8.4 (Epic's binary build), at `/Users/Shared/Epic Games/UE_5.8` |
| Toolchain | Xcode with the Metal Toolchain, Apple clang |
| Date | November 2026 |

## Why rebuild, not run

A Windows plugin is a set of `.dll` files: Windows' PE format, built for x86-64 against Windows APIs. The Mac editor is a Mach-O process for arm64 (and x86-64). There's no way to load one inside the other, so the only route to Mac is the plugin's source. `salvage refit` finds out how far that route goes for each plugin.

## Scanning every engine plugin

`salvage refit scan --engine-plugins` read all 894 plugins under `Engine/Plugins` in about 7 seconds:

| Verdict | Plugins |
| --- | --- |
| Works on Mac | 721 |
| Content only, works on Mac | 28 |
| Works on Mac without its Windows-only modules | 19 |
| Allow list only, and the Mac binaries are already there: patch | 8 |
| Allow list only: patch, then rebuild | 73 |
| Nothing in the way, just no Mac binaries: rebuild | 3 |
| Needs porting | 42 |
| No source | 0 |

Reading the rows:

- **Patch, then rebuild (73).** 61 of these list Win64 (some with Linux) and not Mac, and the scan found nothing else in the way. Examples: AxFImporter, OpenImageDenoise, PlainProps, HairModelingToolset, StaticMeshEditorModeling, Shotgrid, VirtualScouting, PSDImporterMaterialDesignerBridge. 8 are for phones and headsets only (Android or iOS allow lists, such as the Google Play and Firebase plugins), and 4 for Linux only (the CodeLite, KDevelop and null source code accessors, and the Linux device profile selector). "Nothing else found" means no Windows-only libraries, Windows API calls or Windows-only build rules; it doesn't promise the plugin is useful on a Mac, or that its code builds there without surprises.
- **Patch only (8).** These ship Mac binaries, but their plugin-wide `SupportedTargetPlatforms` leaves Mac out: ChaosCachingUSD, BaseCharacterFXEditor, XRCreativeFramework, InterchangeOpenVDB, MJPEGDecoderElectra, GoogleARCore, PICOController, LiveLinkXR.
- **Rebuild (3).** No list keeps Mac out, but there are no Mac binaries in the binary build: LowLevelNetTrace, EOSOverlayInputProvider, ReplicationSystemTestPlugin.
- **Needs porting (42).** Of these, 39 call Windows APIs outside any Windows-only `#if` block, 19 have build rules that link Windows libraries or refuse other platforms, and 7 ship prebuilt libraries for Windows only. Most of these exist to wrap Windows or a Windows-only SDK, and no rebuild will change that: Media Foundation (MfMedia, WMFCodecs), Direct3D 12 video (D3D12VideoDecodersElectra), the Microsoft GDK and Xbox plugins, XInputDevice, RawInput, WinDualShock, PIX, Visual Studio integration, NVIDIA Reflex. A few look closer: PSDImporter (below) is one.

## Showcase: PSDImporter

PSDImporter (`Engine/Plugins/Experimental/PSDImporter`) imports layered Photoshop files. Epic ships it for Windows only. It comes with full source, and its PSD parser, psd_sdk (BSD-2-clause), has a macOS implementation upstream.

### What the scan said

```text
  ✗ PSDImporter  needs porting
      the plugin: SupportedTargetPlatforms is Win64
      PSDImporterEditor: PlatformAllowList is Win64
      PSDImporter: PlatformAllowList is Win64
      PSDImporterCore: PlatformAllowList is Win64
      No Mac binaries for PSDImporterEditor, PSDImporter, PSDImporterCore
      Source/ThirdParty/PsdSDK: prebuilt for Windows only (…/Win64/x64/Debug/Psd.lib, …/Win64/x64/Release/Psd.lib), no .a, .dylib or .framework
      Windows-only #if blocks with no #else (they compile on Mac, but do nothing there): Source/ThirdParty/PsdSDK/Includes/PsdCompilerMacros.h:139, Source/ThirdParty/PsdSDK/Includes/PsdPlatform.h:62
```

So: four lists to patch, and one library to supply. PSDImporter's own code has no Windows API use.

### The trap, reproduced

Run BuildPlugin on the unpatched plugin with `-TargetPlatforms=Mac`, and it prints `BUILD SUCCESSFUL` in under a second. The log says `Building plugin for target platforms:` with nothing after it, nothing is compiled, and the package has no binaries. `salvage refit check` on that package:

```text
  0 files compiled
  ✗ the plugin's allow lists leave Mac out, so nothing was built for Mac (run salvage refit patch first)
  ✗ BuildPlugin built for no target platform
```

### What it took

1. **Patch.** `salvage refit patch <engine>/Plugins/Experimental/PSDImporter --project Refitted` copied the plugin into a scratch Blueprint project's `Plugins/` folder (the engine's copy is untouched) and added Mac to the plugin's `SupportedTargetPlatforms` and to each module's `PlatformAllowList`. Running it again changed nothing.
2. **The library (a port step, by hand).** Epic's bundled psd_sdk headers match upstream commit `311b5c2` (October 2023) exactly, ignoring whitespace. The current upstream changes struct layouts for large-document support, so it would link but misbehave. That commit built for macOS as a universal static library (arm64 and x86-64) with plain clang in seconds, at first from the sources its CMake file lists for Apple, including `PsdNativeFile_Mac.mm` (see step 7 for why that file was swapped out). The resulting `Psd.a` went into `Source/ThirdParty/PsdSDK/Libraries/Mac/<Release|Debug>/`, and the Mac branch of `PsdSDK.Build.cs`, which already had the line for that path commented out with a TODO, got that line back. A rescan then said "rebuild for Mac".
3. **Build.** `salvage refit build Plugins/PSDImporter --install` ran BuildPlugin (2 minutes 37 seconds, 124 files compiled for Apple silicon and Intel), checked the package and copied the binaries into the plugin:

```text
  124 files compiled
  ✓ PSDImporterEditor  Binaries/Mac/libUnrealEditor-PSDImporterEditor.dylib (x86_64, arm64)
  ✓ PSDImporter  Binaries/Mac/libUnrealEditor-PSDImporter.dylib (x86_64, arm64)
  ✓ PSDImporterCore  Binaries/Mac/libUnrealEditor-PSDImporterCore.dylib (x86_64, arm64)

  Mac binaries for every module.
```

4. **Load.** The Mac editor opened the project and loaded all three modules from the project plugin (`LogModuleManager: InternalLoadLibrary: 'PSDImporterEditor'` and the other two), along with the plugin's content.
5. **Import, first try: a crash.** Importing a two-layer test PSD (written for this test) through the plugin's factory aborted the editor with `std::range_error: wstring_convert: to_bytes error`. psd_sdk's Mac file reader converts the path it's given from `wchar_t` (UTF-32 on a Mac) to UTF-8 with `std::wstring_convert`, which throws on bad input. PSDImporter hands it the path through Unreal's `StringCast<wchar_t>`, and on a Mac, where Unreal's characters are 16-bit and `wchar_t` is 32-bit, that doesn't come out as a clean, terminated UTF-32 string. On Windows both are 16-bit, so this never showed.
6. **The fix (a port step, by hand).** One call site in `PSDImporterCore/Private/PSDFileImport.cpp`: on Mac, build the path as a null-terminated UTF-32 `std::wstring` (joining UTF-16 surrogate pairs) before passing it to psd_sdk.

7. **Import, second try: another crash.** This time the editor died with a SIGBUS on a Grand Central Dispatch worker thread. psd_sdk's 2023 Mac file reader (`PsdNativeFile_Mac.mm`) keeps a block literal for each read in a plain allocated struct without copying it to the heap, then runs it later from a stack frame that's gone. That's a bug in psd_sdk's Mac code, not in Unreal or PSDImporter.
8. **The fix (a port step, by hand).** psd_sdk also has a POSIX file reader (`PsdNativeFile_Linux.cpp`, using `open` and POSIX asynchronous I/O), which macOS supports. Rebuilding `Psd.a` with that file and `PsdStringUtil.cpp` in place of the `.mm`, then running `salvage refit build --install` again, gave a clean package.
9. **Import, third try: it works.** In the Mac editor, the plugin's factory imported the test PSD (64 × 64 pixels, two layers) as a `PSDDocument` asset with one texture per layer, and the editor shut down cleanly:

```text
LogPython: REFIT: imported objects: ['/Game/PSD/Boat.Boat']
LogPython: REFIT: asset /Game/PSD/Boat.Boat class PSDDocument
LogPython: REFIT: asset /Game/PSD/Boat_Layers/T_Boat_Background_0.T_Boat_Background_0 class Texture2D
LogPython: REFIT: asset /Game/PSD/Boat_Layers/T_Boat_Square_1.T_Boat_Square_1 class Texture2D
```

The import ran from a Python script (`-ExecutePythonScript`) with the editor in unattended mode; nobody clicked through the editor's UI, and only the document asset was saved to disk. The quad actors and layered materials PSDImporter can build from a document weren't tried.

### What's left for a real port

- The psd_sdk library should be built and checked in for Mac (and Linux, which the Build.cs also leaves as a TODO), from the commit that matches the headers, with the POSIX file reader rather than the Mac one, or with the Mac one fixed to copy its blocks.
- The path conversion in step 6 belongs in PSDImporterCore for every non-Windows platform.
- The plugin's Mac header (`PsdNativeFile_Mac.h`) goes unused: PSDImporter includes the Windows header on every platform. The layouts are close enough here (an 8-byte handle against a 4-byte file descriptor), but a port should include the header that matches the library.
- The scan's two notes about Windows-only `#if` blocks are in psd_sdk headers (the MSVC-only include of `Windows.h` and an MSVC compiler switch) and need nothing on a Mac.
- Writing PSDs (psd_sdk's export side) wasn't tried.

## What refit does and doesn't do

- **Scan** reads the `.uplugin`, the Mac binaries (`Binaries/Mac/UnrealEditor.modules`), every `*.Build.cs`, prebuilt libraries (`.lib` and `.dll` against `.a`, `.dylib`, `.tbd` and `.framework`), and C and C++ sources. For the sources it strips comments and strings, follows `#if`, `#ifdef`, `#ifndef`, `#elif` and `#else`, and reports Windows headers, Win32 calls and Win32 types outside any Windows-only block, with file and line. These are heuristics: they catch the usual reasons a plugin won't build on a Mac, not every one.
- **Patch** only changes allow lists, deny lists and `SupportedTargetPlatforms`, in place, keeping the file's layout. It never writes inside an engine install, and `--undo` restores the `.uplugin` byte for byte (and removes the copy it made, unless someone has worked in it).
- **Build** runs `RunUAT BuildPlugin -TargetPlatforms=Mac` and checks that each module the editor loads has a Mac binary that's a Mach-O file exporting Unreal's module marker. It also reads the log for the empty target list and the number of files compiled.
- **Porting** stays with a person. Refit lists what's in the way and where; building a third-party library or changing code is up to whoever does the port.
- Tried only on UE 5.8.4 on this Mac. Fab plugins installed into the engine show up in the engine scan. None were installed here, so the engine's own plugins stood in for them.
