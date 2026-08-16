//! Thin adapter over the in-house `rusty_regx` engine for the two web-scrape /
//! filename patterns the vendor pipeline needs, replacing the former
//! `regex-lite` dependency.
//!
//! `rusty_regx` is a POSIX-ERE engine: it has no `\d`/`\w`/`\s` shorthand
//! classes and no `(?i)` inline flag. The vendor pipeline only ever uses those
//! shorthands and case-insensitive matching, so this module translates the
//! shorthands into equivalent bracket expressions and routes case-insensitive
//! matching through `Regex::new_posix_ci`.
//!
//! Parity note: `regex-lite` matches leftmost-*first*; `new_posix_ci` matches
//! leftmost-*longest*. The two shipped patterns contain no alternation and no
//! ambiguous unbounded quantifier over a captured span, so their captures are
//! identical under both disciplines. The `tests` module below differential-
//! tests this against `regex-lite` (a dev-dependency) over the real patterns.
//! A future web-scrape pattern that relies on leftmost-first disambiguation,
//! or on an unsupported construct (`\b`, lookaround, lazy quantifiers), must
//! be reviewed before it is added to `vendors.json`.

pub use rusty_regx::Regex;

/// Translate `regex-lite`-flavoured shorthand classes into POSIX-ERE bracket
/// expressions that `rusty_regx` understands. Backslash escapes for literal
/// metacharacters (`\.`, `\(`, ...) are passed through unchanged.
fn translate(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str("[0-9]"),
            Some('D') => out.push_str("[^0-9]"),
            Some('w') => out.push_str("[A-Za-z0-9_]"),
            Some('W') => out.push_str("[^A-Za-z0-9_]"),
            Some('s') => out.push_str("[ \t\r\n\x0c\x0b]"),
            Some('S') => out.push_str("[^ \t\r\n\x0c\x0b]"),
            // Any other escape (\. \( \\ ...) is a literal metachar for the
            // ERE engine too — pass both bytes through verbatim.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Compile a case-insensitive web-scrape pattern (former `(?i)…` usage).
pub fn compile_ci(pattern: &str) -> Result<Regex, String> {
    Regex::new_posix_ci(&translate(pattern)).map_err(|e| e.to_string())
}

/// Compile a case-sensitive pattern (former default `regex_lite::Regex::new`).
pub fn compile(pattern: &str) -> Result<Regex, String> {
    Regex::new(&translate(pattern)).map_err(|e| e.to_string())
}

/// Backslash-escape every ERE metacharacter in `literal` so it matches
/// itself. Used to splice a resolved file name (which routinely contains `.`,
/// and may contain `+` or `-`) into a checksum-scrape pattern.
///
/// `translate` passes `\<metachar>` through verbatim, so escaping here is
/// safe under both `compile` and `compile_ci`.
pub fn escape(literal: &str) -> String {
    const META: &[char] = &[
        '.', '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ];
    let mut out = String::with_capacity(literal.len());
    for c in literal.chars() {
        if META.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_shorthands() {
        assert_eq!(translate(r"\d{8}"), "[0-9]{8}");
        assert_eq!(translate(r"a\.b"), r"a\.b");
        assert_eq!(translate(r"\w+"), "[A-Za-z0-9_]+");
    }

    /// Differential guard against the retired `regex-lite` engine over the
    /// exact patterns the vendor pipeline ships/hardcodes.
    #[test]
    fn matches_regex_lite_on_shipped_patterns() {
        // Site 1: the MSYS2 web-scrape pattern, matched case-insensitively.
        let scrape = r#"href="(msys2-base-x86_64-(\d{8})\.tar\.xz)""#;
        let html = r#"<a href="msys2-base-x86_64-20260615.tar.xz">latest</a>"#;

        let ours = compile_ci(scrape).unwrap();
        let theirs = regex_lite::Regex::new(&format!("(?i){scrape}")).unwrap();
        let oc = ours.captures(html).unwrap();
        let tc = theirs.captures(html).unwrap();
        assert_eq!(oc.get(1), tc.get(1).map(|m| m.as_str()));
        assert_eq!(oc.get(2), tc.get(2).map(|m| m.as_str()));
        assert_eq!(oc.get(1), Some("msys2-base-x86_64-20260615.tar.xz"));
        assert_eq!(oc.get(2), Some("20260615"));

        // Mixed-case host markup still matches (the (?i) requirement).
        let html_uc = r#"HREF="MSYS2-base-x86_64-20260615.tar.xz""#;
        assert_eq!(
            compile_ci(scrape)
                .unwrap()
                .captures(html_uc)
                .unwrap()
                .get(2),
            Some("20260615")
        );

        // Site 2: the hardcoded version-extraction pattern (case-sensitive).
        let ver = r"(\d+\.?\d*\.?\d*\.?\d*)";
        for input in ["node-v20.11.1-win-x64", "7z2301", "foo", "1.2.3.4"] {
            let ours = compile(ver).unwrap();
            let theirs = regex_lite::Regex::new(ver).unwrap();
            assert_eq!(
                ours.captures(input).and_then(|c| c.get(1)),
                theirs
                    .captures(input)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str()),
                "divergence on {input:?}"
            );
        }
    }
}
