//! Custom `serde::Deserializer` over a `RawSection`.
//!
//! Two public entrypoints:
//! - `from_kv_section` — deserialize a struct from the key-value pairs in a section.
//! - `from_bare_lines` — deserialize a `Vec<T>` from the bare-value lines in a section.
//!
//! Design notes:
//! - Unknown keys are silently skipped (lenient INI mode).
//! - `deserialize_bool` uses `grammar::parse_ini_bool`.
//! - `deserialize_i*/u*` use `grammar::parse_ini_int`.
//! - `deserialize_str` / `deserialize_string` return the raw value string.
//! - `deserialize_option`: `None` if key absent from map, else `Some(inner)`.
//! - Unsupported methods return an error.

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use crate::error::ConfigError;
use super::grammar::{parse_ini_bool, parse_ini_int};
use super::raw::{RawLineKind, RawSection};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Deserialize a struct `T` from the key-value pairs in `raw`.
/// Missing fields use their `Default` (requires `#[serde(default)]` on the struct).
/// Unknown keys are silently ignored.
pub(super) fn from_kv_section<T: DeserializeOwned>(raw: &RawSection) -> Result<T, ConfigError> {
    let de = KvDeserializer::new(raw);
    T::deserialize(de)
}

/// Deserialize a collection `T` (usually `Vec<Item>`) from the bare-value lines in `raw`.
/// `KeyValue` lines are ignored.
pub(super) fn from_bare_lines<T: DeserializeOwned>(raw: &RawSection) -> Result<T, ConfigError> {
    let de = BareDeserializer::new(raw);
    T::deserialize(de)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cfg_err(msg: impl Into<String>) -> ConfigError {
    ConfigError::grammar("serde", "", msg.into())
}

// ---------------------------------------------------------------------------
// KvDeserializer — views the section as a map
// ---------------------------------------------------------------------------

struct KvDeserializer<'a> {
    /// All kv pairs in source order (for the `MapAccess` impl).
    pairs: Vec<(&'a str, &'a str)>,
}

impl<'a> KvDeserializer<'a> {
    fn new(raw: &'a RawSection) -> Self {
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        for line in &raw.lines {
            if let RawLineKind::KeyValue { key, value } = &line.kind {
                pairs.push((key.as_str(), value.as_str()));
            }
        }
        KvDeserializer { pairs }
    }
}

impl<'de, 'a: 'de> de::Deserializer<'de> for KvDeserializer<'a> {
    type Error = ConfigError;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(cfg_err("deserialize_any not supported for kv sections"))
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_map(KvMapAccess {
            pairs: self.pairs,
            pos: 0,
            current_value: None,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_map(visitor)
    }

    // For Option<T> at the top-level kv section level — unlikely but handle gracefully.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    // Everything else: unsupported at the top level of a kv deserializer.
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf seq tuple tuple_struct enum identifier ignored_any
    }
}

// ---------------------------------------------------------------------------
// KvMapAccess — iterates key-value pairs
// ---------------------------------------------------------------------------

struct KvMapAccess<'a> {
    pairs: Vec<(&'a str, &'a str)>,
    pos: usize,
    current_value: Option<&'a str>,
}

impl<'de, 'a: 'de> MapAccess<'de> for KvMapAccess<'a> {
    type Error = ConfigError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        if self.pos >= self.pairs.len() {
            return Ok(None);
        }
        let (key, value) = self.pairs[self.pos];
        self.current_value = Some(value);
        self.pos += 1;
        seed.deserialize(StrDeserializer(key)).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        let val = self.current_value.take().ok_or_else(|| cfg_err("value called before key"))?;
        seed.deserialize(ValueDeserializer(val))
    }
}

// ---------------------------------------------------------------------------
// ValueDeserializer — deserializes a single string value into a typed field
// ---------------------------------------------------------------------------

struct ValueDeserializer<'a>(&'a str);

