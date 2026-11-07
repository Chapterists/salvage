---
title: salvage xcode
date: 2026-11-06
---

An Xcode update can break Unreal builds, and the engine's own check accepts versions Epic says don't work: for 5.8, the engine takes Xcode 15.2 to 27.9, while Epic's page recommends 26.1.1 and calls 26.4 not compatible. `salvage xcode` judges every installed Xcode against both, checks each one's Metal tools, and prints the `DEVELOPER_DIR` to pin the right one for a single build, without sudo or xcode-select. Installing Xcode is left to xcodes and XcodesApp, which already do it well; Salvage prints the command. `salvage doctor` now judges the active Xcode by the same rules.
