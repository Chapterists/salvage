//! Just enough of Unreal's ini format to resolve array settings across the
//! config hierarchy (`+Key=` adds, `-Key=` removes, `!Key=` clears).

/// One `key=value` line, with the section it appeared in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub section: String,
    pub op: Op,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `Key=Value`: replaces whatever came before.
    Set,
    /// `+Key=Value`: adds unless already present.
    AddUnique,
    /// `.Key=Value`: adds even if already present.
    Add,
    /// `-Key=Value`: removes a matching value.
    Remove,
    /// `!Key=...`: clears the array.
    Clear,
}

/// Parses ini text into entries, skipping comments and blank lines.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut section = String::new();
    let mut entries = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (op, key) = match key.chars().next() {
            Some('+') => (Op::AddUnique, &key[1..]),
            Some('.') => (Op::Add, &key[1..]),
            Some('-') => (Op::Remove, &key[1..]),
            Some('!') => (Op::Clear, &key[1..]),
            _ => (Op::Set, key),
        };
        entries.push(Entry {
            section: section.clone(),
            op,
            key: key.trim().to_string(),
            value: value.trim().to_string(),
        });
    }
    entries
}

/// Resolves an array setting across ini files, applied in order (base first).
pub fn resolve_array<S: AsRef<str>>(layers: &[S], section: &str, key: &str) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for layer in layers {
        for entry in parse(layer.as_ref()) {
            if !entry.section.eq_ignore_ascii_case(section) || !entry.key.eq_ignore_ascii_case(key)
            {
                continue;
            }
            match entry.op {
                Op::Set => {
                    values.clear();
                    values.push(entry.value);
                }
                Op::AddUnique => {
                    if !values.contains(&entry.value) {
                        values.push(entry.value);
                    }
                }
                Op::Add => values.push(entry.value),
                Op::Remove => values.retain(|v| v != &entry.value),
                Op::Clear => values.clear(),
            }
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTION: &str = "/Script/Example.Settings";

    #[test]
    fn layers_apply_in_order() {
        let base = "[/Script/Example.Settings]\n+Things=A\n+Things=B\n; +Things=Commented\n";
        let project = "[/Script/Example.Settings]\n-Things=A\n+Things=C\n+Things=B\n";
        assert_eq!(
            resolve_array(&[base, project], SECTION, "Things"),
            ["B", "C"]
        );
    }

    #[test]
    fn clear_and_set_replace_earlier_values() {
        let base = "[/Script/Example.Settings]\n+Things=A\n+Things=B\n";
        let cleared = "[/Script/Example.Settings]\n!Things=ClearArray\n+Things=Z\n";
        assert_eq!(resolve_array(&[base, cleared], SECTION, "Things"), ["Z"]);
        let set = "[/Script/Example.Settings]\nThings=Only\n";
        assert_eq!(resolve_array(&[base, set], SECTION, "Things"), ["Only"]);
    }

    #[test]
    fn other_sections_are_ignored() {
        let text = "[Other]\n+Things=X\n[/Script/Example.Settings]\n+things=Y\n";
        assert_eq!(resolve_array(&[text], SECTION, "Things"), ["Y"]);
    }
}