impl<'de, 'a: 'de> de::Deserializer<'de> for ValueDeserializer<'a> {
    type Error = ConfigError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_bool(parse_ini_bool(self.0)?)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i8(parse_ini_int(self.0)?)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i16(parse_ini_int(self.0)?)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i32(parse_ini_int(self.0)?)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i64(parse_ini_int(self.0)?)
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i128(parse_ini_int(self.0)?)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u8(parse_ini_int(self.0)?)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u16(parse_ini_int(self.0)?)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u32(parse_ini_int(self.0)?)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u64(parse_ini_int(self.0)?)
    }

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u128(parse_ini_int(self.0)?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v: f32 = self.0.trim().parse().map_err(|_| {
            ConfigError::grammar("f32", self.0, "invalid float")
        })?;
        visitor.visit_f32(v)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let v: f64 = self.0.trim().parse().map_err(|_| {
            ConfigError::grammar("f64", self.0, "invalid float")
        })?;
        visitor.visit_f64(v)
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let mut chars = self.0.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => visitor.visit_char(c),
            _ => Err(cfg_err("expected single char")),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.0.to_owned())
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_bytes(self.0.as_bytes())
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_byte_buf(self.0.as_bytes().to_vec())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // If we're here the value is present, so it's Some.
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        // A single value as a one-element sequence.
        visitor.visit_seq(SingleValueSeq { value: Some(self.0) })
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(cfg_err("deserialize_map not supported for a scalar value"))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(cfg_err("deserialize_struct not supported for a scalar value"))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        // Deserialize as a unit variant identified by string.
        visitor.visit_enum(EnumStrAccess(self.0))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_unit()
    }
}

// ---------------------------------------------------------------------------
// EnumStrAccess — for unit enum variants deserialized from a string
// ---------------------------------------------------------------------------

struct EnumStrAccess<'a>(&'a str);

impl<'de, 'a: 'de> de::EnumAccess<'de> for EnumStrAccess<'a> {
    type Error = ConfigError;
    type Variant = UnitVariantAccess;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let val = seed.deserialize(StrDeserializer(self.0))?;
        Ok((val, UnitVariantAccess))
    }
}

struct UnitVariantAccess;

impl<'de> de::VariantAccess<'de> for UnitVariantAccess {
    type Error = ConfigError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        _seed: T,
    ) -> Result<T::Value, Self::Error> {
        Err(cfg_err("newtype variant not supported"))
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(cfg_err("tuple variant not supported"))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        Err(cfg_err("struct variant not supported"))
    }
}

// ---------------------------------------------------------------------------
// SingleValueSeq — wraps one string as a one-element sequence
// ---------------------------------------------------------------------------

struct SingleValueSeq<'a> {
    value: Option<&'a str>,
}

impl<'de, 'a: 'de> SeqAccess<'de> for SingleValueSeq<'a> {
    type Error = ConfigError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        match self.value.take() {
            None => Ok(None),
            Some(v) => seed.deserialize(ValueDeserializer(v)).map(Some),
        }
    }
}

// ---------------------------------------------------------------------------
// StrDeserializer — maps a bare &str to a serde visit_str call
// ---------------------------------------------------------------------------

struct StrDeserializer<'a>(&'a str);

impl<'de, 'a: 'de> de::Deserializer<'de> for StrDeserializer<'a> {
    type Error = ConfigError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_string(self.0.to_owned())
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_str(self.0)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum ignored_any
    }
}

// ---------------------------------------------------------------------------
// BareDeserializer — views the section as a sequence of bare-value lines
// ---------------------------------------------------------------------------

struct BareDeserializer<'a> {
    lines: Vec<&'a str>,
}

impl<'a> BareDeserializer<'a> {
    fn new(raw: &'a RawSection) -> Self {
        let lines = raw
            .lines
            .iter()
            .filter_map(|l| {
                if let RawLineKind::BareValue(v) = &l.kind {
                    Some(v.as_str())
                } else {
                    None
                }
            })
            .collect();
        BareDeserializer { lines }
    }
}

