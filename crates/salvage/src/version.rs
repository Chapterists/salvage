//! Dotted version numbers such as `15.2.0` or `27.2`, compared numerically.

use std::cmp::Ordering;
use std::fmt;

/// A dotted version. Missing trailing parts count as zero, so `27.2 == 27.2.0`.
#[derive(Clone, Debug, Eq)]
pub struct Version(Vec<u64>);

impl Version {
    /// Parses `"15.2"`, `"15.2.0"` or `"26.1.1"`. Returns `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        text.split('.')
            .map(|part| part.trim().parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
            .map(Version)
    }

    pub fn parts(&self) -> &[u64] {
        &self.0
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.0.len().max(other.0.len());
        (0..len)
            .map(|i| {
                let a = self.0.get(i).copied().unwrap_or(0);
                let b = other.0.get(i).copied().unwrap_or(0);
                a.cmp(&b)
            })
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_dotted_numbers() {
        assert_eq!(v("26.1.1").parts(), &[26, 1, 1]);
        assert_eq!(v(" 27.2 ").parts(), &[27, 2]);
        assert!(Version::parse("").is_none());
        assert!(Version::parse("27.beta").is_none());
    }

    #[test]
    fn missing_parts_count_as_zero() {
        assert_eq!(v("27.2"), v("27.2.0"));
        assert!(v("27.2") < v("27.9.0"));
        assert!(v("27.10") > v("27.9.0"));
        assert!(v("15.1.9") < v("15.2"));
    }

    #[test]
    fn displays_as_written() {
        assert_eq!(v("15.2.0").to_string(), "15.2.0");
    }
}
