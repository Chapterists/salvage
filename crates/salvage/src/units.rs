//! Human-friendly sizes.

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * KIB;
pub const GIB: u64 = 1024 * MIB;

/// Formats a byte count the way people read it: `512 KB`, `2.8 GB`, `16 GB`.
/// Uses 1024-based units with the familiar short labels.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if n < KIB {
        return format!("{n} bytes");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 || (value - value.round()).abs() < 0.05 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(bytes(12), "12 bytes");
        assert_eq!(bytes(16 * GIB), "16 GB");
        assert_eq!(bytes(2 * GIB + 800 * MIB), "2.8 GB");
        assert_eq!(bytes(117_031_568 * KIB), "112 GB");
        assert_eq!(bytes(1536 * KIB), "1.5 MB");
    }
}
