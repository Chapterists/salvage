// Test program for salvage_unreal.py. These are Salvage's own minimal
// stand-ins for Unreal types: same type and member names, so the formatters
// can't tell the difference, but none of Unreal's code.
//
// Built twice: once as a game build would lay out FName, and once with
// -DCASE_PRESERVING_NAME the way an editor build does.

#include <atomic>
#include <cstdint>
#include <cstring>
#include <new>

using int32 = int32_t;
using uint8 = uint8_t;
using uint16 = uint16_t;
using uint32 = uint32_t;
using TCHAR = char16_t;

// --- containers and strings -------------------------------------------------------

template <typename T>
struct TSizedHeapAllocatorData {
    T* Data = nullptr;
};

template <typename T, int N>
struct TInlineAllocatorData {
    alignas(T) uint8 InlineData[sizeof(T) * N];
    TSizedHeapAllocatorData<T> SecondaryData;
};

template <typename T, typename Allocator = TSizedHeapAllocatorData<T>>
struct TArray {
    Allocator AllocatorInstance{};
    int32 ArrayNum = 0;
    int32 ArrayMax = 0;
};

template <typename T>
TArray<T> MakeArray(const T* values, int32 count) {
    TArray<T> array;
    array.AllocatorInstance.Data = new T[count];
    for (int32 i = 0; i < count; ++i) array.AllocatorInstance.Data[i] = values[i];
    array.ArrayNum = array.ArrayMax = count;
    return array;
}

struct FString {
    TArray<TCHAR> Data;
};

FString MakeString(const char16_t* text) {
    int32 length = 0;
    while (text[length]) ++length;
    return FString{MakeArray(text, length + 1)};  // stored with its NUL, like the engine
}

struct FUtf8String {
    TArray<char> Data;
};

template <typename CharType>
struct TStringView {
    const CharType* DataPtr = nullptr;
    int32 Size = 0;
};

template <typename T>
struct TArrayView {
    T* DataPtr = nullptr;
    int32 ArrayNum = 0;
};

// --- sparse arrays, sets and maps ---------------------------------------------------

// One bit per slot, in 32-bit words: four inline, more on the heap.
struct TBitArray {
    TInlineAllocatorData<uint32, 4> AllocatorInstance;
    int32 NumBits = 0;
    int32 MaxBits = 0;
};

template <int Size, int Align>
struct TAlignedBytes {
    alignas(Align) uint8 Pad[Size];
};

// A slot holds an element, or (once removed) a link in the free list.
template <typename T>
union TSparseArrayElementOrFreeListLink {
    T ElementData;
    struct {
        int32 PrevFreeIndex;
        int32 NextFreeIndex;
    };
};

template <typename T>
struct TSparseArray {
    TArray<TSparseArrayElementOrFreeListLink<TAlignedBytes<sizeof(T), alignof(T)>>> Data;
    TBitArray AllocationFlags;
    int32 FirstFreeIndex = -1;
    int32 NumFreeIndices = 0;
};

// Fills slots 0..count-1, then removes the slots in `removed`.
template <typename T>
TSparseArray<T> MakeSparse(const T* values, int32 count, const int32* removed, int32 removed_count) {
    using Slot = TSparseArrayElementOrFreeListLink<TAlignedBytes<sizeof(T), alignof(T)>>;
    TSparseArray<T> array;
    array.Data.AllocatorInstance.Data = new Slot[count]();
    array.Data.ArrayNum = array.Data.ArrayMax = count;
    int32 words = (count + 31) / 32;
    uint32* bits = words > 4 ? new uint32[words]() : reinterpret_cast<uint32*>(array.AllocationFlags.AllocatorInstance.InlineData);
    if (words > 4) array.AllocationFlags.AllocatorInstance.SecondaryData.Data = bits;
    else std::memset(bits, 0, sizeof(uint32) * 4);
    array.AllocationFlags.NumBits = array.AllocationFlags.MaxBits = count;
    for (int32 i = 0; i < count; ++i) {
        new (&array.Data.AllocatorInstance.Data[i].ElementData) T(values[i]);
        bits[i / 32] |= 1u << (i % 32);
    }
    for (int32 r = 0; r < removed_count; ++r) {
        int32 i = removed[r];
        bits[i / 32] &= ~(1u << (i % 32));
        Slot& slot = array.Data.AllocatorInstance.Data[i];
        std::memset(&slot, 0xAB, sizeof(Slot));  // leftover bytes a formatter must not show
        slot.PrevFreeIndex = -1;
        slot.NextFreeIndex = array.FirstFreeIndex;
        array.FirstFreeIndex = i;
        ++array.NumFreeIndices;
    }
    return array;
}

