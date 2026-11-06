//! Finds Windows API use in C and C++ source that would stop a Mac build:
//! Windows headers, Win32 calls and Win32 types outside any Windows-only
//! `#if` block. Also notes `#if PLATFORM_WINDOWS` blocks with no `#else`,
//! which compile on Mac but leave whatever they do out.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Api { line: usize, what: String },
    OnlyBlock { line: usize },
}

/// Headers that only exist on Windows (compared in lower case).
const HEADERS: [&str; 24] = [
    "windows.h",
    "winsock2.h",
    "ws2tcpip.h",
    "winsock.h",
    "shlobj.h",
    "shellapi.h",
    "shlwapi.h",
    "winreg.h",
    "winbase.h",
    "wtypes.h",
    "objbase.h",
    "comdef.h",
    "atlbase.h",
    "wrl.h",
    "wrl/client.h",
    "d3d11.h",
    "d3d12.h",
    "dxgi.h",
    "dxgi1_4.h",
    "xinput.h",
    "mmsystem.h",
    "psapi.h",
    "tlhelp32.h",
    "dbghelp.h",
];

/// Unreal's wrappers around the Windows headers.
const UNREAL_WINDOWS_HEADERS: [&str; 5] = [
    "windows/allowwindowsplatformtypes.h",
    "windows/windowshwrapper.h",
    "windows/minwindows.h",
    "microsoft/allowmicrosoftplatformtypes.h",
    "windows/allowwindowsplatformatomics.h",
];

/// Win32 functions, matched as a whole word followed by `(`.
const CALLS: [&str; 44] = [
    "CreateFileA",
    "CreateFileW",
    "CreateProcessA",
    "CreateProcessW",
    "GetModuleHandleA",
    "GetModuleHandleW",
    "GetModuleFileNameA",
    "GetModuleFileNameW",
    "GetProcAddress",
    "LoadLibraryA",
    "LoadLibraryW",
    "LoadLibraryExW",
    "FreeLibrary",
    "RegOpenKeyExA",
    "RegOpenKeyExW",
    "RegQueryValueExA",
    "RegQueryValueExW",
    "RegSetValueExW",
    "RegCloseKey",
    "MessageBoxA",
    "MessageBoxW",
    "WaitForSingleObject",
    "WaitForMultipleObjects",
    "CreateEventA",
    "CreateEventW",
    "CloseHandle",
    "VirtualAlloc",
    "VirtualFree",
    "SHGetFolderPathW",
    "SHGetKnownFolderPath",
    "CoInitialize",
    "CoInitializeEx",
    "CoCreateInstance",
    "ShellExecuteA",
    "ShellExecuteW",
    "OutputDebugStringA",
    "OutputDebugStringW",
    "MultiByteToWideChar",
    "WideCharToMultiByte",
    "CreateThread",
    "DeviceIoControl",
    "GetTickCount64",
    "QueryPerformanceCounter",
    "_wfopen",
];

/// Win32 types, matched as whole words.
const TYPES: [&str; 10] = [
    "HWND",
    "HMODULE",
    "HINSTANCE",
    "HRESULT",
    "LPCWSTR",
    "LPWSTR",
    "LPCSTR",
    "WPARAM",
    "LPARAM",
    "HKEY",
];

/// Macros that mean "Windows" in an `#if`.
const WINDOWS_MACROS: [&str; 5] = [
    "PLATFORM_WINDOWS",
    "_WIN32",
    "_WIN64",
    "PLATFORM_MICROSOFT",
    "_MSC_VER",
];

/// Code with comments and string contents blanked out (newlines kept, so
/// line numbers still match).
pub fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i += 2;
            while i < b.len() && !b[i..].starts_with(b"*/") {
                if b[i] == b'\n' {
                    out.push(b'\n');
                }
                i += 1;
            }
            i += 2;
        } else if b[i] == b'"' || b[i] == b'\'' {
            let quote = b[i];
            out.push(quote);
            i += 1;
            while i < b.len() && b[i] != quote && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            out.push(quote);
            i += 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What an `#if` condition says about Windows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Condition {
    /// True only on Windows: `#if PLATFORM_WINDOWS`, `#ifdef _WIN32`.
    Windows,
    /// True everywhere but Windows: `#if !PLATFORM_WINDOWS`, `#ifndef _WIN32`.
    NotWindows,
    Other,
}

fn classify(directive: &str, condition: &str) -> Condition {
    let condition: String = condition.chars().filter(|c| !c.is_whitespace()).collect();
    // Unreal's and the compilers' own macros, plus a library's own Windows
    // switches (PSD_USE_MSVC, FOO_WIN32 and the like).
    let mentions = WINDOWS_MACROS.iter().any(|m| word_in(&condition, m))
        || identifiers(&condition).any(|id| {
            ["WINDOWS", "WIN32", "WIN64", "MSVC", "MICROSOFT"]
                .iter()
                .any(|w| id.contains(w))
        });
    if !mentions || condition.contains("||") {
        return Condition::Other;
    }
    let negated = condition.starts_with('!') || condition.contains("==0");
    let windows = match directive {
        "ifdef" => true,
        "ifndef" => false,
        _ => !negated,
    };
    // `PLATFORM_WINDOWS && SOMETHING` is still Windows-only; anything negated
    // alongside other terms is too unclear to call.
    if negated && condition.contains("&&") {
        return Condition::Other;
    }
    if windows {
        Condition::Windows
    } else {
        Condition::NotWindows
    }
}

fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
}

