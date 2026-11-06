# LLDB formatters for Unreal types

`salvage_unreal.py` teaches LLDB (and Xcode) to show Unreal Engine types as values instead of raw memory. Install it with:

```sh
salvage lldb install     # adds one marked line to ~/.lldbinit, after backing it up
salvage lldb uninstall   # takes it out again
```

It covers:

- Strings and containers: `FString`, `FUtf8String`, `FName`, `FText`, `TStringView`, `TArray`, `TArrayView`
- Pointers: `TSharedPtr`, `TSharedRef`, `TWeakPtr`, `TUniquePtr`, `TObjectPtr`, `FSoftObjectPath`, `TSoftObjectPtr`, `TSoftClassPtr`
- `TOptional`, `TVariant`, `FGuid`, `FGameplayTag`
- Math: `FVector`, `FVector2D`, `FVector4`, `FRotator`, `FQuat`, `FTransform`, `FIntPoint`, `FIntVector`, `FColor`, `FLinearColor`

It works alongside the engine's own formatters. Where both cover a type, whichever loads last wins.

## How it was written

It was written from scratch for Salvage, using documented behavior and the member names of each type. It contains no code from the engine's formatters or headers. Every formatter looks members up by name and falls back to LLDB's normal display if something doesn't match, so an engine version with a different layout shows raw values, not errors.

## Tests

`tests/fixture.cpp` defines small stand-ins with the same type and member names as the real types. `tests/run_tests.py` builds the fixture twice, once with the game FName layout and once with the editor layout, stops it in LLDB, and checks what each formatter prints. CI runs it on macOS:

```sh
python3 crates/salvage/lldb/tests/run_tests.py
```
