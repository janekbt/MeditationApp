//! Translation helpers for user-visible strings.
//!
//! `gettext()` re-exports `gettextrs::gettext` under our project's text
//! domain (set up in `main.rs` at startup). Use it everywhere a string
//! would appear on the UI:
//!
//! ```ignore
//! use crate::i18n::gettext;
//! button.set_label(&gettext("Save"));
//! ```
//!
//! `ngettext()` is the plural-aware counterpart. Pass both forms; the
//! locale's `Plural-Forms` header picks. Use it instead of
//! `gettext("X {n} Y").replace(...)` whenever the rendered string
//! varies on a count — `n` may pick a special form (Russian few/many,
//! Polish 1/few/many, Arabic 0/1/2/few/many/other) that a binary
//! singular/plural split would render wrong:
//!
//! ```ignore
//! use crate::i18n::ngettext;
//! let label = ngettext("1 session", "{n} sessions", n as u32)
//!     .replace("{n}", &n.to_string());
//! ```
//!
//! `xgettext` picks up both `gettext("…")` and `ngettext("…","…",…)`
//! call sites automatically when scanning the files listed in
//! `po/POTFILES.in` (`build-aux/update-translations.sh`). Call them
//! unqualified, as above — see the tests below for why.

pub use gettextrs::{gettext, ngettext};

