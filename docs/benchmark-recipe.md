# Capturing memory runs for `salvage soundings`

This is a recipe for taking memory captures from an Unreal Engine game that you can compare with each other. `salvage soundings` reads two kinds of capture: memreport files and LLM CSVs. Each answers a different question, so it's worth knowing which one you need.

| Capture | What it tells you | Build | Where it lands |
| --- | --- | --- | --- |
| `memreport` | A snapshot: process memory, GPU resources by rendering system, textures, render targets, biggest object classes | Development or Test | `Saved/Profiling/MemReports/*.memreport` |
| `-llmcsv` | A timeline: memory per LLM tag, sampled once a second, so you get peaks as well as end values | Development (Test only if built with `ALLOW_LOW_LEVEL_MEM_TRACKER_IN_TEST`) | `Saved/Profiling/LLM/LLM_*.csv` and `LLMPlatform_*.csv` |

Neither works in Shipping builds. Development builds use somewhat more memory than Shipping (stats, the console, debug helpers), so treat the numbers as a slightly pessimistic ceiling.

## Before you capture

Memory numbers move with almost everything. To make two runs comparable, keep these the same between them:

- **Machine and OS.** Same machine, and close other apps. Restarting first is the simplest way to get a clean baseline.
- **Build.** Same configuration (Development is the easy choice, since both captures work there), same platform, packaged or `-game`. Don't measure from inside the editor: the editor's own memory swamps the game's.
- **Content and settings.** Same map, same scalability level (`sg.*` or `-ScalabilityLevel`), same resolution and window mode.
- **Warm caches.** Run once before you capture so shaders are compiled and the derived data cache is filled. The first launch after a change measures compilation, not your game.
- **Where and when.** Capture at the same place in the level and the same moment in your test. A fixed camera or a scripted flythrough helps.

## Taking a memreport

1. Launch the game, load the level, and wait until streaming settles (30 to 60 seconds after the level appears is usually enough).
2. Open the console and run `memreport`. Use `memreport -full` for a longer report with more sections; soundings reads both.
3. Take it at a quiet moment and again at the heaviest moment of your test (the busiest scene, the most characters on screen). The second one is usually the one that decides whether you fit.
4. Take two or three per run if you can. Captures a few seconds apart show you how noisy the numbers are.

Which commands a memreport runs is set by `[MemReportCommands]` (and `[MemReportFullCommands]` for `-full`) in the engine's config. Soundings reads these sections when they're present:

- `Mem FromReport`: process physical memory now and at peak. This is the draft.
- `rhi.DumpResourceMemory` and its `summary name=...` variants: GPU resources by system (Lumen, Nanite, Shadow, DistanceFields, GPUScene, VirtualTexture and so on).
- `listtextures`: textures in memory.
- `r.DumpRenderTargetPoolMemory`: the render target pool.
- `obj list -resourcesizesort`: object classes by the memory they and their resources use.

## Taking an LLM capture

1. Launch with `-llmcsv`. That turns on the Low Level Memory tracker and writes a row per tag once a second (`LLM.LLMWriteInterval` changes the interval).
2. Play through your test, then quit normally so the files are flushed.
3. Use `LLM_*.csv` for where memory goes by system. Use `LLMPlatform_*.csv` for the total, which includes memory LLM can't attribute ("Untracked").

LLM costs some memory and CPU itself, so compare LLM runs with LLM runs, never with memreports from runs without it. For a live view while you play, `stat LLM`, `stat LLMFULL` and `stat LLMPlatform` show the same numbers on screen.

For allocation-level detail (call stacks, what grew between two moments), record a trace with `-trace=default,memory` and open it in Unreal Insights. Soundings doesn't read traces.

## Capturing without touching the keyboard

For runs that should match each other closely, let the engine take the captures at fixed frames and then quit. The CSV profiler can run console commands at a given frame number, and capping the frame rate turns frame numbers into rough times:

```sh
UnrealEditor MyGame.uproject -game -windowed -resx=1280 -resy=720 \
  -llmcsv -ExecCmds="t.MaxFPS 30" \
  -csvCaptureFrames=2100 -ExitAfterCsvProfiling \
  -csvExecCmds="900:memreport -full,1800:memreport -full"
```

At 30 frames a second, that takes a memreport about 30 and 60 seconds in, then quits at about 70 seconds. On macOS the editor binary is inside `UnrealEditor.app/Contents/MacOS/`. Running with `-game` from the editor binary loads uncooked content, so absolute numbers are higher than a cooked build's; compare runs made the same way.

## Reading the results

```sh
# One capture: the draft, how it sits on 8, 12 and 16 GB machines, and what's big
salvage soundings Harbor-Mac-11.06-14.10.memreport

# Two captures: what changed from the first to the second
salvage soundings before.memreport after.memreport

# Other machine sizes, or a different allowance for the OS and other apps
salvage soundings run.memreport --target 6 --target 8 --headroom 30

# In CI: exit 1 unless the capture fits an 8 GB machine
salvage soundings run.memreport --require 8

# Machine-readable
salvage soundings run.memreport --json
```

The **draft** is the run's peak process memory (or its current memory if no peak is recorded). Soundings measures it against each target machine after keeping back a share for the OS and everything else, 25% by default. That share is a rule of thumb, not an Epic number: raise it if your players will have a browser open, lower it for a dedicated device.

Pools are shown by view: headline totals, GPU systems, LLM tags, and object classes. Views overlap (a texture is an object, an RHI resource and a texture all at once), so compare pools within a view and don't add views together. Pools that take a large share of the draft are flagged with the setting that usually sizes them, such as `r.Streaming.PoolSize` for textures or `r.Nanite.Streaming.StreamingPoolSize` for Nanite.

## Caveats

- On Apple silicon the CPU and GPU share memory, so GPU resources count against the same budget as everything else. On PCs with a discrete GPU, most of the GPU numbers live in video memory instead.
- Process memory as the platform reports it isn't the same as what LLM tracks. Compare like with like.
- A single capture is a single moment. When a change saves less than the noise between repeated captures, take more captures before you trust it.
