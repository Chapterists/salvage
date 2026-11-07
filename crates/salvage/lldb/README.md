# LLDB formatters for Unreal types

`salvage_unreal.py` teaches LLDB (and Xcode) to show Unreal Engine types as values instead of raw memory. Install it with:

```sh
salvage lldb install     # adds one marked line to ~/.lldbinit, after backing it up
salvage lldb uninstall   # takes it out again
```

It covers:

- Strings and containers: `FString`, `FUtf8String`, `FName`, `FText`, `TStringView`, `TArray`, `TArrayView`, `TSparseArray`, `TSet`, `TMap`, `TMultiMap`
- Pointers: `TSharedPtr`, `TSharedRef`, `TWeakPtr`, `TUniquePtr`, `TObjectPtr`, `FSoftObjectPath`, `TSoftObjectPtr`, `TSoftClassPtr`
- `TOptional`, `TVariant`, `FGuid`, `FGameplayTag`
- Math: `FVector`, `FVector2D`, `FVector4`, `FRotator`, `FQuat`, `FTransform`, `FIntPoint`, `FIntVector`, `FColor`, `FLinearColor`

`TSparseArray`, `TSet`, `TMap` and `TMultiMap` show `Num=` and one child per element, numbered by its slot. Removed elements leave holes in a sparse array, and those slots are skipped, so the numbering can have gaps. A `TMap` child shows its pair as `(key, value)`. The hash a `TSet` keeps alongside its elements isn't shown.

It works alongside the engine's own formatters. Where both cover a type, whichever loads last wins. You'd still want the engine's for `TWeakObjectPtr`, `TBitArray` and `TChunkedArray`, which these don't cover yet. These read the sparse set that `TSet` and `TMap` use by default in UE 5.8; a build that sets `UE_USE_COMPACT_SET_AS_DEFAULT` gets raw values for those.

## How it was written

It was written from scratch for Salvage, using documented behavior and the member names of each type. It contains no code from the engine's formatters or headers. Every formatter looks members up by name and falls back to LLDB's normal display if something doesn't match, so an engine version with a different layout shows raw values, not errors.

## Tests

`tests/fixture.cpp` defines small stand-ins with the same type and member names as the real types. `tests/run_tests.py` builds the fixture twice, once with the game FName layout and once with the editor layout, stops it in LLDB, and checks what each formatter prints. CI runs it on macOS:

```sh
python3 crates/salvage/lldb/tests/run_tests.py
```