impl<'de, 'a: 'de> de::Deserializer<'de> for BareDeserializer<'a> {
    type Error = ConfigError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_seq(BareSeqAccess {
            lines: self.lines,
            pos: 0,
        })
    }

    // A `from_bare_lines` on a single-element section can be used to get the
    // first bare value as a string — but typically callers call it for Vec<T>.
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.lines.first() {
            Some(&s) => visitor.visit_string(s.to_owned()),
            None => Err(cfg_err("expected at least one bare-value line")),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.lines.first() {
            Some(&s) => visitor.visit_str(s),
            None => Err(cfg_err("expected at least one bare-value line")),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.lines.is_empty() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char
        bytes byte_buf unit unit_struct newtype_struct tuple tuple_struct
        map struct enum identifier ignored_any
    }
}

// ---------------------------------------------------------------------------
// BareSeqAccess — iterate over bare-value lines
// ---------------------------------------------------------------------------

struct BareSeqAccess<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'de, 'a: 'de> SeqAccess<'de> for BareSeqAccess<'a> {
    type Error = ConfigError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Self::Error> {
        if self.pos >= self.lines.len() {
            return Ok(None);
        }
        let val = self.lines[self.pos];
        self.pos += 1;
        seed.deserialize(ValueDeserializer(val)).map(Some)
    }
}

// ---------------------------------------------------------------------------
// impl serde::de::Error for ConfigError
// ---------------------------------------------------------------------------

