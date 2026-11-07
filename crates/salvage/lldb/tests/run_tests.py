#!/usr/bin/env python3
"""Checks salvage_unreal.py against a real LLDB session.

Builds fixture.cpp twice (game-style and editor-style FName layouts), stops
it at break_here(), prints the fixture's variables, and compares each one's
summary with what we expect. Needs clang++ and lldb (Xcode on macOS).

    python3 crates/salvage/lldb/tests/run_tests.py
"""

import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
FORMATTERS = os.path.join(os.path.dirname(HERE), "salvage_unreal.py")
FIXTURE = os.path.join(HERE, "fixture.cpp")

# variable -> exact summary LLDB should print after "name = "
EXPECTED = {
    "greeting": '"Ahoy, Unreal"',
    "empty_string": '""',
    "utf8": '"café"',
    "view": '"partial"',
    "numbers": "Num=5",
    "no_numbers": "Num=0",
    "numbers_view": "Num=3",
    "inline_numbers": "Num=2",
    "sparse": "Num=3",
    "sparse_spilled": "Num=100",
    "crew": "Num=2",
    "cargo": "Num=3",
    "no_cargo": "Num=0",
    "moorings": "Num=3",
    "hull_name": "Hull",
    "numbered_name": "Crate_3",
    "far_name": "FarBlock",
    "shared_crate": "refs=2 0x",
    "crate_ref": "refs=2 0x",
    "null_crate": "nullptr",
    "weak_crate": "refs=2 0x",
    "expired_crate": "expired",
    "unique_crate": "0x",
    "empty_unique": "nullptr",
    "ship_ptr": "Ship_1 (Actor)",
    "null_object": "nullptr",
    "unresolved_object": "unresolved object reference",
    "text": '"Anchors aweigh"',
    "empty_text": '""',
    "some_number": "17",
    "no_number": "unset",
    "some_vector": "(X=1, Y=2, Z=3)",
    "variant_int": "int 9",
    "variant_string": 'FString "Mast"',
    "soft_path": '"/Game/Ships/Galleon.Galleon:Hull.Plank"',
    "null_path": "null",
    "soft_ship": '"/Game/Ships/Sloop.Sloop"',
    "tag": "Weather.Storm",
    "no_tag": "(none)",
    "guid": "01234567-89AB-CDEF-0123-456789ABCDEF",
    "position": "(X=1.5, Y=-2, Z=300.25)",
    "uv": "(X=0.25, Y=0.75)",
    "plane": "(X=0, Y=0, Z=1, W=10)",
    "rotation": "(X=0, Y=0, Z=0, W=1)",
    "facing": "(Pitch=10, Yaw=90, Roll=0)",
    "transform": "T=(100, 200, 300) R=(0, 0, 0, 1) S=(1, 1, 2)",
    "cell": "(X=4, Y=-2)",
    "voxel": "(X=1, Y=2, Z=3)",
    "orange": "(R=255, G=128, B=0, A=255)",
    "teal": "(R=0, G=0.5, B=0.5, A=1)",
}

# Children LLDB should show when a value is expanded: (variable, child line fragment)
EXPECTED_CHILDREN = [
    ("numbers", "[4] = 5"),
    ("inline_numbers", "[1] = 8"),
    ("numbers_view", "[0] = 1"),
    ("some_number", "Value = 17"),
    ("variant_string", 'Value = "Mast"'),
    ("sparse", "[0] = 10"),
    ("sparse", "[2] = 30"),
    ("sparse", "[4] = 50"),
    ("sparse_spilled", "[1] = 1"),
    # 200 slots need 7 words of bits, so they're all on the heap. LLDB prints the
    # first 24 children of a long container, so check one near the end of those.
    ("sparse_spilled", "[47] = 47"),
    ("crew", '[0] = "Ada"'),
    ("crew", '[2] = "Cy"'),
    ("cargo", '[0] = ("Hull", 3)'),
    ("cargo", '[3] = ("Rope", 12)'),
    ("moorings", '[0] = (1, "Dock A")'),
    ("moorings", '[1] = (1, "Dock B")'),
]

