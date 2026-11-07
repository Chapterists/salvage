//! Reading crash reports: macOS `.ips` files (JSON), the older text format
//! (`.crash`, and the `minidump.dmp` Unreal writes on a Mac, which is that
//! format despite its name), and Unreal's `CrashContext.runtime-xml`.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Image {
    pub name: String,
    pub path: Option<PathBuf>,
    /// Upper case with dashes, as dwarfdump and Spotlight write it.
    pub uuid: Option<String>,
    pub arch: Option<String>,
    pub base: u64,
    /// The last address of the image, when the report gives it.
    pub end: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Frame {
    pub index: usize,
    /// Index into the report's images, when the frame's image is known.
    pub image: Option<usize>,
    pub module: String,
    pub address: u64,
    /// The symbol the report itself names, if any (often without file and line).
    pub report_symbol: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Thread {
    pub index: usize,
    pub name: Option<String>,
    pub crashed: bool,
    pub frames: Vec<Frame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// An Unreal crash folder (Saved/Crashes/<id>).
    Unreal,
    /// A macOS .ips report.
    Ips,
    /// A text crash report (.crash).
    Text,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub source: Source,
    pub path: PathBuf,
    pub process: Option<String>,
    pub time: Option<String>,
    /// e.g. "EXC_BAD_ACCESS (SIGSEGV)".
    pub exception: Option<String>,
    /// The macOS build the crash happened on, e.g. "26B123".
    pub os_build: Option<String>,
    /// Unreal's own description: crash, assert, ensure and its message.
    pub unreal: Option<UnrealContext>,
    pub images: Vec<Image>,
    pub threads: Vec<Thread>,
}

impl Report {
    pub fn crashed_thread(&self) -> Option<&Thread> {
        self.threads
            .iter()
            .find(|t| t.crashed)
            .or_else(|| self.threads.first())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct UnrealContext {
    pub crash_type: Option<String>,
    pub error_message: Option<String>,
    pub game_name: Option<String>,
    pub engine_version: Option<String>,
    pub build_configuration: Option<String>,
    pub time_of_crash: Option<String>,
    pub crash_guid: Option<String>,
    pub is_ensure: bool,
    pub is_assert: bool,
}

/// The build in parentheses: `macOS 27.2 (26B123)` gives `26B123`.
pub fn os_build_of(text: &str) -> Option<String> {
    let open = text.rfind('(')?;
    let close = text[open..].find(')')? + open;
    let build = text[open + 1..close].trim();
    (!build.is_empty() && build.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| build.to_string())
}

/// `2423ab55dd623a34b8c81d496c0c7be4` or any casing/dashing, as
/// `2423AB55-DD62-3A34-B8C8-1D496C0C7BE4`.
pub fn normalize_uuid(text: &str) -> Option<String> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let h = hex.to_ascii_uppercase();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

fn hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
}

/// Reads whichever kind of report `path` is: an Unreal crash folder, an
/// .ips file or a text crash report.
pub fn read(path: &Path) -> Result<Report, String> {
    if path.is_dir() {
        return read_unreal_folder(path);
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if text.trim_start().starts_with('{') {
        parse_ips(path, &text)
    } else {
        parse_text(path, &text, Source::Text)
    }
}

fn read_unreal_folder(dir: &Path) -> Result<Report, String> {
    let context_file = dir.join("CrashContext.runtime-xml");
    let context = std::fs::read_to_string(&context_file).ok();
    let unreal = context.as_deref().map(parse_context);
    let text_report = match std::fs::read_to_string(dir.join("minidump.dmp")) {
        Ok(text) if text.starts_with("Incident Identifier") || text.contains("Binary Images:") => {
            Some(parse_text(dir, &text, Source::Unreal)?)
        }
        _ => None,
    };
    // On a Mac, Unreal's text report marks its own crash-handling thread as
    // the one that crashed, and its symbolized call stack drops the frames it
    // couldn't name, which are often the game's own. The context's portable
    // call stack keeps every frame of the thread that really crashed, so that
    // is the crashing thread; the text report adds the images (with UUIDs)
    // and the other threads.
    let mut report = match (&context, text_report) {
        (Some(xml), Some(mut text)) => {
            let frames = context_frames(xml, &mut text.images);
            for thread in &mut text.threads {
                thread.crashed = false;
            }
            if !frames.is_empty() {
                text.threads.insert(
                    0,
                    Thread {
                        index: usize::MAX,
                        name: Some("crashed (from the crash context)".into()),
                        crashed: true,
                        frames,
                    },
                );
            }
            text
        }
        (Some(xml), None) => parse_context_stack(dir, xml),
        (None, Some(text)) => text,
        (None, None) => {
            return Err(format!(
                "{} has no CrashContext.runtime-xml or crash report",
                dir.display()
            ));
        }
    };
    report.source = Source::Unreal;
    // Unreal's own time (UTC) sorts the same way for every crash folder.
    if let Some(time) = unreal.as_ref().and_then(|u| u.time_of_crash.clone()) {
        report.time = Some(time);
    }
    report.unreal = unreal;
    Ok(report)
}

// --- Unreal's crash context -------------------------------------------------------

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(&xml[start..end])
}

pub fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn parse_context(xml: &str) -> UnrealContext {
    let get = |name: &str| {
        tag(xml, name)
            .map(|v| unescape(v.trim()))
            .filter(|v| !v.is_empty())
    };
    let truthy = |name: &str| get(name).is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1");
    UnrealContext {
        crash_type: get("CrashType"),
        error_message: get("ErrorMessage"),
        game_name: get("GameName"),
        engine_version: get("EngineVersion"),
        build_configuration: get("BuildConfiguration"),
        time_of_crash: get("TimeOfCrash"),
        crash_guid: get("CrashGUID"),
        is_ensure: truthy("IsEnsure"),
        is_assert: truthy("IsAssert"),
    }
}

/// One line of the context's portable call stack: `module 0x<base> + <hex
/// offset>`. A module Unreal couldn't name shows as `Unknown 0x0 + <address>`,
/// with only the low 32 bits of the address.
fn portable_line(line: &str) -> Option<(String, u64, u64)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let plus = parts.iter().position(|p| *p == "+")?;
    if plus < 1 || plus + 1 >= parts.len() {
        return None;
    }
    let base = hex(parts[plus - 1])?;
    let offset = hex(parts[plus + 1])?;
    let module = if plus >= 2 {
        parts[..plus - 1].join(" ")
    } else {
        String::new()
    };
    Some((module, base, offset))
}

/// The address an `Unknown 0x0 + <low 32 bits>` frame really had: the image
/// whose range holds those low bits. Unreal prints `Unknown` for modules its
/// own lookup missed, which are the dylibs it loaded itself, never macOS's
/// shared-cache libraries; so when a macOS library also happens to fit, the
/// one other image wins. More than one candidate after that: no guess.
pub fn widen_truncated(low: u64, images: &[Image]) -> Option<(usize, u64)> {
    let mut fits = Vec::new();
    for (i, image) in images.iter().enumerate() {
        let Some(end) = image.end else { continue };
        let high = image.base & !0xffff_ffff;
        for candidate in [high | low, (high + (1 << 32)) | low] {
            if candidate >= image.base && candidate <= end {
                fits.push((i, candidate));
            }
        }
    }
    let is_system = |i: usize| {
        images[i].path.as_ref().is_some_and(|p| {
            let p = p.to_string_lossy();
            p.starts_with("/System/") || p.starts_with("/usr/lib/")
        })
    };
    if fits.len() == 1 {
        return fits.pop();
    }
    let others: Vec<(usize, u64)> = fits.into_iter().filter(|(i, _)| !is_system(*i)).collect();
    if others.len() == 1 {
        others.into_iter().next()
    } else {
        None
    }
}

/// The crashing thread's frames from the context's portable call stack,
/// matched to `images` (adding images it names that aren't there).
pub fn context_frames(xml: &str, images: &mut Vec<Image>) -> Vec<Frame> {
    let stack = tag(xml, "PCallStack").map(unescape).unwrap_or_default();
    let mut frames = Vec::new();
    for line in stack.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some((module, base, offset)) = portable_line(line) else {
            continue;
        };
        let (image, address) = if base == 0 {
            match widen_truncated(offset, images) {
                Some((i, address)) => (Some(i), address),
                None => (None, offset),
            }
        } else {
            let address = base + offset;
            let image = images.iter().position(|i| i.base == base).or_else(|| {
                images.push(Image {
                    name: module.clone(),
                    path: None,
                    uuid: None,
                    arch: None,
                    base,
                    end: None,
                });
                Some(images.len() - 1)
            });
            (image, address)
        };
        let module = image
            .and_then(|i| images.get(i))
            .map(|i| i.name.clone())
            .unwrap_or(module);
        frames.push(Frame {
            index: frames.len(),
            image,
            module,
            address,
            report_symbol: None,
        });
    }
    frames
}

