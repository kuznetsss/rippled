//! Stage-1 INI lexer: normalise text → `RawSections`.
//!
//! Rules (matching design §5 / analysis §1.2–§1.4):
//! 1. Normalise `\r\n` and `\r` → `\n`.
//! 2. Drop blank lines.
//! 3. Drop whole-line `#` comments (first non-whitespace char is `#`).
//! 4. Strip trailing `#…` comments; honour `\#` escape.
//! 5. Detect `[name]` headers.  Lines without a closing `]` reuse the current section.
//! 6. Key regex `[a-zA-Z][_a-zA-Z0-9]*\s*=\s*(.+\S)` → `KeyValue`; else → `BareValue`.
//! 7. Two `[name]` headers concatenate their lines.

use crate::error::{ConfigError, SourceSpan};
use super::raw::{RawLine, RawLineKind, RawSection, RawSections};

/// Tokenise the full INI text.  Returns a `RawSections` with the index already built.
pub(super) fn tokenize(text: &str) -> Result<RawSections, ConfigError> {
    // Step 1: normalise line endings.
    let normalised: String = text.replace("\r\n", "\n").replace('\r', "\n");

    let mut result = RawSections::default();
    // Lines that appear before any section header go into a synthetic "__preamble__" bucket
    // that the adapter ignores (design §5: "lines without `]` reuse the current section").
    // We push a placeholder so we always have somewhere to put lines.
    let mut current_idx: Option<usize> = None;

    for (zero_line, raw_line) in normalised.lines().enumerate() {
        let line_no = (zero_line + 1) as u32;
        let span = SourceSpan { line: line_no, col_start: 1, col_end: raw_line.len() as u32 + 1 };

        // Step 2: drop blank lines.
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Step 3: drop whole-line comments.
        if trimmed.starts_with('#') {
            continue;
        }

        // Step 5: section headers.
        if let Some(header_content) = try_parse_header(trimmed) {
            let name_lower = header_content.to_lowercase();
            // Find an existing section with this name (step 7: concatenate).
            if let Some(existing) = result.sections.iter().position(|s| s.name == name_lower) {
                current_idx = Some(existing);
            } else {
                current_idx = Some(result.sections.len());
                result.sections.push(RawSection {
                    name: name_lower,
                    lines: Vec::new(),
                    span,
                });
            }
            continue;
        }

        // Step 4: strip trailing comments, honouring `\#`.
        let (content, had_comment) = strip_trailing_comment(raw_line);
        let content = content.trim().to_owned();

        if content.is_empty() {
            continue;
        }

        // Step 6: classify as KeyValue or BareValue.
        let kind = classify_line(&content);

        let rl = RawLine { kind, span: span.clone(), had_trailing_comment: had_comment };

        match current_idx {
            Some(idx) => result.sections[idx].lines.push(rl),
            None => {
                // Lines before the first section header: create a preamble section.
                result.sections.push(RawSection {
                    name: String::from("__preamble__"),
                    lines: vec![rl],
                    span,
                });
                // Don't set current_idx; the preamble is not a real header,
                // so subsequent non-header lines also go to preamble.
                // We need to set current_idx to this section so next lines go here.
                current_idx = Some(result.sections.len() - 1);
            }
        }
    }

    result.build_index();
    Ok(result)
}

/// If the trimmed line is a `[name]` header, return the name (without brackets).
/// We require the closing `]` to be present (design §5 rule 5).
fn try_parse_header(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('[') {
        return None;
    }
    let close = trimmed.find(']')?;
    let name = trimmed[1..close].trim();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Strip a trailing `#` comment from a raw line.
/// Returns `(content_without_comment, had_comment)`.
///
/// The `\#` escape keeps the `#` in the content (backslash is removed).
fn strip_trailing_comment(line: &str) -> (&str, bool) {
    // Walk through the line looking for an unescaped `#`.
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            if i > 0 && bytes[i - 1] == b'\\' {
                // Escaped — we'll handle the backslash removal after finding the end.
                i += 1;
                continue;
            }
            // Unescaped `#` → everything from here is a comment.
            return (&line[..i], true);
        }
        i += 1;
    }
    (line, false)
}

/// Remove `\#` escape sequences from a string that has already had its trailing
/// comment stripped.  Replaces `\#` with `#`.
fn unescape_hash(s: &str) -> String {
    s.replace("\\#", "#")
}

