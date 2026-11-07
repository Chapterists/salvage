"""LLDB formatters for Unreal Engine types, written for Salvage.

Load with:  command script import /path/to/salvage_unreal.py
(`salvage lldb install` adds that line to ~/.lldbinit for you.)

Covers strings and containers (FString, FName, FText, TArray, TArrayView,
TStringView, TSparseArray, TSet, TMap, TMultiMap), pointers (TSharedPtr, TSharedRef, TWeakPtr, TUniquePtr,
TObjectPtr, TSoftObjectPtr, FSoftObjectPath), TOptional, TVariant, FGuid,
FGameplayTag, and math types (FVector, FVector2D, FVector4, FRotator, FQuat,
FTransform, FIntPoint, FIntVector, FColor, FLinearColor).

Written from scratch from Unreal's documented behavior and member names. Every
formatter navigates by member name and falls back quietly, so a layout it
doesn't recognize shows the raw value instead of an error.
"""

import struct

import lldb

CATEGORY = "salvage-unreal"
MAX_STRING = 2048
MAX_CHILDREN = 1000
MAX_NAME = 1024
NAME_BLOCK_OFFSET_BITS = 16
NAME_ENTRY_STRIDE = 2

# --- helpers -------------------------------------------------------------------


def _raw(valobj):
    """The value without any synthetic children we added."""
    return valobj.GetNonSyntheticValue() if valobj.IsSynthetic() else valobj


def _child(valobj, *names):
    """The first child with one of `names` (searching base classes and anonymous unions)."""
    for name in names:
        child = valobj.GetChildMemberWithName(name)
        if child.IsValid():
            return _raw(child)
    return None


def _unsigned(valobj, size=None):
    """Reads an integer, including std::atomic wrappers, by its bytes."""
    if valobj is None or not valobj.IsValid():
        return None
    size = size or valobj.GetByteSize()
    error = lldb.SBError()
    data = valobj.GetData()
    if data.GetByteSize() >= size and size in (1, 2, 4, 8):
        if size == 1:
            return data.GetUnsignedInt8(error, 0)
        if size == 2:
            return data.GetUnsignedInt16(error, 0)
        if size == 4:
            return data.GetUnsignedInt32(error, 0)
        return data.GetUnsignedInt64(error, 0)
    return valobj.GetValueAsUnsigned(0)


def _signed32(valobj):
    value = _unsigned(valobj, 4)
    if value is None:
        return None
    return value - (1 << 32) if value & 0x80000000 else value


def _read(process, address, size):
    if not address or size <= 0:
        return None
    error = lldb.SBError()
    data = process.ReadMemory(address, size, error)
    return data if error.Success() else None


def _pointer_size(valobj):
    return valobj.GetTarget().GetAddressByteSize() or 8


def _read_pointer(process, address, pointer_size):
    data = _read(process, address, pointer_size)
    if data is None:
        return None
    return struct.unpack("<Q" if pointer_size == 8 else "<I", data)[0]


def _decode(data, char_size):
    if char_size == 2:
        return data.decode("utf-16-le", "replace")
    if char_size == 4:
        return data.decode("utf-32-le", "replace")
    return data.decode("utf-8", "replace")


def _quote(text, truncated=False):
    text = text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\r", "\\r")
    return '"%s"%s' % (text, "..." if truncated else "")


def _template_arg(valobj, index):
    for t in (valobj.GetType(), valobj.GetType().GetCanonicalType()):
        if t.GetNumberOfTemplateArguments() > index:
            arg = t.GetTemplateArgumentType(index)
            if arg.IsValid():
                return arg
    return None


def _number(value):
    text = "%.4f" % value
    text = text.rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


def _safe(function):
    """Wraps a summary so a surprise in the debuggee never raises into LLDB."""

    def summary(valobj, internal_dict):
        try:
            result = function(_raw(valobj))
            return result if result is not None else ""
        except Exception:  # noqa: BLE001 - a formatter must never take LLDB down
            return ""

    summary.__name__ = function.__name__
    return summary