/// A crash folder with no text report: just the context's crashing thread,
/// with no UUIDs, so symbols can only be matched by name.
fn parse_context_stack(dir: &Path, xml: &str) -> Report {
    let mut images = Vec::new();
    let frames = context_frames(xml, &mut images);
    Report {
        source: Source::Unreal,
        path: dir.to_path_buf(),
        process: tag(xml, "ExecutableName").map(unescape),
        time: None,
        exception: tag(xml, "ErrorMessage").map(|m| unescape(m.trim())),
        os_build: None,
        unreal: None,
        images,
        threads: vec![Thread {
            index: 0,
            name: None,
            crashed: true,
            frames,
        }],
    }
}

// --- the text format ---------------------------------------------------------------

/// Parses the text crash report format: a header of `Key: value` lines,
/// `Thread N Crashed:` sections of frames, and `Binary Images:`.
pub fn parse_text(path: &Path, text: &str, source: Source) -> Result<Report, String> {
    let mut header = std::collections::HashMap::new();
    let mut threads: Vec<Thread> = Vec::new();
    let mut thread_names = std::collections::HashMap::new();
    let mut images = Vec::new();
    let mut in_images = false;
    let mut current: Option<Thread> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Binary Images:") {
            if let Some(t) = current.take() {
                threads.push(t);
            }
            in_images = true;
            continue;
        }
        if in_images {
            if let Some(image) = parse_image_line(trimmed) {
                images.push(image);
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Thread ")
            && let Some(split) = rest.find(|c: char| !c.is_ascii_digit())
            && let Ok(index) = rest[..split].parse::<usize>()
        {
            let tail = rest[split..].trim();
            if let Some(name) = tail.strip_prefix("name:") {
                thread_names.insert(index, name.trim().to_string());
                continue;
            }
            if tail == "Crashed:" || tail == ":" || tail.starts_with("Crashed") {
                if let Some(t) = current.take() {
                    threads.push(t);
                }
                current = Some(Thread {
                    index,
                    name: None,
                    crashed: tail.starts_with("Crashed"),
                    frames: Vec::new(),
                });
                continue;
            }
        }
        if let Some(thread) = current.as_mut() {
            if trimmed.is_empty() {
                threads.push(current.take().expect("current thread"));
                continue;
            }
            if let Some(frame) = parse_frame_line(trimmed) {
                thread.frames.push(frame);
            }
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':')
            && !key.is_empty()
            && !key.contains("  ")
        {
            header
                .entry(key.trim().to_string())
                .or_insert_with(|| value.trim().to_string());
        }
    }
    if let Some(t) = current.take() {
        threads.push(t);
    }
    if threads.is_empty() {
        return Err(format!(
            "{} has no threads in it; is it a crash report?",
            path.display()
        ));
    }
    for thread in &mut threads {
        thread.name = thread_names.get(&thread.index).cloned();
        for frame in &mut thread.frames {
            frame.image = images
                .iter()
                .position(|i: &Image| i.name == frame.module)
                .or_else(|| {
                    images
                        .iter()
                        .position(|i| frame.address >= i.base && i.name.is_empty())
                });
        }
    }
    if let Some(crashed) = header
        .get("Crashed Thread")
        .and_then(|v| v.split_whitespace().next()?.parse::<usize>().ok())
    {
        for thread in &mut threads {
            thread.crashed = thread.index == crashed;
        }
    }
    let exception = header.get("Exception Type").cloned();
    Ok(Report {
        source,
        path: path.to_path_buf(),
        process: header
            .get("Process")
            .map(|p| p.split(" [").next().unwrap_or(p).to_string()),
        time: header.get("Date/Time").cloned(),
        exception,
        os_build: header.get("OS Version").and_then(|v| os_build_of(v)),
        unreal: None,
        images,
        threads,
    })
}