struct FSetElementId {
    int32 Index;
};

template <typename T>
struct TSparseSetElementBase {
    T Value;
    FSetElementId HashNextId;
    int32 HashIndex;
};

template <typename T>
struct TSetElement : TSparseSetElementBase<T> {};

template <typename T>
struct TSet {
    TSparseArray<TSetElement<T>> Elements;
    TInlineAllocatorData<FSetElementId, 1> Hash;
    int32 HashSize = 0;
};

template <typename T>
TSet<T> MakeSet(const T* values, int32 count, const int32* removed, int32 removed_count) {
    TSetElement<T> elements[64];
    for (int32 i = 0; i < count; ++i) elements[i] = TSetElement<T>{{values[i], {-1}, 0}};
    return TSet<T>{MakeSparse(elements, count, removed, removed_count), {}, 0};
}

template <typename K, typename V>
struct TTuple {
    K Key;
    V Value;
};

template <typename K, typename V>
struct TMapBase {
    TSet<TTuple<K, V>> Pairs;
};

template <typename K, typename V>
struct TSortableMapBase : TMapBase<K, V> {};

template <typename K, typename V>
struct TMap : TSortableMapBase<K, V> {};

template <typename K, typename V>
struct TMultiMap : TSortableMapBase<K, V> {};

template <typename Map, typename K, typename V>
Map MakeMap(const TTuple<K, V>* pairs, int32 count, const int32* removed, int32 removed_count) {
    Map map;
    map.Pairs = MakeSet(pairs, count, removed, removed_count);
    return map;
}

// --- FName and its name pool -----------------------------------------------------

struct FNameEntryId {
    uint32 Value;
};

struct FName {
    FNameEntryId ComparisonIndex{0};
    uint32 Number = 0;
#ifdef CASE_PRESERVING_NAME
    FNameEntryId DisplayIndex{0};
#endif
};

alignas(2) static uint8 GNameBlock0[1024];
alignas(2) static uint8 GNameBlock1[1024];
static uint8* GNameBlocks[2] = {GNameBlock0, GNameBlock1};
static uint32 GNameCursor[2] = {0, 0};

struct FNameDebugVisualizer {
    __attribute__((used, noinline)) static uint8** GetBlocks() { return GNameBlocks; }
};

// Adds an entry the way the pool stores it: a 2-byte header, then the characters.
FName AddName(const char* text, uint32 block = 0, uint32 number = 0) {
    uint16 length = (uint16)strlen(text);
#ifdef CASE_PRESERVING_NAME
    uint16 header = (uint16)(length << 1);
#else
    uint16 header = (uint16)((length << 6) | (3 << 1));  // a made-up probe hash in bits 1-5
#endif
    uint32 offset = GNameCursor[block];
    memcpy(GNameBlocks[block] + offset, &header, 2);
    memcpy(GNameBlocks[block] + offset + 2, text, length);
    GNameCursor[block] += (2 + length + 1) & ~1u;
    FName name;
    name.ComparisonIndex.Value = (block << 16) | (offset / 2);
    name.Number = number;
#ifdef CASE_PRESERVING_NAME
    name.DisplayIndex = name.ComparisonIndex;
#endif
    return name;
}

// --- smart pointers ----------------------------------------------------------------