# Children that must not appear: removed slots are skipped, not shown as garbage.
UNEXPECTED_CHILDREN = [
    ("sparse", "[1] ="),
    ("sparse", "[3] ="),
    ("sparse_spilled", "[0] ="),
    ("crew", "[1] ="),
    ("cargo", "[1] ="),
]


def run(command, **kwargs):
    return subprocess.run(command, capture_output=True, text=True, **kwargs)


def tool(name):
    """On macOS, run Xcode's tools through xcrun so they find the SDK."""
    return ["xcrun", name] if sys.platform == "darwin" else [name]


def build(workdir, flavor, defines):
    binary = os.path.join(workdir, "fixture-" + flavor)
    result = run([*tool("clang++"), "-std=c++20", "-g", "-O0", "-fno-limit-debug-info", *defines, FIXTURE, "-o", binary])
    if result.returncode != 0:
        sys.exit("clang++ failed:\n" + result.stderr)
    return binary


def inspect(binary):
    names = list(EXPECTED)
    commands = [
        "command script import " + FORMATTERS,
        "breakpoint set --name break_here",
        "process launch",
        "frame select 1",
        "frame variable --depth 1 " + " ".join(names),
    ]
    args = [*tool("lldb"), "--batch", "--no-lldbinit"]
    for command in commands:
        args += ["--one-line", command]
    args.append(binary)
    result = run(args, timeout=180)
    return result.stdout + result.stderr


def check(flavor, output):
    failures = []
    lines = output.splitlines()
    for name, expected in EXPECTED.items():
        pattern = re.compile(r"^\([^)]*(\([^)]*\))?[^)]*\) %s = (.*)$" % re.escape(name))
        found = [m.group(2) for m in (pattern.match(line) for line in lines) if m]
        if not found:
            failures.append("%s: %s not printed" % (flavor, name))
            continue
        actual = re.sub(r"\s*\{\}?$", "", found[-1]).strip()  # drop " {" or " {}" around children
        ok = actual.startswith(expected) if expected.endswith("0x") else actual == expected
        if not ok:
            failures.append("%s: %s = %s, expected %s" % (flavor, name, actual, expected))
    for name, fragment in EXPECTED_CHILDREN:
        start = next((i for i, l in enumerate(lines) if re.match(r"^\(.*\) %s = " % re.escape(name), l)), None)
        block = []
        if start is not None:
            for line in lines[start + 1 :]:
                if line.startswith("("):
                    break
                block.append(line)
        if not any(fragment in line for line in block):
            failures.append("%s: %s has no child %r" % (flavor, name, fragment))
    for name, fragment in UNEXPECTED_CHILDREN:
        start = next((i for i, l in enumerate(lines) if re.match(r"^\(.*\) %s = " % re.escape(name), l)), None)
        block = []
        if start is not None:
            for line in lines[start + 1 :]:
                if line.startswith("("):
                    break
                block.append(line)
        if start is None or any(line.strip().startswith(fragment) for line in block):
            failures.append("%s: %s shows removed slot %r" % (flavor, name, fragment))
    return failures


TOTAL = len(EXPECTED) + len(EXPECTED_CHILDREN) + len(UNEXPECTED_CHILDREN)


def main():
    failures = []
    with tempfile.TemporaryDirectory() as workdir:
        for flavor, defines in (("game", []), ("editor", ["-DCASE_PRESERVING_NAME"])):
            output = inspect(build(workdir, flavor, defines))
            problems = check(flavor, output)
            if problems and "-v" in sys.argv:
                print(output)  # the whole LLDB session, to see what went wrong
            failures += problems
            print("%s layout: %d of %d checks passed" % (flavor, TOTAL - len(problems), TOTAL))
    for failure in failures:
        print("  FAIL " + failure)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
