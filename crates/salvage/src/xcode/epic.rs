//! What Epic's documentation says about Xcode for each engine version.
//!
//! The engine's own Apple_SDK.json gives the range its build check accepts;
//! Epic's macOS requirements page is narrower and names versions that don't
//! work. Each row quotes the page as it read on the date checked.

use crate::version::Version;

pub const SOURCE: &str = "https://dev.epicgames.com/documentation/en-us/unreal-engine/macos-development-requirements-for-unreal-engine";

/// What Epic recommends for an engine version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recommended {
    /// One version: "Xcode 26.1.1".
    Exactly(&'static str),
    /// A version or anything newer: "15.4 or newer".
    AtLeast(&'static str),
    /// The newest release of a major version: "Latest Xcode 14".
    LatestOf(u64),
}

#[derive(Clone, Copy, Debug)]
pub struct EpicRow {
    /// The engine versions the row covers, as the page writes them.
    pub engines: &'static str,
    /// First and last minor version of UE 5 the row covers.
    pub first_minor: u64,
    pub last_minor: u64,
    pub minimum: &'static str,
    pub recommended: Recommended,
    /// Versions the page says don't work. A version here matches its point
    /// releases too ("26.4" covers 26.4.1).
    pub not_compatible: &'static [&'static str],
    /// The row as the page shows it.
    pub says: &'static str,
    pub source: &'static str,
    pub checked: &'static str,
}

const CHECKED: &str = "2026-11-06";

pub const ROWS: [EpicRow; 7] = [
    EpicRow {
        engines: "5.8",
        first_minor: 8,
        last_minor: 8,
        minimum: "26.0",
        recommended: Recommended::Exactly("26.1.1"),
        not_compatible: &["26.4"],
        says: "5.8 | Sonoma 14.5 | Latest macOS Sequoia 15 | Xcode 26.0 | Xcode 26.1.1 | Xcode 26.4 is not compatible with Unreal Engine.",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.6 - 5.7",
        first_minor: 6,
        last_minor: 7,
        minimum: "15.2",
        recommended: Recommended::AtLeast("15.4"),
        not_compatible: &[],
        says: "5.6 - 5.7 | Sonoma 14.0 | Latest macOS 14 Sonoma | Xcode 15.2 | 15.4 or newer",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.5",
        first_minor: 5,
        last_minor: 5,
        minimum: "15.2",
        recommended: Recommended::AtLeast("15.4"),
        not_compatible: &[],
        says: "5.5 | Ventura 13.5 | Latest macOS 13 Ventura | Xcode 15.2 | 15.4 or newer",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.4",
        first_minor: 4,
        last_minor: 4,
        minimum: "14.1",
        recommended: Recommended::LatestOf(14),
        not_compatible: &[],
        says: "5.4 | macOS 13 Ventura | Latest macOS 13 Ventura | Xcode 14.1 | Latest Xcode 14",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.2 - 5.3",
        first_minor: 2,
        last_minor: 3,
        minimum: "14.1",
        recommended: Recommended::LatestOf(14),
        not_compatible: &[],
        says: "5.2 - 5.3 | macOS 12.5 Monterey | Latest macOS 13 Ventura | Xcode 14.1 | Latest Xcode 14",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.1",
        first_minor: 1,
        last_minor: 1,
        minimum: "13.4.1",
        recommended: Recommended::LatestOf(14),
        not_compatible: &[],
        says: "5.1 | macOS 12 Monterey | Latest macOS 13 Ventura | Xcode 13.4.1 | Latest Xcode 14",
        source: SOURCE,
        checked: CHECKED,
    },
    EpicRow {
        engines: "5.0",
        first_minor: 0,
        last_minor: 0,
        minimum: "12.4",
        recommended: Recommended::LatestOf(13),
        not_compatible: &[],
        says: "5.0 | macOS Catalina 10.15.7 | Latest macOS Monterey | Xcode 12.4 | Latest Xcode 13",
        source: SOURCE,
        checked: CHECKED,
    },
];

/// The row for an engine version (UE 5 only).
pub fn row_for(engine: &Version) -> Option<&'static EpicRow> {
    let parts = engine.parts();
    if parts.first() != Some(&5) {
        return None;
    }
    let minor = parts.get(1).copied().unwrap_or(0);
    ROWS.iter()
        .find(|row| (row.first_minor..=row.last_minor).contains(&minor))
}

impl EpicRow {
    pub fn minimum(&self) -> Version {
        Version::parse(self.minimum).expect("table versions parse")
    }

    /// The listed version this one falls under, if Epic says it doesn't work.
    pub fn not_compatible_match(&self, xcode: &Version) -> Option<&'static str> {
        self.not_compatible.iter().copied().find(|listed| {
            let listed = Version::parse(listed).expect("table versions parse");
            let n = listed.parts().len();
            xcode
                .parts()
                .iter()
                .chain(std::iter::repeat(&0))
                .take(n)
                .eq(listed.parts().iter())
        })
    }

    pub fn is_recommended(&self, xcode: &Version) -> bool {
        match self.recommended {
            Recommended::Exactly(v) => Version::parse(v).is_some_and(|v| *xcode == v),
            Recommended::AtLeast(v) => Version::parse(v).is_some_and(|v| *xcode >= v),
            Recommended::LatestOf(major) => xcode.parts().first() == Some(&major),
        }
    }

    pub fn describe_recommended(&self) -> String {
        match self.recommended {
            Recommended::Exactly(v) => format!("Xcode {v}"),
            Recommended::AtLeast(v) => format!("Xcode {v} or newer"),
            Recommended::LatestOf(major) => format!("the latest Xcode {major}"),
        }
    }

    /// A concrete version to install when nothing installed is recommended.
    pub fn version_to_install(&self) -> Option<&'static str> {
        match self.recommended {
            Recommended::Exactly(v) | Recommended::AtLeast(v) => Some(v),
            Recommended::LatestOf(_) => None,
        }
    }
}