struct FReferenceControllerBase {
    virtual ~FReferenceControllerBase() = default;
    std::atomic<int32> SharedReferenceCount{1};
    std::atomic<int32> WeakReferenceCount{1};
};

struct FSharedReferencer {
    FReferenceControllerBase* ReferenceController = nullptr;
};
struct FWeakReferencer {
    FReferenceControllerBase* ReferenceController = nullptr;
};

template <typename T>
struct TSharedPtr {
    T* Object = nullptr;
    FSharedReferencer SharedReferenceCount;
};
template <typename T>
struct TSharedRef {
    T* Object = nullptr;
    FSharedReferencer SharedReferenceCount;
};
template <typename T>
struct TWeakPtr {
    T* Object = nullptr;
    FWeakReferencer WeakReferenceCount;
};

struct FDefaultDelete {};
template <typename T, typename Deleter = FDefaultDelete>
struct TUniquePtr : private Deleter {
    T* Ptr = nullptr;
};

struct FCrate {
    int32 Weight = 0;
    FString Label;
};

// --- UObjects ----------------------------------------------------------------------

struct UObject;
struct UClass;

struct FObjectHandlePrivate {
    uintptr_t PointerOrRef = 0;
};

struct FObjectPtr {
    union {
        FObjectHandlePrivate Handle;
        UObject* DebugPtr;
    };
    FObjectPtr() : Handle{} {}
};

template <typename T>
struct TObjectPtr {
    union {
        FObjectPtr ObjectPtr;
        T* DebugPtr;
    };
    TObjectPtr() : ObjectPtr() {}
};

namespace ObjectPtr_Private {
template <typename T>
struct TNonAccessTrackedObjectPtr {
    FObjectPtr ObjectPtr;
};
}  // namespace ObjectPtr_Private

struct UObjectBase {
    virtual ~UObjectBase() = default;
    int32 ObjectFlags = 0;
    int32 InternalIndex = 0;
    ObjectPtr_Private::TNonAccessTrackedObjectPtr<UClass> ClassPrivate;
    struct FNameAndObjectHashIndex {
        FName Name;
    } NamePrivate;
    ObjectPtr_Private::TNonAccessTrackedObjectPtr<UObject> OuterPrivate;
};
struct UObject : UObjectBase {};
struct UClass : UObject {};
struct AActor : UObject {
    int32 Health = 100;
};

// --- FText -------------------------------------------------------------------------

struct ITextData {
    virtual ~ITextData() = default;
};
struct FTextHistory {
    FString SourceString;
};
struct FLocalizedTextData : ITextData {
    FTextHistory History;
    TSharedPtr<FString> LocalizedString;
};

template <typename T>
struct TRefCountPtr {
    T* Reference = nullptr;
};

struct FText {
    TRefCountPtr<ITextData> TextData;
    uint32 Flags = 0;
};

// --- TOptional, TVariant -----------------------------------------------------------

template <typename T>
struct TOptional {
    struct FUnsetType {};
    union {
        FUnsetType UnsetValue;
        T TypedValue;
    };
    bool bIsSet = false;
    TOptional() : UnsetValue() {}
    explicit TOptional(const T& value) : TypedValue(value), bIsSet(true) {}
};

template <typename... Ts>
struct TVariantStorage {
    alignas(8) uint8 Storage[32];
};

template <typename T, typename... Ts>
struct TVariant : private TVariantStorage<T, Ts...> {
    uint8 TypeIndex = 0;
    template <typename U>
    void Set(const U& value, uint8 index) {
        new (this->Storage) U(value);
        TypeIndex = index;
    }
};

// --- soft references, tags, ids ----------------------------------------------------

struct FTopLevelAssetPath {
    FName PackageName;
    FName AssetName;
};
struct FSoftObjectPath {
    FTopLevelAssetPath AssetPath;
    FUtf8String SubPathString;
};
struct FWeakObjectPtr {
    int32 ObjectIndex = 0;
    int32 ObjectSerialNumber = 0;
};
struct FSoftObjectPtr {
    FWeakObjectPtr WeakPtr;
    FSoftObjectPath ObjectID;
};
template <typename T>
struct TSoftObjectPtr {
    FSoftObjectPtr SoftObjectPtr;
};

