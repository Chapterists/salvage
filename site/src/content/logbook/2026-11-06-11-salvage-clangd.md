---
title: salvage clangd
date: 2026-11-06
---

`salvage clangd` sets up clangd for an Unreal project's C++, so VS Code or Neovim can complete, navigate and check the code without Xcode's indexer. It runs UnrealBuildTool to write the compile database, adds a `.clangd` tuned for Unreal, and with `--refresh` skips UnrealBuildTool when nothing relevant changed. On Epic's Third Person template on a 16 GB M4, the first run took 7 seconds and an unchanged refresh 0.01 seconds, and clangd found 0 errors. Windows and Linux are unit-tested only, and it hasn't been tried on a large project yet.