/// `5   libFoo.dylib    0x00000001068233a0 0x1066f8000 + 1225632` or
/// `5   libFoo.dylib    0x00000001068233a0 FooBar() + 12`.
fn parse_frame_line(line: &str) -> Option<Frame> {
    let (index, rest) = line.split_once(char::is_whitespace)?;
    let index = index.parse::<usize>().ok()?;
    let rest = rest.trim_start();
    // The module name runs up to the address, which is the first " 0x" token.
    let at = rest.find(" 0x")?;
    let module = rest[..at].trim().to_string();
    let after = rest[at + 1..].trim_start();
    let (address, tail) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
    let address = hex(address)?;
    let tail = tail.trim();
    let report_symbol = if tail.starts_with("0x") || tail.is_empty() {
        None
    } else {
        Some(strip_offset(tail).to_string())
    };
    Some(Frame {
        index,
        image: None,
        module,
        address,
        report_symbol,
    })
}

/// `FooBar() + 12` as `FooBar()`.
pub fn strip_offset(symbol: &str) -> &str {
    match symbol.rfind(" + ") {
        Some(at) if symbol[at + 3..].trim().chars().all(|c| c.is_ascii_digit()) => &symbol[..at],
        _ => symbol,
    }
}

/// `0x102c14000 - 0x102c73fff +Name arm64 (1.0 - 1.0) <uuid> /path/to/Name`.
fn parse_image_line(line: &str) -> Option<Image> {
    let (start, rest) = line.split_once(char::is_whitespace)?;
    let base = hex(start)?;
    let rest = rest.trim_start().strip_prefix('-')?.trim_start();
    let (end, rest) = rest.split_once(char::is_whitespace)?;
    let end = hex(end);
    let open = rest.find('<')?;
    let close = rest[open..].find('>')? + open;
    let uuid = normalize_uuid(&rest[open + 1..close]);
    let path = rest[close + 1..].trim();
    let before: Vec<&str> = rest[..open].split_whitespace().collect();
    // Name, then arch, then an optional "(version)".
    let name = before
        .first()
        .map(|n| n.trim_start_matches('+').to_string())
        .unwrap_or_default();
    let arch = before
        .get(1)
        .filter(|a| !a.starts_with('('))
        .map(|a| a.to_string());
    Some(Image {
        name,
        path: (!path.is_empty()).then(|| PathBuf::from(path)),
        uuid,
        arch,
        base,
        end,
    })
}