/// Classify a (trimmed, comment-stripped) content line as `KeyValue` or `BareValue`.
///
/// Key regex: `[a-zA-Z][_a-zA-Z0-9]*\s*=\s*(.+\S)`.
/// Note: `key=` (empty value) → `BareValue("key=")` per analysis §6.11.
fn classify_line(content: &str) -> RawLineKind {
    // Fast path: must contain `=` somewhere after a valid identifier start.
    if let Some(eq_pos) = find_kv_eq(content) {
        let key_raw = content[..eq_pos].trim_end();
        let value_raw = content[eq_pos + 1..].trim_start();

        if is_valid_key(key_raw) && !value_raw.is_empty() {
            let value = unescape_hash(value_raw.trim_end());
            return RawLineKind::KeyValue {
                key: key_raw.to_owned(),
                value,
            };
        }
    }
    // Everything else (including `key=` with empty value) is a bare value.
    RawLineKind::BareValue(unescape_hash(content))
}

/// Find the position of the `=` that separates key and value.
/// Returns `None` if there is no `=`.
fn find_kv_eq(s: &str) -> Option<usize> {
    s.find('=')
}

/// Returns `true` iff `s` matches `[a-zA-Z][_a-zA-Z0-9]*`.
fn is_valid_key(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::raw::RawLineKind;

    fn lex(text: &str) -> RawSections {
        tokenize(text).expect("tokenize failed")
    }

    #[test]
    fn simple_section_kv() {
        let rs = lex("[overlay]\nmax_unknown_time=600\nmax_diverged_time=300\n");
        assert_eq!(rs.sections.len(), 1);
        let sec = &rs.sections[0];
        assert_eq!(sec.name, "overlay");
        assert_eq!(sec.lines.len(), 2);
        assert!(matches!(&sec.lines[0].kind, RawLineKind::KeyValue { key, value } if key == "max_unknown_time" && value == "600"));
        assert!(matches!(&sec.lines[1].kind, RawLineKind::KeyValue { key, value } if key == "max_diverged_time" && value == "300"));
    }

    #[test]
    fn bare_value_lines() {
        let rs = lex("[ips]\nr.ripple.com 51235\naltnet.ripple.com 51235\n");
        let sec = &rs.sections[0];
        assert_eq!(sec.lines.len(), 2);
        assert!(matches!(&sec.lines[0].kind, RawLineKind::BareValue(v) if v == "r.ripple.com 51235"));
    }

    #[test]
    fn whole_line_comment_dropped() {
        let rs = lex("[test]\n# this is a comment\nfoo=bar\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
    }

    #[test]
    fn trailing_comment_stripped() {
        let rs = lex("[test]\nfoo=bar # trailing\n");
        let sec = &rs.sections[0];
        assert!(matches!(&sec.lines[0].kind, RawLineKind::KeyValue { value, .. } if value == "bar"));
        assert!(sec.lines[0].had_trailing_comment);
    }

    #[test]
    fn escaped_hash_preserved() {
        let rs = lex("[test]\nfoo=bar\\#baz\n");
        let sec = &rs.sections[0];
        assert!(matches!(&sec.lines[0].kind, RawLineKind::KeyValue { value, .. } if value == "bar#baz"));
    }

    #[test]
    fn duplicate_section_concatenated() {
        let rs = lex("[validators]\nkey1\n[validators]\nkey2\n");
        assert_eq!(rs.sections.len(), 1);
        assert_eq!(rs.sections[0].lines.len(), 2);
    }

    #[test]
    fn blank_lines_dropped() {
        let rs = lex("[overlay]\n\n\nmax_unknown_time=600\n\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
    }

    #[test]
    fn crlf_normalised() {
        let rs = lex("[overlay]\r\nmax_unknown_time=600\r\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
    }

    #[test]
    fn empty_value_is_bare() {
        // `key=` with nothing after `=` should be BareValue
        let rs = lex("[test]\nfoo=\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(_)));
    }

    #[test]
    fn sections_named_lookup() {
        let rs = lex("[alpha]\nfoo=1\n[beta]\nbar=2\n");
        let names: Vec<_> = rs.sections_named("alpha").collect();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name, "alpha");
    }

    #[test]
    fn section_header_case_insensitive() {
        let rs = lex("[Overlay]\nmax_unknown_time=600\n");
        assert_eq!(rs.sections[0].name, "overlay");
    }

    // ---- NEW TESTS: line ending normalisation ----

    #[test]
    fn cr_only_normalised() {
        // Old-style Mac \r line endings should work like \n
        let rs = lex("[overlay]\rmax_unknown_time=600\r");
        assert_eq!(rs.sections.len(), 1);
        assert_eq!(rs.sections[0].lines.len(), 1);
        assert!(matches!(&rs.sections[0].lines[0].kind,
            RawLineKind::KeyValue { key, value } if key == "max_unknown_time" && value == "600"));
    }

    #[test]
    fn crlf_section_header_works() {
        let rs = lex("[overlay]\r\nfoo=bar\r\n");
        assert_eq!(rs.sections[0].name, "overlay");
        assert_eq!(rs.sections[0].lines.len(), 1);
        assert!(matches!(&rs.sections[0].lines[0].kind,
            RawLineKind::KeyValue { key, value } if key == "foo" && value == "bar"));
    }

    #[test]
    fn mixed_line_endings() {
        // Mix of \n, \r\n, \r in one document
        let rs = lex("[a]\nfoo=1\r\nbar=2\rbaz=3\n");
        assert_eq!(rs.sections[0].lines.len(), 3);
    }

    // ---- NEW TESTS: comment handling ----

    #[test]
    fn whole_line_comment_with_leading_spaces() {
        // Leading whitespace before # → still a comment
        let rs = lex("[test]\n   # this is still a comment\nfoo=bar\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::KeyValue { key, .. } if key == "foo"));
    }

    #[test]
    fn trailing_comment_no_space() {
        // # immediately after value (no space)
        let rs = lex("[test]\nfoo=bar#comment\n");
        let sec = &rs.sections[0];
        assert!(matches!(&sec.lines[0].kind, RawLineKind::KeyValue { value, .. } if value == "bar"));
        assert!(sec.lines[0].had_trailing_comment);
    }

    #[test]
    fn escaped_hash_in_bare_value() {
        // BareValue with \# should become bare value with # in it
        let rs = lex("[test]\nsome\\#value\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(v) if v == "some#value"));
    }

    #[test]
    fn escaped_hash_mid_value_then_real_comment() {
        // value\#kept # dropped
        let rs = lex("[test]\nfoo=val\\#kept # dropped\n");
        let sec = &rs.sections[0];
        // The \# is an escape, so # is kept; then there's another # which starts the comment
        // strip_trailing_comment finds '#' at position of "val\#kept " -- wait, the \ before # skips,
        // so the comment starts at the second #.
        assert!(matches!(&sec.lines[0].kind, RawLineKind::KeyValue { value, .. } if value == "val#kept"));
        assert!(sec.lines[0].had_trailing_comment);
    }

    #[test]
    fn no_trailing_comment_flag_when_none() {
        let rs = lex("[test]\nfoo=bar\n");
        assert!(!rs.sections[0].lines[0].had_trailing_comment);
    }

    // ---- NEW TESTS: section header rules ----

    #[test]
    fn section_without_closing_bracket_does_not_reset_section() {
        // Line without ] doesn't create a new section; goes into current
        let rs = lex("[overlay]\nmax_unknown_time=600\n[broken\nmax_diverged_time=300\n");
        // "[broken" has no ], so it's not a section header — becomes a bare line in overlay
        assert_eq!(rs.sections.len(), 1);
        assert_eq!(rs.sections[0].name, "overlay");
        assert_eq!(rs.sections[0].lines.len(), 3); // the kv, the "[broken" bare, the kv
    }

    #[test]
    fn section_header_empty_name_ignored() {
        // "[]" has empty name, not a valid header — treated as a bare line in current section
        let rs = lex("[overlay]\nfoo=1\n[]\nbar=2\n");
        // [] is not a valid header; "[]" becomes a bare line, bar=2 also goes to overlay
        // So overlay has 3 lines: foo=1, [], bar=2
        assert_eq!(rs.sections.len(), 1);
        assert_eq!(rs.sections[0].lines.len(), 3);
        // The [] line itself is a BareValue
        assert!(matches!(&rs.sections[0].lines[1].kind, RawLineKind::BareValue(v) if v == "[]"));
    }

    #[test]
    fn section_header_with_trailing_content() {
        // "[name] extra" — the find(']') is first ] so name is still parsed correctly
        let rs = lex("[overlay] some extra\nfoo=1\n");
        // try_parse_header finds first ], name is "overlay"
        assert_eq!(rs.sections[0].name, "overlay");
    }

    #[test]
    fn two_sections_same_name_concatenated() {
        let rs = lex("[validators]\nkey1\n[validators]\nkey2\n[validators]\nkey3\n");
        assert_eq!(rs.sections.len(), 1);
        assert_eq!(rs.sections[0].lines.len(), 3);
    }

    #[test]
    fn two_different_sections() {
        let rs = lex("[alpha]\nfoo=1\n[beta]\nbar=2\n");
        assert_eq!(rs.sections.len(), 2);
        assert_eq!(rs.sections[0].name, "alpha");
        assert_eq!(rs.sections[1].name, "beta");
    }

    #[test]
    fn section_header_span_line_number() {
        let rs = lex("[alpha]\nfoo=1\n");
        assert_eq!(rs.sections[0].span.line, 1);
    }

    // ---- NEW TESTS: key regex ----

    #[test]
    fn key_starting_with_underscore_is_bare() {
        // _foo=1 is not a valid key (must start with alpha)
        let rs = lex("[test]\n_foo=1\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(_)));
    }

    #[test]
    fn key_starting_with_digit_is_bare() {
        // 1foo=1 is not a valid key
        let rs = lex("[test]\n1foo=1\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(_)));
    }

    #[test]
    fn key_with_hyphen_is_bare() {
        // a-b=1 is not valid because hyphen is not in [_a-zA-Z0-9]
        let rs = lex("[test]\na-b=1\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(_)));
    }

    #[test]
    fn key_with_underscore_in_middle_is_valid() {
        // max_unknown_time is valid
        let rs = lex("[test]\nmax_unknown_time=600\n");
        assert!(matches!(&rs.sections[0].lines[0].kind,
            RawLineKind::KeyValue { key, value } if key == "max_unknown_time" && value == "600"));
    }

    #[test]
    fn key_single_alpha_char_is_valid() {
        let rs = lex("[test]\na=1\n");
        assert!(matches!(&rs.sections[0].lines[0].kind,
            RawLineKind::KeyValue { key, value } if key == "a" && value == "1"));
    }

    #[test]
    fn empty_value_is_bare_detail() {
        // foo= produces BareValue("foo=") per analysis §6.11
        let rs = lex("[test]\nfoo=\n");
        assert!(matches!(&rs.sections[0].lines[0].kind, RawLineKind::BareValue(v) if v == "foo="));
    }

    // ---- NEW TESTS: whitespace-only lines ----

    #[test]
    fn whitespace_only_line_ignored() {
        let rs = lex("[test]\n   \nfoo=bar\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
    }

    #[test]
    fn tab_only_line_ignored() {
        let rs = lex("[test]\n\t\nfoo=bar\n");
        assert_eq!(rs.sections[0].lines.len(), 1);
    }

    // ---- NEW TESTS: source spans ----

    #[test]
    fn line_numbers_are_one_based() {
        let rs = lex("[section]\nfoo=bar\n");
        // Section header is on line 1, foo=bar on line 2
        assert_eq!(rs.sections[0].span.line, 1);
        assert_eq!(rs.sections[0].lines[0].span.line, 2);
    }

    #[test]
    fn line_numbers_correct_after_blank_lines() {
        // Blank lines don't get processed but should still increment the counter
        let rs = lex("[section]\n\nfoo=bar\n");
        // foo=bar is on line 3 (section=1, blank=2, foo=3)
        assert_eq!(rs.sections[0].lines[0].span.line, 3);
    }

    #[test]
    fn line_numbers_correct_after_comments() {
        // Comment lines count toward line numbering
        let rs = lex("[section]\n# comment\nfoo=bar\n");
        assert_eq!(rs.sections[0].lines[0].span.line, 3);
    }

    // ---- NEW TESTS: preamble ----

    #[test]
    fn lines_before_first_section_create_preamble() {
        let rs = lex("loose_line=something\n[section]\nfoo=bar\n");
        // preamble is created for the first line
        assert_eq!(rs.sections.len(), 2);
        assert_eq!(rs.sections[0].name, "__preamble__");
        assert_eq!(rs.sections[1].name, "section");
    }

    #[test]
    fn multiple_preamble_lines_all_go_to_preamble() {
        let rs = lex("line1\nline2\n[section]\nfoo=bar\n");
        assert_eq!(rs.sections[0].name, "__preamble__");
        assert_eq!(rs.sections[0].lines.len(), 2);
    }

    // ---- NEW TESTS: by_name index ----

    #[test]
    fn by_name_index_covers_all_sections() {
        let rs = lex("[alpha]\nfoo=1\n[beta]\nbar=2\n[gamma]\nbaz=3\n");
        // sections_named should find each
        assert_eq!(rs.sections_named("alpha").count(), 1);
        assert_eq!(rs.sections_named("beta").count(), 1);
        assert_eq!(rs.sections_named("gamma").count(), 1);
        assert_eq!(rs.sections_named("delta").count(), 0);
    }

    #[test]
    fn first_named_returns_first_match() {
        let rs = lex("[alpha]\nfoo=1\n[beta]\nbar=2\n");
        assert!(rs.first_named("alpha").is_some());
        assert!(rs.first_named("missing").is_none());
    }

    #[test]
    fn empty_input_produces_no_sections() {
        let rs = lex("");
        assert_eq!(rs.sections.len(), 0);
    }

    #[test]
    fn only_comments_and_blanks_produces_no_sections() {
        let rs = lex("# comment\n\n# another\n   \n");
        assert_eq!(rs.sections.len(), 0);
    }
}
