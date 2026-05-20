//! Raw intermediate representation produced by the INI lexer.
//!
//! `RawSections` is the output of Stage 1 (lexing).  Stage 2 (`adapt`) walks
//! the sections and converts them into the typed `Config` fields.

use std::collections::HashMap;
use crate::error::SourceSpan;

/// What kind of content a line carries.
#[derive(Debug, Clone, PartialEq)]
pub enum RawLineKind {
    /// `key = value` — both sides trimmed.
    KeyValue { key: String, value: String },
    /// Anything that didn't match the key-value regex; stored verbatim (trimmed).
    BareValue(String),
}

/// A single logical line inside a section, with source location.
#[derive(Debug, Clone)]
pub struct RawLine {
    pub kind: RawLineKind,
    pub span: SourceSpan,
    /// True if the original line had a trailing `#…` comment that was stripped.
    pub had_trailing_comment: bool,
}

/// One INI section (`[name]` … next `[…]`).
/// When the same name appears twice the lexer concatenates their lines.
#[derive(Debug, Clone)]
pub struct RawSection {
    pub name: String,
    pub lines: Vec<RawLine>,
    /// Span of the *first* `[name]` header that created this section.
    pub span: SourceSpan,
}

impl RawSection {
    /// Build a last-write-wins key→value map from all `KeyValue` lines.
    /// Useful for Category-1 sections that need look-up access outside serde.
    pub fn lookup(&self) -> HashMap<&str, &str> {
        let mut map = HashMap::new();
        for line in &self.lines {
            if let RawLineKind::KeyValue { key, value } = &line.kind {
                map.insert(key.as_str(), value.as_str());
            }
        }
        map
    }
}

/// The full bag of raw sections produced by the lexer.
#[derive(Debug, Default)]
pub struct RawSections {
    /// Sections in source order.
    pub sections: Vec<RawSection>,
    /// Maps section name (verbatim, case-sensitive per design §7 #4) → indices into `sections`.
    by_name: HashMap<String, Vec<usize>>,
}

impl RawSections {
    /// Build (or rebuild) the lookup index.  Called once after lexing finishes.
    pub fn build_index(&mut self) {
        self.by_name.clear();
        for (i, sec) in self.sections.iter().enumerate() {
            self.by_name
                .entry(sec.name.clone())
                .or_default()
                .push(i);
        }
    }

    /// Iterate over all sections whose name exactly matches `name` (case-sensitive).
    /// Mis-cased section names will not match and will silently fall through to the
    /// unknown-section arm in the adapter, matching C++ BasicConfig behavior.
    pub fn sections_named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a RawSection> {
        let indices = self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[]);
        indices.iter().map(move |&i| &self.sections[i])
    }

    /// Return the first section matching `name` (case-sensitive), if any.
    pub fn first_named(&self, name: &str) -> Option<&RawSection> {
        self.sections_named(name).next()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SourceSpan;

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

    // ---- RawSection::lookup tests ----

    #[test]
    fn lookup_returns_all_kv_pairs() {
        let sec = make_section("test", vec![
            make_kv_line("foo", "1", 1),
            make_kv_line("bar", "2", 2),
        ]);
        let map = sec.lookup();
        assert_eq!(map.get("foo"), Some(&"1"));
        assert_eq!(map.get("bar"), Some(&"2"));
    }

    #[test]
    fn lookup_ignores_bare_lines() {
        let sec = make_section("test", vec![
            make_kv_line("foo", "1", 1),
            make_bare_line("some_bare_value", 2),
        ]);
        let map = sec.lookup();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("foo"), Some(&"1"));
    }

    #[test]
    fn lookup_last_write_wins_for_duplicate_keys() {
        let sec = make_section("test", vec![
            make_kv_line("foo", "first", 1),
            make_kv_line("foo", "second", 2),
            make_kv_line("foo", "third", 3),
        ]);
        let map = sec.lookup();
        // Last occurrence wins
        assert_eq!(map.get("foo"), Some(&"third"));
    }

    #[test]
    fn lookup_empty_section_returns_empty_map() {
        let sec = make_section("test", vec![]);
        let map = sec.lookup();
        assert!(map.is_empty());
    }

    #[test]
    fn lookup_only_bare_lines_returns_empty_map() {
        let sec = make_section("test", vec![
            make_bare_line("bare1", 1),
            make_bare_line("bare2", 2),
        ]);
        let map = sec.lookup();
        assert!(map.is_empty());
    }

    // ---- RawSections::sections_named tests ----

    #[test]
    fn sections_named_returns_matching_sections_in_order() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("alpha", vec![]));
        rs.sections.push(make_section("beta", vec![]));
        rs.sections.push(make_section("alpha", vec![]));
        rs.build_index();

        let names: Vec<_> = rs.sections_named("alpha").map(|s| s.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        // Both should be "alpha"
        assert!(names.iter().all(|&n| n == "alpha"));
    }

    #[test]
    fn sections_named_case_sensitive_lookup() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("overlay", vec![]));
        rs.build_index();

        // Lookup is case-sensitive per design §7 #4. Only exact match returns results.
        assert_eq!(rs.sections_named("OVERLAY").count(), 0);
        assert_eq!(rs.sections_named("Overlay").count(), 0);
        assert_eq!(rs.sections_named("overlay").count(), 1);
    }

    #[test]
    fn sections_named_missing_returns_empty_iterator() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("alpha", vec![]));
        rs.build_index();

        assert_eq!(rs.sections_named("nonexistent").count(), 0);
    }

    #[test]
    fn first_named_returns_first_matching_section() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("alpha", vec![make_kv_line("id", "first", 1)]));
        rs.sections.push(make_section("beta", vec![]));
        rs.sections.push(make_section("alpha", vec![make_kv_line("id", "second", 3)]));
        rs.build_index();

        let sec = rs.first_named("alpha").unwrap();
        // Should be the first occurrence
        let map = sec.lookup();
        assert_eq!(map.get("id"), Some(&"first"));
    }

    #[test]
    fn first_named_returns_none_for_missing() {
        let mut rs = RawSections::default();
        rs.build_index();
        assert!(rs.first_named("anything").is_none());
    }

    // ---- RawSections::build_index tests ----

    #[test]
    fn build_index_covers_all_sections() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("alpha", vec![]));
        rs.sections.push(make_section("beta", vec![]));
        rs.sections.push(make_section("gamma", vec![]));
        rs.build_index();

        assert_eq!(rs.sections_named("alpha").count(), 1);
        assert_eq!(rs.sections_named("beta").count(), 1);
        assert_eq!(rs.sections_named("gamma").count(), 1);
    }

    #[test]
    fn build_index_clears_and_rebuilds() {
        let mut rs = RawSections::default();
        rs.sections.push(make_section("alpha", vec![]));
        rs.build_index();
        // Add another section and rebuild
        rs.sections.push(make_section("beta", vec![]));
        rs.build_index();

        assert_eq!(rs.sections_named("beta").count(), 1);
    }

    // ---- RawLineKind equality ----

    #[test]
    fn rawlinekind_equality() {
        let kv1 = RawLineKind::KeyValue { key: "foo".to_owned(), value: "bar".to_owned() };
        let kv2 = RawLineKind::KeyValue { key: "foo".to_owned(), value: "bar".to_owned() };
        let kv3 = RawLineKind::KeyValue { key: "foo".to_owned(), value: "baz".to_owned() };
        let bv = RawLineKind::BareValue("bare".to_owned());

        assert_eq!(kv1, kv2);
        assert_ne!(kv1, kv3);
        assert_ne!(kv1, bv);
    }
}
