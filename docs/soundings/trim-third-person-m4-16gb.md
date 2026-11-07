# salvage trim on the Third Person template, M4 Mac with 16 GB

What `salvage trim` changes in memory and frame time on one machine, with one small project. These are the only numbers behind the tool so far. A small template can't exercise every setting, so read "what wasn't measured" before applying them to a real game.

## Setup

| | |
| --- | --- |
| Machine | Apple M4 (10-core CPU, 10-core GPU), 16 GB unified memory |
| OS | macOS 27.2 (26B123) |
| Engine | Unreal Engine 5.8.4 (58902052), Epic's binary build |
| Project | A fresh copy of the Third Person template (Blueprint), level `Lvl_ThirdPerson`, no changes besides `salvage trim` |
| Build | `-game` from the editor binary (Development, uncooked content), not a packaged build |
| Quality | Default scalability: every group at Epic (3) |
| Window | 1280 x 720 windowed, frame rate capped at 30 fps (`t.MaxFPS 30`) |
| Date | November 2026 |

## Method

Each run launched the game, let the player stand at the start of the level with the default camera (no input), and quit by itself:

```sh
UnrealEditor Project.uproject -game -windowed -resx=1280 -resy=720 -unattended \
  -llmcsv -ExecCmds="t.MaxFPS 30" -csvCaptureFrames=2100 -ExitAfterCsvProfiling \
  -csvExecCmds="900:memreport -full,1800:memreport -full"
```

That gives a memreport about 30 and 60 seconds in, an LLM CSV and an LLM platform CSV sampled once a second, and a CSV profile with per-frame timings. One warm-up run came first so shaders and the derived data cache were filled. Nothing else heavy ran on the machine during the runs.

Configurations, interleaved round by round so drift over time hits all of them equally:

- **Stock**: no `salvage trim`.
- **16 GB tier**: `salvage trim` applied. A 16 GB Mac lands in that tier's memory bucket on its own.
- **12 GB tier** and **8 GB tier**: `salvage trim` applied, with `-MemBucket=3` and `-MemBucket=4` to put the 16 GB machine in the Smaller and Smallest buckets. This applies those tiers' settings; it doesn't make the machine smaller (see below).

Every run's log was checked for the device profile values it pushed, so each configuration got the settings it was meant to.

The final tiers had 4 runs each. A first batch of 5 runs each (stock, each tier and single settings on top of stock) decided which settings stayed; its results are further down.

## Results

Medians of 4 runs, with the lowest and highest in brackets. Differences are against stock. Memory is in MB.

| | Stock | 16 GB tier | 12 GB tier | 8 GB tier |
| --- | --- | --- | --- | --- |
| Process physical memory at 60 s (memreport) | 7687 [7407–7693] | 7582 [7072–7590], −105 | 7081 [7069–7092], −606 | 6583 [6349–6586], −1104 |
| Peak process physical memory (CSV profile, 10–70 s) | 7692 [7682–7694] | 7618 [7111–7631], −74 | 7100 [7096–7103], −592 | 6590 [6586–6598], −1102 |
| Peak LLM tracked total (LLM platform CSV) | 5509 [5509–5509] | 5317 [4805–5317], −192 | 4742 [4742–4742], −767 | 4165 [4165–4165], −1344 |
| Render target pool in use (memreport) | 1828 [1828–1830] | 1830 [1828–1830] | 1428 [1426–1428], −400 | 904 [904–904], −924 |
| Metal buffers (LLM platform CSV) | 576 | 448, −128 | 384, −192 | 320, −256 |

Frame times over frames 300 to 2100 (10 to 70 seconds), medians of the per-run medians, in milliseconds:

| | Stock | 16 GB tier | 12 GB tier | 8 GB tier |
| --- | --- | --- | --- | --- |
| Frame time, median | 33.33 | 33.33 | 33.33 | 33.33 |
| Frame time, 95th percentile | 33.62 | 33.63 | 33.75 | 33.74 |
| Frames over 50 ms (of 1800) | 4 [3–4] | 4 [3–6] | 4 [4–6] | 6 [4–8] |
| GPU time, median | 25.06 | 25.05 | 23.48 | 23.43 |
| Render thread, median | 3.47 | 3.50 | 3.87 | 3.98 |
| Game thread, median | 1.25 | 1.23 | 1.35 | 1.36 |

What `salvage soundings` makes of a stock run and an 8 GB tier run (memreports at 60 s, runs close to the medians):

```text
  Draft  7.5 GB → 6.4 GB at peak   −1.1 GB

  Against smaller machines  25% kept free for the OS and other apps
      8 GB  ✗ trim 1.5 GB       →  ✗ trim 453 MB
     12 GB  ✓ 1.5 GB spare      →  ✓ 2.6 GB spare
     16 GB  ✓ 4.5 GB spare      →  ✓ 5.6 GB spare

  Biggest changes
    total  Render target pool             1.8 GB → 906 MB    −925 MB
```

and the LLM platform CSVs of the same two runs:

