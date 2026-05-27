//! Stage 1: INI tokenizer.
//!
//! A faithful port of C++ `parseIniFile` + `Section::append` from
//! `src/xrpld/core/detail/Config.cpp` and
//! `src/libxrpl/basics/BasicConfig.cpp`.

use std::collections::HashMap;

use regex::Regex;

/// Mirror of C++ `Section`.
#[derive(Debug, Default, Clone)]
pub struct Section {
    pub name: String,
    pub lookup: HashMap<String, String>,
    pub values: Vec<String>,
    pub lines: Vec<String>,
    pub had_trailing_comments: bool,
}

impl Section {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Append lines to the section.  Mirrors `Section::append` exactly:
    ///  - strip trailing comments (`#` unless preceded by `\`)
    ///  - skip empty results
    ///  - classify as kv-pair (inserted into `lookup`) or value (pushed into
    ///    `values`)
    ///  - everything non-empty ends up in `lines`
    pub fn append(&mut self, raw_lines: &[String]) {
        // Compiled once on first call (inside fn scope — Rust doesn't allow
        // static initialisation of non-const values without lazy_static, so we
        // use a local once-cell via std::sync::OnceLock).
        static KV_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = KV_RE.get_or_init(|| {
            Regex::new(
                r"(?x)^
                (?:\s*)                        # optional leading whitespace
                ([a-zA-Z][_a-zA-Z0-9]*)        # key
                (?:\s*)=(?:\s*)                # '='
                (.*\S+)                        # value (at least one non-space)
                (?:\s*)$",
            )
            .expect("KV_RE is valid")
        });

        for raw in raw_lines {
            let mut line = raw.clone();

            // ------------------------------------------------------------------
            // Trailing-comment stripping — mirrors the C++ removeComment lambda
            // in Section::append (BasicConfig.cpp lines 46-76).
            // ------------------------------------------------------------------
            let removed_trailing = remove_comment(&mut line);

            if removed_trailing && !line.is_empty() {
                self.had_trailing_comments = true;
            }

            if line.is_empty() {
                continue;
            }

            // Key/value pair?
            if let Some(caps) = re.captures(&line) {
                let key = caps[1].to_string();
                let val = caps[2].to_string();
                self.lookup.insert(key, val);
            } else {
                self.values.push(line.clone());
            }

            self.lines.push(line);
        }
    }
}

/// Mirrors the C++ `removeComment` lambda.
///
/// Scans for `#`. If the character before it is `\`, erase the `\` and keep
/// looking (escaped `#`).  Otherwise, truncate at `#` and trim trailing
/// whitespace.  Returns `true` if a real trailing comment was found (the
/// value was truncated).
///
/// Special case: if the first character is `#`, the entire value is a comment
/// and is cleared (`val = ""`). Returns `false` for the "entire value is
/// comment" case (C++ does not set `removedTrailing = true` there — it just
/// sets `val = ""` and breaks).
fn remove_comment(val: &mut String) -> bool {
    // Mirrors the C++ removeComment lambda in Section::append (BasicConfig.cpp
    // lines 46-76).
    //
    // C++ algorithm: scan for '#' starting from 0 each time (but the erase
    // shifts characters left, so the next `find('#', comment)` starts from
    // the original `comment` position which now points 1 past the erased-
    // and-kept '#').
    //
    // We replicate that with an explicit cursor.

    let mut search_from: usize = 0;
    loop {
        match val[search_from..].find('#') {
            None => return false,
            Some(rel) => {
                let pos = search_from + rel;
                if pos == 0 {
                    // Entire value is a comment.
                    val.clear();
                    return false;
                }
                let bytes = val.as_bytes();
                if bytes[pos - 1] == b'\\' {
                    // Erase the backslash at pos-1.
                    val.remove(pos - 1);
                    // The '#' is now at pos-1.  In the C++ code, after erasing,
                    // the loop calls `val.find('#', comment)` where `comment`
                    // was the old position of '#', i.e. `pos`.  After the erase
                    // the character at `pos-1` is '#', and the character at
                    // `pos` is whatever was after '#'.  So the next search from
                    // `pos` (= old pos) will skip past the now-literal '#'.
                    //
                    // In our string-slice terms: after the remove, the string is
                    // one byte shorter.  The '#' is now at pos-1.  We should
                    // continue searching from pos (which in the new string is
                    // 1 past the '#').
                    search_from = pos; // skip the literal '#' we just unescaped
                } else {
                    // Real trailing comment: truncate and trim.
                    val.truncate(pos);
                    let trimmed = val.trim_end().to_string();
                    *val = trimmed;
                    return true;
                }
            }
        }
    }
}

