---
title: salvage refit, and a PSD imported on a Mac
date: 2026-11-06
---

`salvage refit` answers a question Mac users of Unreal ask a lot: can I use this Windows plugin? Not as it is, but often you can rebuild it. Refit says what's in a plugin's way, lets Mac through its allow lists on a copy in your project, builds it, and checks that the build really made Mac binaries, because a plugin that leaves Mac out "builds" without compiling anything. A scan of all 894 plugins in Unreal Engine 5.8.4 found 768 that work on a Mac, 84 that need a patch, a rebuild or both, and 42 that need porting. One of those 42, Epic's PSDImporter, took a library build and two crash fixes by hand, then imported a layered PSD in the Mac editor. The sounding is plain about what wasn't tried: nobody clicked through the editor's UI, and the fixes haven't been offered to Epic.
