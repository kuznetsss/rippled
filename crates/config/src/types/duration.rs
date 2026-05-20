use std::time::Duration;
use crate::error::ConfigError;

const MIN_AMENDMENT_MAJORITY_SECS: u64 = 15 * 60; // 15 minutes

/// Parse the `amendment_majority_time` value.
///
/// Grammar (INI loose):  `^\s*(\d+)\s*(minutes|hours|days|weeks)\s*(.*)$`
/// Grammar (TOML strict): `^\s*(\d+)\s*(minutes|hours|days|weeks)\s*$`
///
/// Floor: 15 minutes. Values below the floor are clamped to 15 minutes.
///
/// `strict = true` rejects trailing junk after the unit word.
pub fn parse_amendment_majority_time(s: &str, strict: bool) -> Result<Duration, ConfigError> {
    let s = s.trim();

    // Find the numeric prefix
    let digit_end = s
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(s.len());

    if digit_end == 0 {
        return Err(ConfigError::grammar(
            "amendment_majority_time",
            s,
            "expected a positive integer followed by a time unit",
        ));
    }

    let count: u64 = s[..digit_end].parse().map_err(|_| {
        ConfigError::grammar(
            "amendment_majority_time",
            s,
            "integer overflow in time value",
        )
    })?;

    let rest = s[digit_end..].trim_start();

    // Match the unit
    let (unit_secs, unit_len) = if rest.starts_with("weeks") {
        (7 * 24 * 3600u64, "weeks".len())
    } else if rest.starts_with("days") {
        (24 * 3600u64, "days".len())
    } else if rest.starts_with("hours") {
        (3600u64, "hours".len())
    } else if rest.starts_with("minutes") {
        (60u64, "minutes".len())
    } else {
        return Err(ConfigError::grammar(
            "amendment_majority_time",
            s,
            "expected unit: minutes, hours, days, or weeks",
        ));
    };

    let after_unit = &rest[unit_len..];

    if strict && !after_unit.trim().is_empty() {
        return Err(ConfigError::grammar(
            "amendment_majority_time",
            s,
            "trailing content after time unit is not allowed in TOML mode",
        ));
    }

    let secs = count
        .checked_mul(unit_secs)
        .ok_or_else(|| {
            ConfigError::grammar(
                "amendment_majority_time",
                s,
                "time value overflows",
            )
        })?;

    // Floor: 15 minutes
    let secs = secs.max(MIN_AMENDMENT_MAJORITY_SECS);
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes() {
        let d = parse_amendment_majority_time("30 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn hours() {
        let d = parse_amendment_majority_time("2 hours", false).unwrap();
        assert_eq!(d, Duration::from_secs(2 * 3600));
    }

    #[test]
    fn days() {
        let d = parse_amendment_majority_time("1 days", false).unwrap();
        assert_eq!(d, Duration::from_secs(24 * 3600));
    }

    #[test]
    fn weeks() {
        let d = parse_amendment_majority_time("1 weeks", false).unwrap();
        assert_eq!(d, Duration::from_secs(7 * 24 * 3600));
    }

    #[test]
    fn floor_15_minutes() {
        // 5 minutes gets floored to 15
        let d = parse_amendment_majority_time("5 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn trailing_junk_ok_in_ini() {
        // INI loose mode: trailing content ignored
        let d = parse_amendment_majority_time("15 minutes # comment", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn trailing_junk_rejected_in_toml() {
        let result = parse_amendment_majority_time("15 minutes extra", true);
        assert!(result.is_err());
    }

    #[test]
    fn missing_unit_is_error() {
        assert!(parse_amendment_majority_time("30", false).is_err());
    }

    #[test]
    fn empty_is_error() {
        assert!(parse_amendment_majority_time("", false).is_err());
    }

    // ---- additional coverage ----

    #[test]
    fn minutes_no_space() {
        // No whitespace between number and unit
        let d = parse_amendment_majority_time("30minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn hours_no_space() {
        let d = parse_amendment_majority_time("2hours", false).unwrap();
        assert_eq!(d, Duration::from_secs(2 * 3600));
    }

    #[test]
    fn days_no_space() {
        let d = parse_amendment_majority_time("1days", false).unwrap();
        assert_eq!(d, Duration::from_secs(24 * 3600));
    }

    #[test]
    fn weeks_no_space() {
        let d = parse_amendment_majority_time("1weeks", false).unwrap();
        assert_eq!(d, Duration::from_secs(7 * 24 * 3600));
    }

    #[test]
    fn floor_14_minutes_clamped_to_15() {
        // 14 minutes is below the 15-minute floor — should be clamped to 15
        let d = parse_amendment_majority_time("14 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn floor_exactly_15_minutes_unchanged() {
        let d = parse_amendment_majority_time("15 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn floor_1_minute_clamped() {
        let d = parse_amendment_majority_time("1 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn floor_0_minutes_clamped() {
        let d = parse_amendment_majority_time("0 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(15 * 60));
    }

    #[test]
    fn trailing_junk_ini_with_hash_comment() {
        let d = parse_amendment_majority_time("30 minutes # this is a comment", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn trailing_junk_ini_with_semicolon() {
        let d = parse_amendment_majority_time("30 minutes ; comment", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn toml_strict_no_trailing() {
        // clean input should pass in strict mode
        let d = parse_amendment_majority_time("30 minutes", true).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn toml_strict_rejects_hash_comment() {
        assert!(parse_amendment_majority_time("30 minutes # comment", true).is_err());
    }

    #[test]
    fn invalid_unit() {
        assert!(parse_amendment_majority_time("30 seconds", false).is_err());
    }

    #[test]
    fn unit_prefix_match_minutess_is_ok() {
        // "minutess" starts with "minutes" — the parser accepts it in loose mode
        // because it matches the "minutes" prefix and the trailing 's' becomes
        // trailing junk (ignored in INI/loose mode).
        let d = parse_amendment_majority_time("30 minutess", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn unit_prefix_match_minutess_strict_is_err() {
        // In strict mode trailing junk is rejected.
        assert!(parse_amendment_majority_time("30 minutess", true).is_err());
    }

    #[test]
    fn invalid_number_leading_alpha() {
        assert!(parse_amendment_majority_time("abc minutes", false).is_err());
    }

    #[test]
    fn leading_whitespace_trimmed() {
        let d = parse_amendment_majority_time("  30 minutes", false).unwrap();
        assert_eq!(d, Duration::from_secs(30 * 60));
    }

    #[test]
    fn multiple_weeks() {
        let d = parse_amendment_majority_time("2 weeks", false).unwrap();
        assert_eq!(d, Duration::from_secs(2 * 7 * 24 * 3600));
    }

    #[test]
    fn multiple_days() {
        let d = parse_amendment_majority_time("3 days", false).unwrap();
        assert_eq!(d, Duration::from_secs(3 * 24 * 3600));
    }
}