# --- arrays and strings -----------------------------------------------------------


def _allocation(allocator):
    """Address of an allocator's elements (heap, or inline storage when not spilled)."""
    secondary = _child(allocator, "SecondaryData")
    if secondary is not None:
        heap = _child(secondary, "Data")
        address = heap.GetValueAsUnsigned(0) if heap is not None else 0
        if address:
            return address
        inline = _child(allocator, "InlineData")
        return inline.GetLoadAddress() if inline is not None else 0
    data = _child(allocator, "Data")
    if data is None:
        return 0
    if data.GetType().IsPointerType():
        return data.GetValueAsUnsigned(0)
    return data.GetLoadAddress()


def _array(valobj):
    """(address, count, element type) for a TArray."""
    num = _child(valobj, "ArrayNum")
    allocator = _child(valobj, "AllocatorInstance")
    if num is None or allocator is None:
        return None
    count = _signed32(num) if num.GetByteSize() == 4 else num.GetValueAsSigned(0)
    return _allocation(allocator), max(count or 0, 0), _template_arg(valobj, 0)


def _string_from(process, address, count, char_size):
    """Reads `count` characters, dropping a trailing NUL."""
    if not address or count <= 0:
        return "", False
    truncated = count > MAX_STRING
    count = min(count, MAX_STRING)
    data = _read(process, address, count * char_size)
    if data is None:
        return None, False
    text = _decode(data, char_size)
    return text.rstrip("\x00"), truncated


def _fstring(valobj):
    """The text of an FString (or FUtf8String/FAnsiString), or None."""
    data = _child(valobj, "Data")
    if data is None:
        return None
    info = _array(data)
    if info is None:
        return None
    address, count, element = info
    char_size = element.GetByteSize() if element is not None else 2
    text, truncated = _string_from(valobj.GetProcess(), address, count, char_size or 2)
    return None if text is None else (text, truncated)


def fstring_summary(valobj):
    result = _fstring(valobj)
    return None if result is None else _quote(*result)


def string_view_summary(valobj):
    data = _child(valobj, "DataPtr")
    size = _child(valobj, "Size")
    if data is None or size is None:
        return None
    pointee = data.GetType().GetPointeeType()
    char_size = pointee.GetByteSize() or 1
    count = size.GetValueAsSigned(0)
    text, truncated = _string_from(valobj.GetProcess(), data.GetValueAsUnsigned(0), count, char_size)
    return None if text is None else _quote(text, truncated)


class _ElementsProvider:
    """Synthetic children [0], [1]... for contiguous containers."""

    def __init__(self, valobj, internal_dict):
        self.valobj = valobj
        self.address = 0
        self.count = 0
        self.element = None

    def locate(self):  # (address, count, element type)
        raise NotImplementedError

    def update(self):
        try:
            self.address, self.count, self.element = self.locate()
        except Exception:  # noqa: BLE001
            self.address, self.count, self.element = 0, 0, None
        if not self.address or self.element is None or not self.element.GetByteSize():
            self.count = 0
        return False

    def num_children(self):
        return min(self.count, MAX_CHILDREN)

    def get_child_index(self, name):
        try:
            return int(name.lstrip("[").rstrip("]"))
        except ValueError:
            return -1

    def get_child_at_index(self, index):
        if index < 0 or index >= self.num_children():
            return None
        offset = index * self.element.GetByteSize()
        return self.valobj.CreateValueFromAddress("[%d]" % index, self.address + offset, self.element)

    def has_children(self):
        return True


class TArrayProvider(_ElementsProvider):
    def locate(self):
        return _array(_raw(self.valobj)) or (0, 0, None)


class TArrayViewProvider(_ElementsProvider):
    def locate(self):
        valobj = _raw(self.valobj)
        data = _child(valobj, "DataPtr")
        num = _child(valobj, "ArrayNum")
        if data is None or num is None:
            return 0, 0, None
        return data.GetValueAsUnsigned(0), max(num.GetValueAsSigned(0), 0), data.GetType().GetPointeeType()