// --- .ips --------------------------------------------------------------------------

/// Parses a macOS .ips crash report: a JSON header line, then the JSON report.
pub fn parse_ips(path: &Path, text: &str) -> Result<Report, String> {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let header: serde_json::Value = serde_json::from_str(first)
        .map_err(|e| format!("{} isn't an .ips report: {e}", path.display()))?;
    let body: serde_json::Value = if rest.trim().is_empty() {
        header.clone()
    } else {
        serde_json::from_str(rest)
            .map_err(|e| format!("{} isn't an .ips report: {e}", path.display()))?
    };
    let str_of =
        |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let images: Vec<Image> = body
        .get("usedImages")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .map(|i| Image {
                    name: str_of(i, "name").unwrap_or_default(),
                    path: str_of(i, "path").map(PathBuf::from),
                    uuid: str_of(i, "uuid").and_then(|u| normalize_uuid(&u)),
                    arch: str_of(i, "arch"),
                    base: i.get("base").and_then(|b| b.as_u64()).unwrap_or(0),
                    end: match (
                        i.get("base").and_then(|b| b.as_u64()),
                        i.get("size").and_then(|b| b.as_u64()),
                    ) {
                        (Some(base), Some(size)) if size > 0 => Some(base + size - 1),
                        _ => None,
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    let faulting = body
        .get("faultingThread")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let threads: Vec<Thread> = body
        .get("threads")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .enumerate()
                .map(|(index, t)| Thread {
                    index,
                    name: str_of(t, "name").or_else(|| str_of(t, "queue")),
                    crashed: faulting == Some(index)
                        || t.get("triggered").and_then(|v| v.as_bool()) == Some(true),
                    frames: t
                        .get("frames")
                        .and_then(|f| f.as_array())
                        .map(|frames| {
                            frames
                                .iter()
                                .enumerate()
                                .map(|(i, f)| {
                                    let image = f
                                        .get("imageIndex")
                                        .and_then(|x| x.as_u64())
                                        .map(|x| x as usize);
                                    let offset =
                                        f.get("imageOffset").and_then(|x| x.as_u64()).unwrap_or(0);
                                    let base = image
                                        .and_then(|x| images.get(x))
                                        .map(|img| img.base)
                                        .unwrap_or(0);
                                    Frame {
                                        index: i,
                                        image: image.filter(|x| *x < images.len()),
                                        module: image
                                            .and_then(|x| images.get(x))
                                            .map(|img| img.name.clone())
                                            .unwrap_or_default(),
                                        address: base + offset,
                                        report_symbol: str_of(f, "symbol"),
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    if threads.is_empty() {
        return Err(format!(
            "{} has no threads in it; is it a crash report?",
            path.display()
        ));
    }
    let exception = body.get("exception").map(|e| {
        let kind = str_of(e, "type").unwrap_or_default();
        match str_of(e, "signal") {
            Some(signal) => format!("{kind} ({signal})"),
            None => kind,
        }
    });
    Ok(Report {
        source: Source::Ips,
        path: path.to_path_buf(),
        process: str_of(&body, "procName").or_else(|| str_of(&header, "app_name")),
        time: str_of(&body, "captureTime").or_else(|| str_of(&header, "timestamp")),
        exception,
        os_build: str_of(&header, "os_version").and_then(|v| os_build_of(&v)),
        unreal: None,
        images,
        threads,
    })
}