/// Guards the gettext catalogue against silent drift. The template is
/// generated inside the GNOME SDK (`build-aux/update-translations.sh`),
/// whose xgettext parses Rust properly but skips path-qualified calls
/// (`crate::i18n::gettext(…)`) and anything inside path-qualified macros
/// (`glib::clone!(…)`). These tests fail if a string would be missed,
/// if a file is missing from POTFILES.in, or if the template is stale.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn crate_dir() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn rust_sources() -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&crate_dir().join("src"), &mut out);
        out.sort();
        out
    }

    fn relative(path: &Path) -> String {
        path.strip_prefix(crate_dir()).unwrap().to_string_lossy().into_owned()
    }

    /// The source up to its `mod tests`, with `//` comment lines blanked,
    /// so doc examples and these tests' own messages don't count as call
    /// sites.
    fn code_of(path: &Path) -> String {
        let text = std::fs::read_to_string(path).unwrap();
        let end = text.find("#[cfg(test)]\nmod tests").unwrap_or(text.len());
        text[..end]
            .lines()
            .map(|l| if l.trim_start().starts_with("//") { "" } else { l })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parses a Rust string literal starting at `s[0] == '"'`, returning
    /// its value and the byte length consumed.
    fn string_literal(s: &str) -> (String, usize) {
        let mut value = String::new();
        let mut chars = s.char_indices().skip(1).peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (value, i + 1),
                '\\' => match chars.next().unwrap().1 {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    '\'' => value.push('\''),
                    '\n' => {
                        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    other => panic!("unhandled escape \\{other} in {s:.60}"),
                },
                c => value.push(c),
            }
        }
        panic!("unterminated string literal: {s:.60}");
    }

    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Every `gettext("…")` / `ngettext("…", "…", n)` msgid in `code`.
    fn msgids(code: &str) -> Vec<String> {
        let mut out = Vec::new();
        for name in ["gettext(", "ngettext("] {
            for (at, _) in code.match_indices(name) {
                if code[..at].chars().next_back().is_some_and(is_ident) {
                    continue;
                }
                let mut rest = code[at + name.len()..].trim_start();
                let forms = if name == "ngettext(" { 2 } else { 1 };
                for _ in 0..forms {
                    let (value, len) = string_literal(rest);
                    out.push(value);
                    rest = rest[len..].trim_start().trim_start_matches(',').trim_start();
                }
            }
        }
        out
    }

    /// Byte length of the bracketed group `s` starts with, up to but not
    /// including its closing bracket. Skips brackets inside strings.
    fn group_len(s: &str) -> usize {
        let (mut depth, mut i) = (0usize, 0usize);
        while i < s.len() {
            match s.as_bytes()[i] {
                b'"' => i += string_literal(&s[i..]).1 - 1,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        i
    }

    fn potfiles() -> BTreeSet<String> {
        std::fs::read_to_string(crate_dir().join("po/POTFILES.in"))
            .unwrap()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect()
    }

    /// Every msgid and msgid_plural in the template, unescaped.
    fn template_msgids() -> BTreeSet<String> {
        let pot = std::fs::read_to_string(crate_dir().join("po/meditate.pot")).unwrap();
        let mut out = BTreeSet::new();
        let mut current: Option<String> = None;
        for line in pot.lines() {
            let quoted = if let Some(r) = line.strip_prefix("msgid_plural ") {
                out.extend(current.take());
                current = Some(String::new());
                r
            } else if let Some(r) = line.strip_prefix("msgid ") {
                out.extend(current.take());
                current = Some(String::new());
                r
            } else if line.starts_with('"') {
                line
            } else {
                out.extend(current.take());
                continue;
            };
            if let Some(cur) = current.as_mut() {
                cur.push_str(&string_literal(quoted).0);
            }
        }
        out.extend(current);
        out
    }

    #[test]
    fn scanner_reads_rust_literals_like_the_compiler() {
        let code = "x(gettext(\"a \\\"b\\\"\")); y = ngettext(\n    \"one \\\n     line\",\n    \"{n} lines\",\n    n);";
        assert_eq!(msgids(code), ["a \"b\"", "one line", "{n} lines"]);
        assert!(msgids("pgettext(\"x\")").is_empty());
    }

    #[test]
    fn every_file_with_translatable_strings_is_in_potfiles() {
        let listed = potfiles();
        let mut missing: Vec<String> = rust_sources()
            .iter()
            .filter(|p| !msgids(&code_of(p)).is_empty())
            .map(|p| relative(p))
            .filter(|p| !listed.contains(p))
            .collect();
        for entry in std::fs::read_dir(crate_dir().join("data/ui")).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let rel = relative(&path);
            if (text.contains("_(\"") || text.contains("translatable=\"yes\""))
                && !listed.contains(&rel)
            {
                missing.push(rel);
            }
        }
        assert!(missing.is_empty(), "add to po/POTFILES.in: {missing:?}");
    }

    #[test]
    fn no_gettext_call_is_path_qualified() {
        let offenders: Vec<String> = rust_sources()
            .iter()
            .filter(|p| {
                let code = code_of(p);
                code.contains("::gettext(") || code.contains("::ngettext(")
            })
            .map(|p| relative(p))
            .collect();
        assert!(
            offenders.is_empty(),
            "xgettext skips `path::gettext(…)`; import it and call `gettext(…)`: {offenders:?}",
        );
    }

    #[test]
    fn no_gettext_call_sits_inside_a_path_qualified_macro() {
        let mut offenders = Vec::new();
        for path in rust_sources() {
            let code = code_of(&path);
            for (at, _) in code.match_indices("!(") {
                let head = &code[..at];
                let name_start = head.trim_end_matches(is_ident).len();
                if !head[..name_start].ends_with("::") {
                    continue;
                }
                let body = &code[at + 1..];
                let i = group_len(body);
                if !msgids(&body[..i]).is_empty() {
                    let line = head.lines().count();
                    offenders.push(format!("{}:{line}", relative(&path)));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "xgettext skips strings inside `path::macro!(…)`; import the macro \
             (e.g. `use glib::clone;` then `clone!(…)`): {offenders:?}",
        );
    }

    /// A language's singular form can cover more counts than 1 (Russian
    /// uses it for 21, 31, …), so its translation carries `{n}` too. A call
    /// that doesn't fill `{n}` in would show "1" for those counts, or a
    /// literal "{n}".
    #[test]
    fn every_plural_count_is_filled_in() {
        let mut offenders = Vec::new();
        for path in rust_sources() {
            let code = code_of(&path);
            for (at, _) in code.match_indices("ngettext(") {
                if code[..at].chars().next_back().is_some_and(is_ident) {
                    continue;
                }
                let open = at + "ngettext".len();
                let args = &code[open..open + group_len(&code[open..])];
                if !msgids(&format!("ngettext{args})"))[1].contains("{n}") {
                    continue;
                }
                let after = code[open + group_len(&code[open..]) + 1..].trim_start();
                if !after.starts_with(".replace(\"{n}\"") {
                    let line = code[..at].lines().count();
                    offenders.push(format!("{}:{line}", relative(&path)));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "ngettext(…) result needs .replace(\"{{n}}\", …): {offenders:?}",
        );
    }

    #[test]
    fn template_contains_every_translatable_string() {
        let template = template_msgids();
        let missing: BTreeSet<String> = rust_sources()
            .iter()
            .flat_map(|p| msgids(&code_of(p)))
            .filter(|m| !template.contains(m))
            .collect();
        assert!(
            missing.is_empty(),
            "{} strings missing from po/meditate.pot — run \
             build-aux/update-translations.sh: {missing:#?}",
            missing.len(),
        );
    }
}