def array_summary(valobj):
    info = _array(valobj)
    return None if info is None else "Num=%d" % info[1]


def array_view_summary(valobj):
    num = _child(valobj, "ArrayNum")
    return None if num is None else "Num=%d" % num.GetValueAsSigned(0)


# --- sparse arrays, sets and maps ------------------------------------------------------
#
# A TSparseArray keeps its elements in a TArray whose slots are either an element or,
# once removed, a link in a free list. A TBitArray (AllocationFlags) says which slots
# hold elements, one bit per slot, in 32-bit words stored inline or on the heap. TSet
# keeps its elements in a TSparseArray of set elements (the value plus hash links), and
# TMap and TMultiMap keep their key/value pairs in a TSet (Pairs). Children are shown by
# slot index, so removed slots show up as gaps in the numbering.


def _allocated_slots(flags, count):
    """Indices of the set bits among the first `count` bits of a TBitArray."""
    allocator = _child(flags, "AllocatorInstance")
    num_bits = _child(flags, "NumBits")
    if allocator is None or num_bits is None:
        return None
    count = min(count, max(_signed32(num_bits) or 0, 0))
    if count <= 0:
        return []
    words = (count + 31) // 32
    data = _read(flags.GetProcess(), _allocation(allocator), words * 4)
    if data is None:
        return None
    values = struct.unpack("<%dI" % words, data)
    slots = [i for i in range(count) if values[i // 32] >> (i % 32) & 1]
    return slots[:MAX_CHILDREN]


def _sparse(valobj):
    """(slots address, slot count, slot size, element count, allocation flags) for a TSparseArray."""
    data = _child(valobj, "Data")
    flags = _child(valobj, "AllocationFlags")
    free = _child(valobj, "NumFreeIndices")
    if data is None or flags is None or free is None:
        return None
    info = _array(data)
    if info is None or info[2] is None:
        return None
    address, count, slot = info
    return address, count, slot.GetByteSize(), max(count - max(_signed32(free) or 0, 0), 0), flags


def _set_elements(valobj):
    """The TSparseArray inside a TSet, or inside the TSet inside a TMap."""
    pairs = _child(valobj, "Pairs")
    return _child(pairs if pairs is not None else valobj, "Elements")


def sparse_array_summary(valobj):
    info = _sparse(valobj)
    return None if info is None else "Num=%d" % info[3]


def set_summary(valobj):
    elements = _set_elements(valobj)
    return None if elements is None else sparse_array_summary(elements)


def pair_summary(valobj):
    key = _child(valobj, "Key")
    value = _child(valobj, "Value")
    if key is None or value is None or valobj.GetNumChildren() != 2:
        return None
    shown = lambda v: v.GetSummary() or v.GetValue() or "{...}"  # noqa: E731
    return "(%s, %s)" % (shown(key), shown(value))


class TSparseArrayProvider:
    """Children [slot] for the allocated slots of a TSparseArray, skipping removed ones."""

    def __init__(self, valobj, internal_dict):
        self.valobj = valobj
        self.children = []  # (slot, address, type)

    def sparse_array(self):
        return _raw(self.valobj)

    def element(self, sparse_array, address):
        """(address, type) of what to show for the element stored at `address`."""
        element = _template_arg(sparse_array, 0)
        return (address, element) if element is not None else None

    def update(self):
        self.children = []
        try:
            sparse_array = self.sparse_array()
            info = _sparse(sparse_array) if sparse_array is not None else None
            if info is None:
                return False
            address, count, stride, _, flags = info
            if not address or not stride:
                return False
            for slot in _allocated_slots(flags, count) or []:
                shown = self.element(sparse_array, address + slot * stride)
                if shown is None:
                    break
                self.children.append((slot, shown[0], shown[1]))
        except Exception:  # noqa: BLE001
            self.children = []
        return False

    def num_children(self):
        return len(self.children)

    def get_child_index(self, name):
        try:
            slot = int(name.lstrip("[").rstrip("]"))
        except ValueError:
            return -1
        return next((i for i, child in enumerate(self.children) if child[0] == slot), -1)

    def get_child_at_index(self, index):
        if index < 0 or index >= len(self.children):
            return None
        slot, address, element = self.children[index]
        return self.valobj.CreateValueFromAddress("[%d]" % slot, address, element)

    def has_children(self):
        return True


class TSetProvider(TSparseArrayProvider):
    """Children [slot] for a TSet's elements, or a TMap's key/value pairs."""

    def sparse_array(self):
        return _set_elements(_raw(self.valobj))

    def element(self, sparse_array, address):
        # A set element holds the value plus its hash links; show just the value.
        stored = _template_arg(sparse_array, 0)
        if stored is None:
            return None
        value = _child(self.valobj.CreateValueFromAddress("element", address, stored), "Value")
        return None if value is None else (value.GetLoadAddress(), value.GetType())


# --- FName -------------------------------------------------------------------------

_name_blocks = {}


def _name_blocks_address(valobj):
    """Address of the name pool's block table, cached per process."""
    process = valobj.GetProcess()
    key = process.GetUniqueID()
    if key in _name_blocks:
        return _name_blocks[key]
    target = valobj.GetTarget()
    address = 0
    # Older engines keep a global pointer to the block table.
    legacy = target.FindFirstGlobalVariable("GNameBlocksDebug")
    if legacy.IsValid():
        address = legacy.GetValueAsUnsigned(0)
    if not address:
        options = lldb.SBExpressionOptions()
        options.SetIgnoreBreakpoints(True)
        options.SetTryAllThreads(False)
        options.SetTimeoutInMicroSeconds(500000)
        frame = process.GetSelectedThread().GetSelectedFrame()
        context = frame if frame.IsValid() else target
        result = context.EvaluateExpression("(unsigned char**)FNameDebugVisualizer::GetBlocks()", options)
        if result.IsValid() and result.GetError().Success():
            address = result.GetValueAsUnsigned(0)
    _name_blocks[key] = address
    return address


def _entry_id(valobj):
    if valobj is None:
        return None
    value = _child(valobj, "Value")
    return _unsigned(value if value is not None else valobj, 4)


def fname_text(valobj):
    """The text of an FName, with its number suffix, or None if it can't be read."""
    display = _child(valobj, "DisplayIndex")
    case_preserving = display is not None
    index = _entry_id(display if case_preserving else _child(valobj, "ComparisonIndex"))
    if index is None:
        return None
    number_child = _child(valobj, "Number")
    number = _unsigned(number_child, 4) if number_child is not None else 0
    text = _name_entry(valobj, index, case_preserving)
    if text is None:
        return None
    return "%s_%d" % (text, number - 1) if number else text


def _name_entry(valobj, index, case_preserving):
    blocks = _name_blocks_address(valobj)
    if not blocks:
        return None
    process = valobj.GetProcess()
    pointer_size = _pointer_size(valobj)
    block = _read_pointer(process, blocks + (index >> NAME_BLOCK_OFFSET_BITS) * pointer_size, pointer_size)
    if not block:
        return None
    entry = block + (index & ((1 << NAME_BLOCK_OFFSET_BITS) - 1)) * NAME_ENTRY_STRIDE
    header = _read(process, entry, 2)
    if header is None:
        return None
    header = struct.unpack("<H", header)[0]
    wide = header & 1
    length = header >> 1 if case_preserving else header >> 6
    if length > MAX_NAME:
        return None
    data = _read(process, entry + 2, length * (2 if wide else 1))
    if data is None:
        return None
    return data.decode("utf-16-le", "replace") if wide else data.decode("latin-1")


def fname_summary(valobj):
    text = fname_text(valobj)
    if text is not None:
        return text
    index = _entry_id(_child(valobj, "ComparisonIndex"))
    return None if index is None else "<name %d>" % index


# --- FText -------------------------------------------------------------------------

_TEXT_MEMBERS = ("LocalizedString", "DisplayString", "SourceString")


def _find_string(valobj, depth):
    """Looks through an object's members for its display string."""
    if depth < 0 or valobj is None or not valobj.IsValid():
        return None
    name = valobj.GetType().GetUnqualifiedType().GetName()
    if name in ("FString", "FUtf8String", "FAnsiString"):
        return _fstring(valobj)
    if name.startswith(("TSharedPtr<", "TSharedRef<")):
        target = _child(valobj, "Object")
        if target is not None and target.GetValueAsUnsigned(0):
            return _find_string(target.Dereference(), depth - 1)
        return None
    for member in _TEXT_MEMBERS:
        child = _child(valobj, member)
        if child is not None:
            found = _find_string(child, depth - 1)
            if found is not None:
                return found
    for i in range(min(valobj.GetNumChildren(), 32)):
        child = valobj.GetChildAtIndex(i)
        if child.GetType().IsPointerType():
            continue
        found = _find_string(child, depth - 1)
        if found is not None:
            return found
    return None


def ftext_summary(valobj):
    data = _child(valobj, "TextData")
    if data is None:
        return None
    pointer = _child(data, "Reference", "Object", "Ptr")
    if pointer is None or not pointer.GetValueAsUnsigned(0):
        return '""'
    target = pointer.GetDynamicValue(lldb.eDynamicDontRunTarget).Dereference()
    found = _find_string(target, 4)
    return _quote(*found) if found is not None else "<text>"


# --- smart pointers ---------------------------------------------------------------


def _describe_pointee(pointer):
    """Summary of what a pointer points at, or its address."""
    address = pointer.GetValueAsUnsigned(0)
    if not address:
        return "nullptr"
    target = pointer.Dereference()
    summary = target.GetSummary() if target.IsValid() else None
    return summary if summary else "0x%x" % address


def _reference_counts(controller):
    """(shared, weak) counts from a shared pointer's reference controller."""
    if controller is None or not controller.GetValueAsUnsigned(0):
        return None
    target = controller.Dereference()
    return _signed32(_child(target, "SharedReferenceCount")), _signed32(_child(target, "WeakReferenceCount"))


def _controller(valobj, holder):
    counter = _child(valobj, holder)
    return _child(counter, "ReferenceController") if counter is not None else None


def shared_ptr_summary(valobj):
    pointer = _child(valobj, "Object")
    if pointer is None:
        return None
    if not pointer.GetValueAsUnsigned(0):
        return "nullptr"
    counts = _reference_counts(_controller(valobj, "SharedReferenceCount"))
    refs = "refs=%d " % counts[0] if counts and counts[0] is not None else ""
    return "%s%s" % (refs, _describe_pointee(pointer))


def weak_ptr_summary(valobj):
    pointer = _child(valobj, "Object")
    if pointer is None:
        return None
    counts = _reference_counts(_controller(valobj, "WeakReferenceCount"))
    if not pointer.GetValueAsUnsigned(0) or not counts or not counts[0]:
        return "expired" if pointer.GetValueAsUnsigned(0) else "nullptr"
    return "refs=%d %s" % (counts[0], _describe_pointee(pointer))


def unique_ptr_summary(valobj):
    pointer = _child(valobj, "Ptr")
    return None if pointer is None else _describe_pointee(pointer)


class PointerProvider:
    """One child, `Object`: the pointer a smart pointer holds."""

    MEMBERS = ("Object", "Ptr", "DebugPtr")

    def __init__(self, valobj, internal_dict):
        self.valobj = valobj
        self.pointer = None

    def update(self):
        self.pointer = _child(_raw(self.valobj), *self.MEMBERS)
        return False

    def num_children(self):
        return 1 if self.pointer is not None and self.pointer.GetValueAsUnsigned(0) else 0

    def get_child_index(self, name):
        return 0 if name == "Object" else -1

    def get_child_at_index(self, index):
        if index != 0 or self.pointer is None:
            return None
        return self.valobj.CreateValueFromAddress("Object", self.pointer.GetLoadAddress(), self.pointer.GetType())

    def has_children(self):
        return True


# --- UObjects --------------------------------------------------------------------


def _raw_object_address(valobj, depth=4):
    """Digs the raw address out of TObjectPtr / FObjectPtr / object handles."""
    if valobj is None or depth < 0:
        return None
    if valobj.GetType().IsPointerType():
        return valobj.GetValueAsUnsigned(0)
    for name in ("DebugPtr", "Handle", "ObjectPtr", "PointerOrRef", "Ptr"):
        child = _child(valobj, name)
        if child is not None:
            found = _raw_object_address(child, depth - 1)
            if found is not None:
                return found
    if valobj.GetByteSize() == _pointer_size(valobj):
        return _unsigned(valobj)
    return None


def _object_name(target, address, object_type):
    """'Name (Class)' for a UObject at `address`."""
    if object_type is None or not object_type.IsValid() or not object_type.IsTypeComplete():
        object_type = target.FindFirstType("UObjectBase")
    if not object_type.IsValid():
        return None
    obj = target.CreateValueFromAddress("object", lldb.SBAddress(address, target), object_type)
    name_holder = _child(obj, "NamePrivate")
    if name_holder is None:
        return None
    name = fname_text(_child(name_holder, "Name") or name_holder)
    class_name = None
    class_holder = _child(obj, "ClassPrivate")
    class_address = _raw_object_address(class_holder) if class_holder is not None else None
    if class_address:
        class_obj = target.CreateValueFromAddress("class", lldb.SBAddress(class_address, target), target.FindFirstType("UObjectBase"))
        class_name_holder = _child(class_obj, "NamePrivate")
        if class_name_holder is not None:
            class_name = fname_text(_child(class_name_holder, "Name") or class_name_holder)
    if name is None:
        return None
    return "%s (%s)" % (name, class_name) if class_name else name


def object_ptr_summary(valobj):
    address = _raw_object_address(valobj)
    if address is None:
        return None
    if not address:
        return "nullptr"
    if address & 1:
        return "unresolved object reference"
    pointee = _template_arg(valobj, 0)
    name = _object_name(valobj.GetTarget(), address, pointee)
    return name if name else "0x%x" % address


class ObjectPtrProvider(PointerProvider):
    MEMBERS = ("DebugPtr",)


# --- soft references and tags --------------------------------------------------


def _fname_member(valobj, name):
    child = _child(valobj, name)
    return fname_text(child) if child is not None else None


def soft_object_path_text(valobj):
    asset_path = _child(valobj, "AssetPath")
    if asset_path is not None:
        package = _fname_member(asset_path, "PackageName")
        asset = _fname_member(asset_path, "AssetName")
        path = ""
        if package and package != "None":
            path = package + ("." + asset if asset and asset != "None" else "")
    else:  # older engines store the whole path in one FName
        path = _fname_member(valobj, "AssetPathName") or ""
        if path == "None":
            path = ""
    sub_path = _child(valobj, "SubPathString")
    sub = _fstring(sub_path) if sub_path is not None else None
    if sub and sub[0]:
        path = "%s:%s" % (path, sub[0])
    return path


def soft_object_path_summary(valobj):
    path = soft_object_path_text(valobj)
    return _quote(path) if path else "null"


def soft_object_ptr_summary(valobj):
    inner = _child(valobj, "SoftObjectPtr")
    if inner is not None:
        valobj = inner
    path = _child(valobj, "ObjectID")
    if path is None:
        return None
    return soft_object_path_summary(path)


def gameplay_tag_summary(valobj):
    name = _fname_member(valobj, "TagName")
    if name is None:
        return None
    return "(none)" if name == "None" else name


def guid_summary(valobj):
    parts = [_unsigned(_child(valobj, n), 4) for n in "ABCD"]
    if any(p is None for p in parts):
        return None
    a, b, c, d = parts
    return "%08X-%04X-%04X-%04X-%04X%08X" % (a, b >> 16, b & 0xFFFF, c >> 16, c & 0xFFFF, d)


# --- TOptional and TVariant ---------------------------------------------------


def optional_summary(valobj):
    value = _child(valobj, "TypedValue", "Value")
    is_set = _child(valobj, "bIsSet")
    if is_set is not None and is_set.GetByteSize() == 1 and not _unsigned(is_set, 1):
        return "unset"
    if value is None:
        return None
    summary = value.GetSummary() or value.GetValue()
    return summary if summary else "set"


class OptionalProvider:
    def __init__(self, valobj, internal_dict):
        self.valobj = valobj
        self.value = None

    def update(self):
        raw = _raw(self.valobj)
        is_set = _child(raw, "bIsSet")
        unset = is_set is not None and is_set.GetByteSize() == 1 and not _unsigned(is_set, 1)
        self.value = None if unset else _child(raw, "TypedValue", "Value")
        return False

    def num_children(self):
        return 1 if self.value is not None else 0

    def get_child_index(self, name):
        return 0 if name == "Value" else -1

    def get_child_at_index(self, index):
        if index != 0 or self.value is None:
            return None
        return self.valobj.CreateValueFromAddress("Value", self.value.GetLoadAddress(), self.value.GetType())

    def has_children(self):
        return True


def _variant_value(valobj):
    index = _child(valobj, "TypeIndex")
    if index is None:
        return None, None
    i = _unsigned(index, 1)
    held = _template_arg(valobj, i)
    if held is None:
        return i, None
    return i, valobj.CreateValueFromAddress("Value", valobj.GetLoadAddress(), held)


def variant_summary(valobj):
    index, value = _variant_value(valobj)
    if index is None:
        return None
    if value is None:
        return "<type %d>" % index
    shown = value.GetSummary() or value.GetValue() or "{...}"
    return "%s %s" % (value.GetType().GetName(), shown)


class VariantProvider:
    def __init__(self, valobj, internal_dict):
        self.valobj = valobj
        self.value = None

    def update(self):
        _, self.value = _variant_value(_raw(self.valobj))
        return False

    def num_children(self):
        return 1 if self.value is not None else 0

    def get_child_index(self, name):
        return 0 if name == "Value" else -1

    def get_child_at_index(self, index):
        return self.value if index == 0 else None

    def has_children(self):
        return True


# --- math --------------------------------------------------------------------------


def _components(valobj, names):
    values = []
    for name in names:
        child = _child(valobj, name)
        if child is None:
            return None
        text = child.GetValue()
        if text is None:
            return None
        try:
            values.append(float(text))
        except ValueError:
            values.append(float(_unsigned(child)))
    return values


def _labelled(valobj, names):
    values = _components(valobj, names)
    if values is None:
        return None
    return "(%s)" % ", ".join("%s=%s" % (n, _number(v)) for n, v in zip(names, values))


def _vectors(names):
    return lambda valobj: _labelled(valobj, names)


vector2_summary = _vectors(("X", "Y"))
vector_summary = _vectors(("X", "Y", "Z"))
vector4_summary = _vectors(("X", "Y", "Z", "W"))
quat_summary = _vectors(("X", "Y", "Z", "W"))
rotator_summary = _vectors(("Pitch", "Yaw", "Roll"))
color_summary = _vectors(("R", "G", "B", "A"))


def int_vector_summary(valobj):
    names = [n for n in ("X", "Y", "Z", "W") if _child(valobj, n) is not None]
    return _labelled(valobj, names) if names else None


def _floats_at(valobj, member, count):
    """`count` components stored at a member, whatever its declared type (it may be a SIMD register)."""
    child = _child(valobj, member)
    if child is None:
        return None
    scalar = _template_arg(valobj, 0)
    size = scalar.GetByteSize() if scalar is not None else 8
    fmt = "<%d%s" % (count, "d" if size == 8 else "f")
    data = _read(valobj.GetProcess(), child.GetLoadAddress(), count * (8 if size == 8 else 4))
    return None if data is None else struct.unpack(fmt, data)


def transform_summary(valobj):
    translation = _floats_at(valobj, "Translation", 3)
    rotation = _floats_at(valobj, "Rotation", 4)
    scale = _floats_at(valobj, "Scale3D", 3)
    if None in (translation, rotation, scale):
        return None
    show = lambda values: "(%s)" % ", ".join(_number(v) for v in values)  # noqa: E731
    return "T=%s R=%s S=%s" % (show(translation), show(rotation), show(scale))


# --- registration ------------------------------------------------------------------

_MATH = r"(UE::Math::)?"

FORMATTERS = [
    # (regex, summary, synthetic provider)
    (r"^F(Utf8|Ansi)?String$", fstring_summary, None),
    (r"^(TStringView<.+>|F(Ansi|Utf8|Wide)?StringView)$", string_view_summary, None),
    (r"^FName$", fname_summary, None),
    (r"^FText$", ftext_summary, None),
    (r"^TArray<.+>$", array_summary, "TArrayProvider"),
    (r"^TArrayView<.+>$", array_view_summary, "TArrayViewProvider"),
    (r"^TSparseArray<.+>$", sparse_array_summary, "TSparseArrayProvider"),
    (r"^(TSet|TSparseSet)<.+>$", set_summary, "TSetProvider"),
    (r"^(TMap|TMultiMap|TSparseMap|TSparseMultiMap)<.+>$", set_summary, "TSetProvider"),
    (r"^(TTuple|TPair)<.+>$", pair_summary, None),
    (r"^TShared(Ptr|Ref)<.+>$", shared_ptr_summary, "PointerProvider"),
    (r"^TWeakPtr<.+>$", weak_ptr_summary, "PointerProvider"),
    (r"^TUniquePtr<.+>$", unique_ptr_summary, "PointerProvider"),
    (r"^(TObjectPtr<.+>|ObjectPtr_Private::TNonAccessTrackedObjectPtr<.+>)$", object_ptr_summary, "ObjectPtrProvider"),
    (r"^FObjectPtr$", object_ptr_summary, None),
    (r"^TOptional<.+>$", optional_summary, "OptionalProvider"),
    (r"^TVariant<.+>$", variant_summary, "VariantProvider"),
    (r"^FSoftObjectPath$", soft_object_path_summary, None),
    (r"^(FSoftObjectPtr|TSoft(Object|Class)Ptr<.+>)$", soft_object_ptr_summary, None),
    (r"^FGameplayTag$", gameplay_tag_summary, None),
    (r"^FGuid$", guid_summary, None),
    (r"^(" + _MATH + r"TVector<.+>|FVector3[fd]|FVector)$", vector_summary, None),
    (r"^(" + _MATH + r"TVector2<.+>|FVector2[fD]|FVector2D)$", vector2_summary, None),
    (r"^(" + _MATH + r"TVector4<.+>|FVector4[fd]?)$", vector4_summary, None),
    (r"^(" + _MATH + r"TQuat<.+>|FQuat4[fd]|FQuat)$", quat_summary, None),
    (r"^(" + _MATH + r"TRotator<.+>|FRotator3[fd]|FRotator)$", rotator_summary, None),
    (r"^(" + _MATH + r"TTransform<.+>|FTransform3[fd]|FTransform)$", transform_summary, None),
    (r"^(" + _MATH + r"TInt(Point|Vector[234]?)<.+>|FInt(Point|Vector[234]?))$", int_vector_summary, None),
    (r"^F(Linear)?Color$", color_summary, None),
]


def __lldb_init_module(debugger, internal_dict):
    module = __name__
    for index, (regex, summary, provider) in enumerate(FORMATTERS):
        name = "_summary_%d" % index
        globals()[name] = _safe(summary)
        # --expand keeps children visible under a summary, for types that have them.
        expand = " --expand" if provider else ""
        debugger.HandleCommand(
            'type summary add -w %s%s -F %s.%s -x "%s"' % (CATEGORY, expand, module, name, regex)
        )
        if provider:
            debugger.HandleCommand(
                'type synthetic add -w %s -l %s.%s -x "%s"' % (CATEGORY, module, provider, regex)
            )
    debugger.HandleCommand("type category enable %s" % CATEGORY)
