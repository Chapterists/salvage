---
title: salvage lldb
date: 2026-11-06
---

`salvage lldb install` makes Xcode and LLDB show Unreal types such as `FString`, `TObjectPtr`, `TSharedPtr`, `FText` and `FVector` as values instead of raw memory. It adds one marked line to `~/.lldbinit` after backing the file up, and `salvage lldb uninstall` takes it out again. The formatters were written from scratch for Salvage. `salvage doctor` now suggests it when no formatters are loaded.
