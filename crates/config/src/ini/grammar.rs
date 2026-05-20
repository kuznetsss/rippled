//! INI-specific scalar parsers.
//!
//! These are different from TOML's serde-native parsers:
//! - Booleans:  `0|1|true|false` (case-insensitive).
//! - Integers:  decimal-only; optional leading `+`; reject `0x`/`0o`/`0b` prefixes,
//!              leading `-` (for unsigned targets), and trailing junk.

use std::str::FromStr;
use crate::error::ConfigError;

/// Parse an INI boolean: `0`, `1`, `true`, `false` (case-insensitive).
pub(super) fn parse_ini_bool(s: &str) -> Result<bool, ConfigError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(ConfigError::grammar(
            "bool",
            s,
            "expected 0, 1, true, or false",
        )),
    }
}

/// Parse an INI decimal integer into any type that implements `FromStr`.
///
/// Rules:
/// - Optional leading `+`.
/// - ASCII decimal digits only; reject `0x`, `0o`, `0b` prefixes.
/// - No leading `-` is accepted for unsigned targets (the calling context decides
///   the target type; if `T` is `u32` and you pass `"-1"` the `from_str` will fail).
/// - No trailing non-digit, non-whitespace characters (e.g. `"42abc"` is an error).
/// - Hard-fail on overflow (relies on `T::from_str` returning an error for overflow).
pub(super) fn parse_ini_int<T, E>(s: &str) -> Result<T, ConfigError>
where
    T: FromStr<Err = E>,
    E: std::fmt::Display,
{
    let s = s.trim();

    if s.is_empty() {
        return Err(ConfigError::grammar("integer", s, "empty value"));
    }

    // Strip optional leading sign.
    let (sign, digits) = if s.starts_with('+') {
        ('+', &s[1..])
    } else if s.starts_with('-') {
        ('-', &s[1..])
    } else {
        (' ', s)
    };
    let _ = sign; // sign is part of the original `s` that we pass to from_str

    // Reject hex / octal / binary prefixes.
    if digits.starts_with("0x")
        || digits.starts_with("0X")
        || digits.starts_with("0o")
        || digits.starts_with("0O")
        || digits.starts_with("0b")
        || digits.starts_with("0B")
    {
        return Err(ConfigError::grammar(
            "integer",
            s,
            "only decimal integers are accepted in INI (no 0x/0o/0b prefix)",
        ));
    }

    // Must be all ASCII digits (after stripping sign).
    if digits.is_empty() {
        return Err(ConfigError::grammar("integer", s, "no digits after sign"));
    }
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(ConfigError::grammar(
            "integer",
            s,
            "non-decimal character in integer value",
        ));
    }

    // Now parse; if T is unsigned and s starts with `-` this will return a
    // parse error from the standard library, which is what we want.
    s.parse::<T>().map_err(|e| {
        ConfigError::grammar("integer", s, format!("{e}"))
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- bool ---

    #[test]
    fn bool_true_forms() {
        assert_eq!(parse_ini_bool("1").unwrap(), true);
        assert_eq!(parse_ini_bool("true").unwrap(), true);
        assert_eq!(parse_ini_bool("True").unwrap(), true);
        assert_eq!(parse_ini_bool("TRUE").unwrap(), true);
    }

    #[test]
    fn bool_false_forms() {
        assert_eq!(parse_ini_bool("0").unwrap(), false);
        assert_eq!(parse_ini_bool("false").unwrap(), false);
        assert_eq!(parse_ini_bool("False").unwrap(), false);
        assert_eq!(parse_ini_bool("FALSE").unwrap(), false);
    }

    #[test]
    fn bool_invalid() {
        assert!(parse_ini_bool("yes").is_err());
        assert!(parse_ini_bool("no").is_err());
        assert!(parse_ini_bool("2").is_err());
        assert!(parse_ini_bool("").is_err());
        assert!(parse_ini_bool("banana").is_err());
    }

    // --- int ---

    #[test]
    fn int_plain_u32() {
        let v: u32 = parse_ini_int("42").unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn int_leading_plus() {
        let v: u32 = parse_ini_int("+99").unwrap();
        assert_eq!(v, 99);
    }

    #[test]
    fn int_i32_negative() {
        let v: i32 = parse_ini_int("-7").unwrap();
        assert_eq!(v, -7);
    }

    #[test]
    fn int_reject_hex() {
        assert!(parse_ini_int::<u32, _>("0xFF").is_err());
        assert!(parse_ini_int::<u32, _>("0x10").is_err());
    }

    #[test]
    fn int_reject_octal() {
        assert!(parse_ini_int::<u32, _>("0o17").is_err());
    }

    #[test]
    fn int_reject_binary() {
        assert!(parse_ini_int::<u32, _>("0b1010").is_err());
    }

    #[test]
    fn int_reject_trailing_junk() {
        assert!(parse_ini_int::<u32, _>("42abc").is_err());
    }

    #[test]
    fn int_reject_empty() {
        assert!(parse_ini_int::<u32, _>("").is_err());
    }

    #[test]
    fn int_reject_sign_only() {
        assert!(parse_ini_int::<u32, _>("+").is_err());
    }

    #[test]
    fn int_overflow() {
        // u32 max is 4_294_967_295; 4_294_967_296 overflows.
        assert!(parse_ini_int::<u32, _>("4294967296").is_err());
    }

    #[test]
    fn int_negative_for_unsigned_is_error() {
        assert!(parse_ini_int::<u32, _>("-1").is_err());
    }

    #[test]
    fn int_zero() {
        let v: u32 = parse_ini_int("0").unwrap();
        assert_eq!(v, 0);
    }

    #[test]
    fn int_whitespace_trimmed() {
        let v: u32 = parse_ini_int("  42  ").unwrap();
        assert_eq!(v, 42);
    }

    // ---- NEW TESTS: bool edge cases ----

    #[test]
    fn bool_mixed_case_true() {
        assert_eq!(parse_ini_bool("tRuE").unwrap(), true);
        assert_eq!(parse_ini_bool("TrUe").unwrap(), true);
    }

    #[test]
    fn bool_mixed_case_false() {
        assert_eq!(parse_ini_bool("fAlSe").unwrap(), false);
        assert_eq!(parse_ini_bool("FaLsE").unwrap(), false);
    }

    #[test]
    fn bool_reject_on() {
        assert!(parse_ini_bool("on").is_err());
    }

    #[test]
    fn bool_reject_off() {
        assert!(parse_ini_bool("off").is_err());
    }

    #[test]
    fn bool_reject_partial_true() {
        assert!(parse_ini_bool("tru").is_err());
        assert!(parse_ini_bool("tr").is_err());
    }

    #[test]
    fn bool_reject_partial_false() {
        assert!(parse_ini_bool("fals").is_err());
        assert!(parse_ini_bool("fal").is_err());
    }

    #[test]
    fn bool_reject_numeric_other_than_0_1() {
        assert!(parse_ini_bool("2").is_err());
        assert!(parse_ini_bool("10").is_err());
        assert!(parse_ini_bool("-1").is_err());
    }

    #[test]
    fn bool_whitespace_trimmed() {
        // Leading/trailing spaces trimmed before matching
        assert_eq!(parse_ini_bool("  1  ").unwrap(), true);
        assert_eq!(parse_ini_bool("  false  ").unwrap(), false);
    }

    // ---- NEW TESTS: int with i64 ----

    #[test]
    fn int_i64_positive() {
        let v: i64 = parse_ini_int("9999999999").unwrap();
        assert_eq!(v, 9999999999i64);
    }

    #[test]
    fn int_i64_negative() {
        let v: i64 = parse_ini_int("-42").unwrap();
        assert_eq!(v, -42i64);
    }

    #[test]
    fn int_i64_negative_large() {
        let v: i64 = parse_ini_int("-9223372036854775808").unwrap();
        assert_eq!(v, i64::MIN);
    }

    #[test]
    fn int_i64_positive_leading_plus() {
        let v: i64 = parse_ini_int("+42").unwrap();
        assert_eq!(v, 42i64);
    }

    // ---- NEW TESTS: overflow ----

    #[test]
    fn int_u32_max_ok() {
        let v: u32 = parse_ini_int("4294967295").unwrap();
        assert_eq!(v, u32::MAX);
    }

    #[test]
    fn int_u32_overflow() {
        assert!(parse_ini_int::<u32, _>("4294967296").is_err());
    }

    #[test]
    fn int_i64_overflow() {
        assert!(parse_ini_int::<i64, _>("9223372036854775808").is_err());
    }

    // ---- NEW TESTS: special forms that should fail ----

    #[test]
    fn int_reject_underscore_separator() {
        // Rust allows 1_000 but our parser should not
        assert!(parse_ini_int::<u32, _>("1_000").is_err());
    }

    #[test]
    fn int_reject_float_format() {
        assert!(parse_ini_int::<u32, _>("1.0").is_err());
    }

    #[test]
    fn int_reject_leading_space_only() {
        // Trimmed; "42" is valid. But " " should fail.
        assert!(parse_ini_int::<u32, _>(" ").is_err());
    }

    #[test]
    fn int_zero_with_plus() {
        let v: u32 = parse_ini_int("+0").unwrap();
        assert_eq!(v, 0);
    }

    #[test]
    fn int_u8_max_ok() {
        let v: u8 = parse_ini_int("255").unwrap();
        assert_eq!(v, 255);
    }

    #[test]
    fn int_u8_overflow() {
        assert!(parse_ini_int::<u8, _>("256").is_err());
    }

    #[test]
    fn int_reject_hex_uppercase() {
        assert!(parse_ini_int::<u32, _>("0XFF").is_err());
    }

    #[test]
    fn int_reject_octal_uppercase() {
        assert!(parse_ini_int::<u32, _>("0O17").is_err());
    }

    #[test]
    fn int_reject_binary_uppercase() {
        assert!(parse_ini_int::<u32, _>("0B1010").is_err());
    }

    #[test]
    fn int_reject_alphabetic() {
        assert!(parse_ini_int::<u32, _>("abc").is_err());
    }
}