impl de::Error for ConfigError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        ConfigError::grammar("serde", "", msg.to_string())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SourceSpan;
    use crate::ini::raw::{RawLine, RawLineKind, RawSection};
    use serde::Deserialize;

    fn make_span(line: u32) -> SourceSpan {
        SourceSpan { line, col_start: 1, col_end: 10 }
    }

    fn make_kv_line(key: &str, value: &str, line: u32) -> RawLine {
        RawLine {
            kind: RawLineKind::KeyValue { key: key.to_owned(), value: value.to_owned() },
            span: make_span(line),
            had_trailing_comment: false,
        }
    }

    fn make_bare_line(value: &str, line: u32) -> RawLine {
        RawLine {
            kind: RawLineKind::BareValue(value.to_owned()),
            span: make_span(line),
            had_trailing_comment: false,
        }
    }

    fn make_section(name: &str, lines: Vec<RawLine>) -> RawSection {
        RawSection {
            name: name.to_owned(),
            lines,
            span: make_span(1),
        }
    }

    // ---- from_kv_section tests ----

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(default)]
    struct SimpleConfig {
        enabled: bool,
        count: u32,
        label: Option<String>,
    }

    impl Default for SimpleConfig {
        fn default() -> Self {
            SimpleConfig { enabled: false, count: 0, label: None }
        }
    }

    #[test]
    fn kv_section_happy_path() {
        let sec = make_section("test", vec![
            make_kv_line("enabled", "true", 1),
            make_kv_line("count", "42", 2),
            make_kv_line("label", "hello", 3),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.enabled, true);
        assert_eq!(cfg.count, 42);
        assert_eq!(cfg.label, Some("hello".to_owned()));
    }

    #[test]
    fn kv_section_bool_uses_ini_grammar() {
        // "1" and "0" should be accepted as booleans
        let sec = make_section("test", vec![
            make_kv_line("enabled", "1", 1),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.enabled, true);

        let sec2 = make_section("test", vec![
            make_kv_line("enabled", "0", 1),
        ]);
        let cfg2: SimpleConfig = from_kv_section(&sec2).unwrap();
        assert_eq!(cfg2.enabled, false);
    }

    #[test]
    fn kv_section_unknown_key_silently_dropped() {
        // Unknown key should not cause error in lenient mode
        let sec = make_section("test", vec![
            make_kv_line("enabled", "true", 1),
            make_kv_line("totally_unknown_key", "value", 2),
            make_kv_line("count", "5", 3),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.enabled, true);
        assert_eq!(cfg.count, 5);
    }

    #[test]
    fn kv_section_missing_optional_field_is_none() {
        let sec = make_section("test", vec![
            make_kv_line("enabled", "false", 1),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.label, None);
    }

    #[test]
    fn kv_section_missing_required_with_default() {
        // With #[serde(default)] an empty section gets defaults
        let sec = make_section("test", vec![]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.enabled, false);
        assert_eq!(cfg.count, 0);
        assert_eq!(cfg.label, None);
    }

    #[test]
    fn kv_section_bare_lines_ignored() {
        // Bare value lines should be ignored by kv deserialization
        let sec = make_section("test", vec![
            make_bare_line("some_bare_line", 1),
            make_kv_line("count", "7", 2),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.count, 7);
    }

    #[test]
    fn kv_section_string_field() {
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(default)]
        struct WithString {
            #[serde(default)]
            name: String,
        }
        impl Default for WithString {
            fn default() -> Self { WithString { name: String::new() } }
        }

        let sec = make_section("test", vec![
            make_kv_line("name", "some value", 1),
        ]);
        let cfg: WithString = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.name, "some value");
    }

    #[test]
    fn kv_section_u32_from_string() {
        let sec = make_section("test", vec![
            make_kv_line("count", "123", 1),
        ]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.count, 123);
    }

    // ---- from_bare_lines tests ----

    #[test]
    fn bare_lines_produces_string_vec() {
        let sec = make_section("ips", vec![
            make_bare_line("r.ripple.com 51235", 1),
            make_bare_line("altnet.ripple.com 51235", 2),
        ]);
        let lines: Vec<String> = from_bare_lines(&sec).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "r.ripple.com 51235");
        assert_eq!(lines[1], "altnet.ripple.com 51235");
    }

    #[test]
    fn bare_lines_ignores_kv_lines() {
        let sec = make_section("test", vec![
            make_bare_line("bare1", 1),
            make_kv_line("key", "value", 2),
            make_bare_line("bare2", 3),
        ]);
        let lines: Vec<String> = from_bare_lines(&sec).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "bare1");
        assert_eq!(lines[1], "bare2");
    }

    #[test]
    fn bare_lines_empty_section_returns_empty_vec() {
        let sec = make_section("test", vec![]);
        let lines: Vec<String> = from_bare_lines(&sec).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn bare_lines_only_kv_returns_empty_vec() {
        let sec = make_section("test", vec![
            make_kv_line("key", "value", 1),
        ]);
        let lines: Vec<String> = from_bare_lines(&sec).unwrap();
        assert!(lines.is_empty());
    }

    // ---- ValueDeserializer tests (via kv_section) ----

    #[test]
    fn value_deserializer_bool_true_variants() {
        for val in &["1", "true", "True", "TRUE"] {
            let sec = make_section("t", vec![make_kv_line("enabled", val, 1)]);
            let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
            assert_eq!(cfg.enabled, true, "failed for value {}", val);
        }
    }

    #[test]
    fn value_deserializer_bool_false_variants() {
        for val in &["0", "false", "False", "FALSE"] {
            let sec = make_section("t", vec![make_kv_line("enabled", val, 1)]);
            let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
            assert_eq!(cfg.enabled, false, "failed for value {}", val);
        }
    }

    #[test]
    fn value_deserializer_invalid_bool_returns_error() {
        let sec = make_section("t", vec![make_kv_line("enabled", "yes", 1)]);
        let result: Result<SimpleConfig, _> = from_kv_section(&sec);
        assert!(result.is_err());
    }

    #[test]
    fn value_deserializer_option_some_when_present() {
        let sec = make_section("t", vec![make_kv_line("label", "hello", 1)]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.label, Some("hello".to_owned()));
    }

    #[test]
    fn value_deserializer_option_none_when_absent() {
        let sec = make_section("t", vec![]);
        let cfg: SimpleConfig = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.label, None);
    }

    // ---- Enum deserialization ----

    #[test]
    fn value_deserializer_enum_unit_variant() {
        #[derive(Debug, Deserialize, PartialEq)]
        enum MyEnum { Alpha, Beta, Gamma }

        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(default)]
        struct WithEnum {
            #[serde(default)]
            mode: Option<MyEnum>,
        }
        impl Default for WithEnum { fn default() -> Self { WithEnum { mode: None } } }

        let sec = make_section("t", vec![make_kv_line("mode", "Alpha", 1)]);
        let cfg: WithEnum = from_kv_section(&sec).unwrap();
        assert_eq!(cfg.mode, Some(MyEnum::Alpha));
    }
}