struct FGameplayTag {
    FName TagName;
};

struct FGuid {
    uint32 A, B, C, D;
};

// --- math --------------------------------------------------------------------------

namespace UE::Math {
template <typename T>
struct TVector {
    T X, Y, Z;
};
template <typename T>
struct TVector2 {
    T X, Y;
};
template <typename T>
struct alignas(16) TVector4 {
    T X, Y, Z, W;
};
template <typename T>
struct alignas(16) TQuat {
    T X, Y, Z, W;
};
template <typename T>
struct TRotator {
    T Pitch, Yaw, Roll;
};
template <typename T>
struct VectorRegister {
    T XYZW[4];
};
template <typename T>
struct alignas(16) TTransform {
    VectorRegister<T> Rotation;
    VectorRegister<T> Translation;
    VectorRegister<T> Scale3D;
};
template <typename T>
struct TIntPoint {
    T X, Y;
};
template <typename T>
struct TIntVector3 {
    T X, Y, Z;
};
}  // namespace UE::Math

using FVector = UE::Math::TVector<double>;
using FVector2D = UE::Math::TVector2<double>;
using FVector4 = UE::Math::TVector4<double>;
using FQuat = UE::Math::TQuat<double>;
using FRotator = UE::Math::TRotator<double>;
using FTransform = UE::Math::TTransform<double>;
using FIntPoint = UE::Math::TIntPoint<int32>;
using FIntVector = UE::Math::TIntVector3<int32>;

struct FColor {
    uint8 B, G, R, A;
};
struct FLinearColor {
    float R, G, B, A;
};

// --- the scene ---------------------------------------------------------------------

__attribute__((noinline)) void break_here() { asm volatile(""); }