fn word_in(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let at = start + pos;
        let end = at + word.len();
        let before = at == 0 || !is_ident(bytes[at - 1]);
        let after = end >= bytes.len() || !is_ident(bytes[end]);
        if before && after {
            return true;
        }
        start = at + 1;
    }
    false
}

/// One open `#if`.
struct Frame {
    line: usize,
    first: Condition,
    /// The branch we're in is Windows-only.
    windows_now: bool,
    /// An `#elif` or `#else` came.
    alternative: bool,
}

pub fn scan(text: &str) -> Vec<Hit> {
    let code = strip_comments(text);
    let mut hits = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut original = text.lines();
    for (index, raw) in code.lines().enumerate() {
        let line = index + 1;
        // Comment stripping blanks quoted text, so quoted includes are read
        // from the original line.
        let original = original.next().unwrap_or("");
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim_start();
            let directive: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let condition = rest[directive.len()..].trim();
            match directive.as_str() {
                "if" | "ifdef" | "ifndef" => {
                    let first = classify(&directive, condition);
                    stack.push(Frame {
                        line,
                        first,
                        windows_now: first == Condition::Windows,
                        alternative: false,
                    });
                }
                "elif" => {
                    if let Some(frame) = stack.last_mut() {
                        frame.alternative = true;
                        frame.windows_now = classify("if", condition) == Condition::Windows;
                    }
                }
                "else" => {
                    if let Some(frame) = stack.last_mut() {
                        frame.alternative = true;
                        frame.windows_now = frame.first == Condition::NotWindows;
                    }
                }
                "endif" => {
                    if let Some(frame) = stack.pop()
                        && frame.first == Condition::Windows
                        && !frame.alternative
                        && !stack.iter().any(|f| f.windows_now)
                    {
                        hits.push(Hit::OnlyBlock { line: frame.line });
                    }
                }
                "include" | "import" => {
                    if !stack.iter().any(|f| f.windows_now)
                        && let Some(header) = original
                            .trim_start()
                            .strip_prefix('#')
                            .map(|r| r.trim_start())
                            .and_then(|r| r.strip_prefix(directive.as_str()))
                            .and_then(included)
                    {
                        let lower = header.to_ascii_lowercase().replace('\\', "/");
                        if HEADERS.contains(&lower.as_str())
                            || UNREAL_WINDOWS_HEADERS.contains(&lower.as_str())
                        {
                            hits.push(Hit::Api {
                                line,
                                what: format!("#include <{header}>"),
                            });
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if stack.iter().any(|f| f.windows_now) {
            continue;
        }
        if let Some(call) = CALLS.iter().find(|c| called(raw, c)) {
            hits.push(Hit::Api {
                line,
                what: format!("{call}()"),
            });
        } else if let Some(ty) = TYPES.iter().find(|t| word_in(raw, t)) {
            hits.push(Hit::Api {
                line,
                what: ty.to_string(),
            });
        }
    }
    hits
}

/// The header named by an `#include` (its quotes or brackets dropped).
fn included(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let (open, close) = match rest.chars().next()? {
        '<' => ('<', '>'),
        '"' => ('"', '"'),
        _ => return None,
    };
    let inner = rest.strip_prefix(open)?;
    let end = inner.find(close)?;
    let header = inner[..end].trim();
    (!header.is_empty()).then(|| header.to_string())
}

fn called(line: &str, name: &str) -> bool {
    let mut start = 0;
    let bytes = line.as_bytes();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while let Some(pos) = line[start..].find(name) {
        let at = start + pos;
        let end = at + name.len();
        let before_ok = at == 0 || !(is_ident(bytes[at - 1]) || bytes[at - 1] == b'.');
        let rest = line[end..].trim_start();
        // `Foo::CloseHandle(` is someone's own method; `::CloseHandle(` is Win32.
        let qualified = at >= 2 && &line[at - 2..at] == "::" && at >= 3 && is_ident(bytes[at - 3]);
        if before_ok
            && !qualified
            && rest.starts_with('(')
            && (end >= bytes.len() || !is_ident(bytes[end]))
        {
            return true;
        }
        start = at + 1;
    }
    false
}