/// Map from section name → Section.  The default section has name `""`.
pub type BasicConfig = HashMap<String, Section>;

/// Parse an INI string into a `BasicConfig`.
///
/// Mirrors `parseIniFile` (Config.cpp lines 164-210) followed by the
/// `BasicConfig::build` call that runs `Section::append` on each section's
/// raw lines.
///
/// Step 1: `parseIniFile` — split into sections, collecting raw lines.
/// Step 2: For each section run `Section::append` on its raw lines.
pub fn parse_ini(s: &str) -> BasicConfig {
    // --- Step 1: normalize line endings ---
    // Replace CRLF → LF, then lone CR → LF.
    let normalized = s.replace("\r\n", "\n").replace('\r', "\n");

    let mut result: BasicConfig = HashMap::new();
    let mut current_section = String::new(); // default = ""

    // Ensure the default section exists.
    result
        .entry(current_section.clone())
        .or_insert_with(|| Section::new(""));

    // Track raw lines per section (before Section::append processing).
    // We use a separate map so we can run append after the full parse pass.
    let mut raw_lines: HashMap<String, Vec<String>> = HashMap::new();
    raw_lines.insert(current_section.clone(), Vec::new());

    for raw_line in normalized.split('\n') {
        // Trim leading and trailing whitespace (bTrim = true in C++).
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            // Blank line or comment — skip (mirrors parseIniFile).
            continue;
        }

        let bytes = trimmed.as_bytes();
        if bytes[0] == b'['
            && bytes[bytes.len() - 1] == b']'
        {
            // New section header.
            let name = &trimmed[1..trimmed.len() - 1];
            current_section = name.to_string();
            // Ensure section exists.
            result
                .entry(current_section.clone())
                .or_insert_with(|| Section::new(current_section.clone()));
            raw_lines
                .entry(current_section.clone())
                .or_default();
        } else {
            // Regular line — append to current section's raw lines.
            raw_lines
                .entry(current_section.clone())
                .or_default()
                .push(trimmed.to_string());
        }
    }

    // --- Step 2: run Section::append on each section's raw lines ---
    for (name, lines) in &raw_lines {
        let section = result
            .entry(name.clone())
            .or_insert_with(|| Section::new(name.clone()));
        section.append(lines);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to get a section by name (returns empty if not present).
    fn sec<'a>(bc: &'a BasicConfig, name: &str) -> &'a Section {
        bc.get(name).unwrap_or_else(|| {
            panic!("section [{name}] not found");
        })
    }

    // 1. Line-ending normalization (\r\n, \r, mixed).
    #[test]
    fn line_ending_crlf() {
        let bc = parse_ini("[foo]\r\nbar\r\n");
        let s = sec(&bc, "foo");
        assert_eq!(s.values, vec!["bar"]);
    }

    #[test]
    fn line_ending_cr_only() {
        let bc = parse_ini("[foo]\rbar\r");
        let s = sec(&bc, "foo");
        assert_eq!(s.values, vec!["bar"]);
    }

    #[test]
    fn line_ending_mixed() {
        let bc = parse_ini("[foo]\r\nval1\rval2\nval3");
        let s = sec(&bc, "foo");
        assert_eq!(s.values, vec!["val1", "val2", "val3"]);
    }

    // 2. Blank-line and #-prefix skipping.
    #[test]
    fn blank_lines_skipped() {
        let bc = parse_ini("[foo]\n\n   \nbar");
        let s = sec(&bc, "foo");
        assert_eq!(s.values, vec!["bar"]);
    }

    #[test]
    fn comment_lines_skipped() {
        let bc = parse_ini("[foo]\n# this is a comment\nbar");
        let s = sec(&bc, "foo");
        assert_eq!(s.values, vec!["bar"]);
    }

    // 3. Multiple [name] headers accumulate lines.
    #[test]
    fn multiple_same_section_accumulates() {
        let bc = parse_ini("[ips]\nhost1\n[other]\nignored\n[ips]\nhost2");
        let s = sec(&bc, "ips");
        // Both should appear (order may vary — sort for determinism).
        let mut vals = s.values.clone();
        vals.sort();
        assert_eq!(vals, vec!["host1", "host2"]);
    }

    // 4. \# escape: keeps # and does NOT set had_trailing_comments.
    #[test]
    fn hash_escape_no_trailing_comment_flag() {
        let bc = parse_ini("[s]\nkey = a\\#b");
        let s = sec(&bc, "s");
        // After escaping: lookup["key"] == "a#b"
        assert_eq!(s.lookup.get("key").map(|v| v.as_str()), Some("a#b"));
        assert!(!s.had_trailing_comments);
    }

    // 5. Trailing comment: truncates value and sets had_trailing_comments.
    #[test]
    fn trailing_comment_strips_and_flags() {
        let bc = parse_ini("[s]\nkey = a # comment");
        let s = sec(&bc, "s");
        assert_eq!(s.lookup.get("key").map(|v| v.as_str()), Some("a"));
        assert!(s.had_trailing_comments);
    }

    // 6. Leading # in value (key = #foo):
    // The '#' is not at position 0 of the full line ("key = #foo"), so it's
    // treated as a trailing comment.  After stripping: "key =" which does not
    // match the kv regex (value part requires at least one non-space char).
    // So it ends up in values[], not in lookup.
    #[test]
    fn leading_hash_in_value_is_trailing_comment() {
        let bc = parse_ini("[s]\nkey = #foo");
        let s = sec(&bc, "s");
        // Key NOT in lookup (value was stripped entirely by comment-removal).
        assert!(!s.lookup.contains_key("key"), "key should not be in lookup");
        // "key =" (trimmed) ends up in values and lines because it's non-empty
        // and didn't match the kv regex.
        assert!(!s.values.is_empty(), "truncated line should appear in values");
        // had_trailing_comments should be true because a real trailing comment
        // was found and the remaining text was non-empty.
        assert!(s.had_trailing_comments);
    }

    // 7. Key regex: lines that don't match go to values.
    #[test]
    fn non_kv_lines_go_to_values() {
        let bc = parse_ini("[s]\nhello world\n1bad = ignored");
        let s = sec(&bc, "s");
        // "hello world" doesn't match key=value
        assert!(s.values.contains(&"hello world".to_string()));
        // "1bad = ignored" doesn't match (key must start with letter)
        assert!(s.values.contains(&"1bad = ignored".to_string()));
        assert!(!s.lookup.contains_key("1bad"));
    }

    // 8. Section with values + kv mixed.
    #[test]
    fn mixed_values_and_kv() {
        let bc = parse_ini("[crawl]\n1\noverlay = 1\ncounts = 0");
        let s = sec(&bc, "crawl");
        assert_eq!(s.values, vec!["1"]);
        assert_eq!(s.lookup.get("overlay").map(|v| v.as_str()), Some("1"));
        assert_eq!(s.lookup.get("counts").map(|v| v.as_str()), Some("0"));
    }

    // 9. Default empty-name section exists.
    #[test]
    fn default_section_created() {
        let bc = parse_ini("# just a comment\n[foo]\nval");
        assert!(bc.contains_key(""));
    }
}
