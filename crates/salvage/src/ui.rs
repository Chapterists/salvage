//! Terminal styling. Output is written through `anstream`, which drops the
//! colors when stdout isn't a terminal or `NO_COLOR` is set.

use anstyle::{AnsiColor, Style};

pub const BOLD: Style = Style::new().bold();
pub const DIM: Style = Style::new().dimmed();
pub const GOOD: Style = AnsiColor::Green.on_default();
pub const WARN: Style = AnsiColor::Yellow.on_default();
pub const BAD: Style = AnsiColor::Red.on_default().bold();
pub const ACCENT: Style = AnsiColor::Cyan.on_default();
pub const COMMAND: Style = AnsiColor::Cyan.on_default().bold();

/// Wraps `text` in a style.
pub fn paint(style: Style, text: impl std::fmt::Display) -> String {
    format!("{style}{text}{style:#}")
}

/// Usable output width: `$COLUMNS` if set, clamped to something readable.
pub fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(60, 120)
}

/// Greedy word wrap. Words longer than the width get a line to themselves.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_word_boundaries() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("a-very-long-word x", 5), ["a-very-long-word", "x"]);
        assert!(wrap("   ", 10).is_empty());
    }
}