int main() {
    FString greeting = MakeString(u"Ahoy, Unreal");
    FString empty_string;
    FUtf8String utf8{MakeArray("caf\xc3\xa9", 6)};
    TStringView<TCHAR> view{u"partial view here", 7};

    const int32 numbers_data[] = {3, 1, 4, 1, 5};
    TArray<int32> numbers = MakeArray(numbers_data, 5);
    TArray<int32> no_numbers;
    TArrayView<int32> numbers_view{numbers.AllocatorInstance.Data + 1, 3};

    TArray<int32, TInlineAllocatorData<int32, 4>> inline_numbers;
    reinterpret_cast<int32*>(inline_numbers.AllocatorInstance.InlineData)[0] = 7;
    reinterpret_cast<int32*>(inline_numbers.AllocatorInstance.InlineData)[1] = 8;
    inline_numbers.ArrayNum = 2;
    inline_numbers.ArrayMax = 4;

    const int32 sparse_values[] = {10, 20, 30, 40, 50};
    const int32 sparse_removed[] = {1, 3};
    TSparseArray<int32> sparse = MakeSparse(sparse_values, 5, sparse_removed, 2);
    int32 many_values[200];
    int32 many_removed[100];
    for (int32 i = 0; i < 200; ++i) many_values[i] = i;
    for (int32 i = 0; i < 100; ++i) many_removed[i] = i * 2;
    TSparseArray<int32> sparse_spilled = MakeSparse(many_values, 200, many_removed, 100);

    const FString crew_names[] = {MakeString(u"Ada"), MakeString(u"Bo"), MakeString(u"Cy")};
    const int32 crew_removed[] = {1};
    TSet<FString> crew = MakeSet(crew_names, 3, crew_removed, 1);

    const TTuple<FString, int32> cargo_pairs[] = {
        {MakeString(u"Hull"), 3}, {MakeString(u"Mast"), 1}, {MakeString(u"Sail"), 2}, {MakeString(u"Rope"), 12}};
    const int32 cargo_removed[] = {1};
    TMap<FString, int32> cargo = MakeMap<TMap<FString, int32>>(cargo_pairs, 4, cargo_removed, 1);
    TMap<FString, int32> no_cargo;

    const TTuple<int32, FString> mooring_pairs[] = {
        {1, MakeString(u"Dock A")}, {1, MakeString(u"Dock B")}, {2, MakeString(u"Pier")}};
    TMultiMap<int32, FString> moorings = MakeMap<TMultiMap<int32, FString>>(mooring_pairs, 3, nullptr, 0);

    FName none_name = AddName("None");
    FName hull_name = AddName("Hull");
    FName numbered_name = AddName("Crate", 0, 4);
    FName far_name = AddName("FarBlock", 1);
    (void)none_name;

    FReferenceControllerBase crate_counts;
    crate_counts.SharedReferenceCount = 2;
    crate_counts.WeakReferenceCount = 2;
    FCrate crate{42, MakeString(u"Rope")};
    TSharedPtr<FCrate> shared_crate{&crate, {&crate_counts}};
    TSharedRef<FCrate> crate_ref{&crate, {&crate_counts}};
    TSharedPtr<FCrate> null_crate;
    TWeakPtr<FCrate> weak_crate{&crate, {&crate_counts}};
    FReferenceControllerBase dead_counts;
    dead_counts.SharedReferenceCount = 0;
    TWeakPtr<FCrate> expired_crate{&crate, {&dead_counts}};
    TUniquePtr<FCrate> unique_crate;
    unique_crate.Ptr = &crate;
    TUniquePtr<FCrate> empty_unique;

    UClass actor_class;
    actor_class.NamePrivate.Name = AddName("Actor");
    AActor ship;
    ship.NamePrivate.Name = AddName("Ship", 0, 2);
    ship.ClassPrivate.ObjectPtr.DebugPtr = &actor_class;
    TObjectPtr<AActor> ship_ptr;
    ship_ptr.DebugPtr = &ship;
    TObjectPtr<AActor> null_object;
    TObjectPtr<AActor> unresolved_object;
    unresolved_object.ObjectPtr.Handle.PointerOrRef = 0x1001;

    FLocalizedTextData text_data;
    FString localized = MakeString(u"Anchors aweigh");
    FReferenceControllerBase text_counts;
    text_data.LocalizedString = TSharedPtr<FString>{&localized, {&text_counts}};
    FText text{{&text_data}};
    FText empty_text;

    TOptional<int32> some_number(17);
    TOptional<int32> no_number;
    TOptional<FVector> some_vector(FVector{1, 2, 3});

    TVariant<int32, double, FString> variant_int;
    variant_int.Set<int32>(9, 0);
    TVariant<int32, double, FString> variant_string;
    variant_string.Set<FString>(MakeString(u"Mast"), 2);

    FSoftObjectPath soft_path{{AddName("/Game/Ships/Galleon"), AddName("Galleon")}, {MakeArray("Hull.Plank", 11)}};
    FSoftObjectPath null_path{{none_name, none_name}, {}};
    TSoftObjectPtr<AActor> soft_ship{{{}, {{AddName("/Game/Ships/Sloop"), AddName("Sloop")}, {}}}};

    FGameplayTag tag{AddName("Weather.Storm")};
    FGameplayTag no_tag{none_name};
    FGuid guid{0x01234567, 0x89ABCDEF, 0x01234567, 0x89ABCDEF};

    FVector position{1.5, -2, 300.25};
    FVector2D uv{0.25, 0.75};
    FVector4 plane{0, 0, 1, 10};
    FQuat rotation{0, 0, 0, 1};
    FRotator facing{10, 90, 0};
    FTransform transform{{{0, 0, 0, 1}}, {{100, 200, 300, 0}}, {{1, 1, 2, 0}}};
    FIntPoint cell{4, -2};
    FIntVector voxel{1, 2, 3};
    FColor orange{0, 128, 255, 255};
    FLinearColor teal{0, 0.5f, 0.5f, 1};

    break_here();
    return hull_name.Number + numbered_name.Number + far_name.Number;
}
