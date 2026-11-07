# salvage crash on real crashes: an Unreal C++ project and a plain C program

What `salvage crash` showed for crashes made on purpose on one Mac, and what Unreal's own crash report shows for the same crashes.

## Setup

| | |
| --- | --- |
| Machine | Apple M4, macOS 27.2 |
| Engine | Unreal Engine 5.8.4 (Epic's binary build) |
| Project | A scratch C++ project ("Wreck") with one module, built as an editor target (Development, arm64), run with `-game` |
| Date | November 2026 |

The module registers three console commands, each in its own function that isn't inlined:

- `wreck.crash`: `CountPlanks(FHull*)` reads `Hull->Planks` through a null pointer (Wreck.cpp line 15).
- `wreck.check`: `FailCheck()` runs `check(GHull != nullptr)` (Wreck.cpp line 25).
- `wreck.ensure`: `RaiseEnsure()` runs `ensureMsgf(...)`.

Each run started the game with `-ExecCmds=<command>`. The project's settings turned off unattended crash reports and usage data. CrashReportClient's dialog was closed without sending once the crash folder was on disk, so nothing went to Epic. A separate C program, built with `clang -g` and a dSYM, dereferenced null for a macOS `.ips` report.

## What Unreal's own report shows

For the null dereference, the crash folder's symbolized call stack (`CallStack` in `CrashContext.runtime-xml`) starts at `FConsoleCommand::Execute`. The two frames in the game's own module, the ones that matter, aren't there. The portable call stack (`PCallStack`) has them, but as `Unknown 0x0000000000000000 + cea859a8`: no module, and only the low 32 bits of an address in a dylib loaded above 4 GB. The text report in the folder (`minidump.dmp`, which on a Mac is Apple's text crash format, not a minidump) names thread 44, Unreal's crash-handling thread (SIGUSR2), as the one that crashed. The game thread's frames aren't in it.

`salvage crash` takes the crashing thread from the portable call stack. It rebuilds each truncated address from the one image whose range holds it, preferring the game's dylibs when a macOS framework in the shared cache also happens to fit. Then it matches each image by UUID.

## Results

The null dereference (output trimmed where it says …, paths shortened):

```text
  salvage crash  CrashReport-UE-Wreck-pid-73399-… (Unreal crash folder)
  UnrealEditor · Crash: Caught signal · 2026-11-07 00:54:29 UTC
  Signature f3dc4c8ec224  UnrealEditor-Wreck!CountPlanks(FHull*)  ←  UnrealEditor-Wreck!TBaseFunctorDelegateInstance<void (), FDefaultDelegateUserPolicy, $_1>::ExecuteIfSafe() const

  Crashed thread
    0  CountPlanks(FHull*)  Wreck.cpp:15  UnrealEditor-Wreck
    1  TBaseFunctorDelegateInstance<void (), FDefaultDelegateUserPolicy, $_1>::ExecuteIfSafe() const  DelegateInstancesImpl.h:804  UnrealEditor-Wreck
    2  FConsoleCommand::Execute(TArray<FString, TSizedDefaultAllocator<32>> const&, UWorld*, FOutputDevice&)  UnrealEditor-Core
    …
   12  -[FCocoaGameThread main]  UnrealEditor-Core
   13  __NSThread__start__  Foundation
   14  _pthread_start  libsystem_pthread.dylib

  Symbols
    ✓ libUnrealEditor-Core.dylib  binary …/UE_5.8/Engine/Binaries/Mac/libUnrealEditor-Core.dylib
    ✓ libUnrealEditor-Wreck.dylib  binary …/Wreck/Binaries/Mac/libUnrealEditor-Wreck.dylib
    · Foundation  macOS library, from this Mac's shared cache (same macOS build)
```

Line 15 is the null dereference. The local build had no dSYM; `atos` got file and line from the binary's debug map, which points at the object files still in the project's `Intermediate` folder. The engine's binaries give function names but no files or lines, because Epic's binary build ships without dSYMs.

The failed `check()` showed `FailCheck()  Wreck.cpp:25` as frame 5. Frames 0 to 4 are the engine's assert plumbing (`FGenericPlatformMisc::RaiseException`, `FDebug::CheckVerifyFailedImpl2` and the like); they're shown, but left out of the signature.

The C program's `.ips`, where macOS had redacted the path to `/private/tmp/*/sinker`: salvage found the dSYM by searching under the part of the path that was kept, and matched its UUID:

```text
  Crashed thread
    0  count_planks  sinker.c:6  sinker
    1  sink  sinker.c:10  sinker
    2  main  sinker.c:15  sinker
    3  start  dyld
```

All three lines are the right ones in the source.

## Grouping

`salvage crash list` on the project, after three null dereferences and one failed check:

```text
  salvage crash list  Wreck: 4 crashes in 2 groups

  f3dc4c8ec224   3×  Crash  2026-11-07 00:54:00 UTC to 2026-11-07 00:54:57 UTC
         UnrealEditor-Wreck!CountPlanks(FHull*)
         UnrealEditor-Wreck!TBaseFunctorDelegateInstance<void (), FDefaultDelegateUserPolicy, $_1>::ExecuteIfSafe() const

  c13d1354f736   1×  Assert  2026-11-07 00:55:19 UTC
         UnrealEditor-Wreck!FailCheck()
         UnrealEditor-Wreck!TBaseFunctorDelegateInstance<void (), FDefaultDelegateUserPolicy, $_2>::ExecuteIfSafe() const
```

## A different build

After a one-line change and a rebuild, the dylib had a new UUID. Reading the old crash then said so, instead of printing wrong lines:

```text
    ! libUnrealEditor-Wreck.dylib  no match: …/Wreck/Binaries/Mac/libUnrealEditor-Wreck.dylib from a different build (UUID C2C742D0-FFA8-3433-A7FA-3335D1F62885)
```

Unsymbolicated frames fall back to offsets in their image (`UnrealEditor-Wreck!+0x59a8`). So crashes from a build whose symbols are gone group by offset, separately from the same crash with symbols. Keep the dSYMs or binaries of builds you ship.

## What wasn't covered

- **Ensures.** `wreck.ensure` was logged as a handled ensure with a call stack, but no crash folder was written while the game kept running, so `salvage crash` had nothing to read. Ensures in a crash folder go through the same path as asserts, but that wasn't seen here.
- **UnrealAtoS.** It needs `.udebugsymbols` files, which Unreal makes for iOS and tvOS builds; none existed here, so that path is covered only by code that calls it, not by a real run.
- **Packaged games.** Only `-game` runs of the editor binary were crashed. A packaged game's single binary and its dSYM go through the same matching, but no packaged crash was tried.
- **Other Macs.** Reports from another macOS build can't use this Mac's shared cache for system libraries, so those frames keep whatever names the report gives.