```text
  Draft  5.4 GB → 4.1 GB at peak   −1.3 GB

  Biggest changes
    LLM    Metal Textures      2 GB → 512 MB    −1.5 GB
    LLM    Untagged          2.6 GB → 3 GB      +448 MB
    LLM    Metal Buffers     576 MB → 320 MB    −256 MB
```

## Reading the numbers

- **Where the savings come from.** The 16 GB tier is the Nanite streaming pool alone. That pool is allocated up front, so 384 MB instead of 512 MB shows up as 128 MB less in Metal buffers and about 190 MB less tracked memory, in every scene. The 12 GB tier adds a smaller Lumen surface cache atlas (2048 instead of Epic's 4096), which takes 400 MB out of the render target pool. The 8 GB tier also cuts virtual shadow map pages from 4096 to 512: the shadow page pool goes from 512 MB to 64 MB.
- **Process memory drops by 55 to 80% of the tracked change.** On Apple silicon the GPU's allocations come out of the same RAM, so they do show up in process memory, but not one for one: Metal heaps grow in 512 MB steps, and in the 8 GB run above, Untagged grew by 448 MB while Metal textures shrank. The process memory row is the one that decides whether the game fits a machine.
- **The 16 GB tier is bimodal.** In 1 of its 4 runs (and 2 of 5 in the first batch) the Metal texture heap stayed 512 MB smaller, which puts that run 500 MB lower. Stock never did this in 9 runs, and neither did the Nanite pool on its own in 5. It's probably where an allocation falls against the 512 MB heap step, but this test can't say. The median is the number to go by.
- **What it costs.** The frame rate didn't move: every configuration held the 30 fps cap, with the same 95th percentile and the same handful of frames over 50 ms. GPU time went down about 1.6 ms (6%) with the 12 and 8 GB tiers, since a smaller Lumen atlas is less to light. Render thread time went up about 0.4 to 0.5 ms (11 to 15%), still far below the frame budget. Screenshots taken at the same frame in each configuration are the same to within frame-to-frame noise: a mean difference of 0.13 per 8-bit channel from stock, the same for the 16 GB tier (whose Nanite pool can't change anything in this view) as for the 12 and 8 GB tiers. That says nothing about other views: the camera here sees a few metres of floor and one character, with no distant geometry and short shadows.
- **No warnings.** None of the runs logged a virtual shadow map page pool overflow or any other pool running out.

## Settings that were dropped

The first batch measured each setting of the original 16 GB tier on its own, on top of stock (5 runs each, medians):

| Setting on its own | Process memory at 60 s | Peak LLM tracked total |
| --- | --- | --- |
| None (stock) | 7688 | 5510 |
| `r.Nanite.Streaming.StreamingPoolSize=384` | 7610, −78 | 5317, −193 |
| `r.RenderTargetPoolMin=300` | 7669, −19 | 5509, 0 |

- **`r.RenderTargetPoolMin`** (300, 200 and 150 MB in the three tiers) made no difference. It only frees render targets that have gone unused, and everything in this pool is in use every frame. It was dropped, and `salvage soundings` no longer suggests it for a large render target pool.
- **Low-memory garbage collection** (`gc.LowMemory.MemoryThresholdMB` of 2048, 1536 and 1024 MB, with shorter purge intervals) turns on when free memory falls below the threshold. On this Mac, once the level was running, macOS kept free memory between about 1.3 and 1.9 GB, so low-memory mode was on all the time in the 16 and 12 GB tiers. It saved nothing measurable. The 12 and 8 GB tiers measured within 20 MB of their final versions with these settings and the render target setting included. They were dropped.
- **The texture pool pins** of the first version (1000, 768 and 512 MB, plus setting the Mac texture pool's share of GPU memory to 0) were changed before measuring. Epic's texture quality presets set the pool to 400, 600, 800 and 1000 MB, and the preset's value is the pool the game actually gets, so the 1000 MB pin did nothing at Epic and the others added memory at lower presets. Only the 8 GB tier pins the pool now, at 400 MB.

## What wasn't measured

- **Real 8 and 12 GB machines.** The 12 and 8 GB tiers ran on a 16 GB machine. That measures what their settings save, not how a game behaves when the machine itself is short of memory: swapping, memory compression and the OS taking memory back.
- **A packaged build.** `-game` from the editor binary loads uncooked content and carries editor modules, so absolute numbers are higher than a packaged game's. The differences between configurations are what this measures.
- **Settings this template doesn't exercise.** The level's streaming textures need about 10 MB, so the 8 GB tier's 400 MB texture pool never filled. There's no foliage (`foliage.DiscardDataOnLoad`), and there are no streamed sounds to fill the audio stream cache (32 MB instead of 64 MB). The level makes little garbage, so the shorter purge interval (`gc.TimeBetweenPurgingPendingKillObjects` of 45 and 30 seconds) had nothing to show. These stay in the tiers because what they do is well defined, but none has a measured saving yet.
- **The editor preset.** `salvage trim --editor` (shader compile worker limits, fewer workers, static Content Browser thumbnails) wasn't measured. It needs a shader-compile-heavy session, not a game run.
- **Motion, other quality levels and other machines.** The player stood still, everything ran at Epic quality, and it was one machine, one template and one resolution.
