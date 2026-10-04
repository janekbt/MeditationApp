//! Release notes for the About dialog, read from the AppStream
//! metainfo — the one place the notes are written.
//!
//! Every stable `<release>` is shown, newest first: the newest one
//! bare (the dialog already heads it with its version), each older
//! one after a dividing line and under its version number, so the
//! history stays visible.
//! Development (beta) releases are left out. Each `<p>` / `<li>` text
//! goes through `translate`, so the metainfo's own gettext entries
//! apply.

/// The line between two releases. The dialog's markup has no rule
/// element (only p, ul, ol, li, em, code; anything else is rejected),
/// so it is a paragraph of box-drawing characters.
const DIVIDER: &str = "<p>\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}</p>";

/// AppStream description markup for the About dialog.
pub fn about_markup(metainfo: &str, translate: impl Fn(String) -> String) -> String {
    let mut out = String::new();
    let mut first = true;
    for release in metainfo.split("<release ").skip(1) {
        let release = &release[..release.find("</release>").unwrap_or(release.len())];
        let head = &release[..release.find('>').unwrap_or(0)];
        if head.contains("type=\"development\"") {
            continue;
        }
        if !first {
            out.push_str(DIVIDER);
            if let Some(version) = attr(head, "version") {
                out.push_str(&format!("<p><em>{}</em></p>", escape(version)));
            }
        }
        first = false;
        let mut in_list = false;
        let mut rest = release;
        while let Some((tag, text, after)) = next_block(rest) {
            match (tag, in_list) {
                ("li", false) => {
                    out.push_str("<ul>");
                    in_list = true;
                }
                ("p", true) => {
                    out.push_str("</ul>");
                    in_list = false;
                }
                _ => {}
            }
            let text = translate(normalize(text));
            out.push_str(&format!("<{tag}>{}</{tag}>", escape(&text)));
            rest = after;
        }
        if in_list {
            out.push_str("</ul>");
        }
    }
    out
}

/// The next `<p>` or `<li>` in `s`: (tag, raw text, the rest after it).
fn next_block(s: &str) -> Option<(&'static str, &str, &str)> {
    let p = s.find("<p>").map(|i| (i, "p"));
    let li = s.find("<li>").map(|i| (i, "li"));
    let (at, tag) = match (p, li) {
        (Some(a), Some(b)) => a.min(b),
        (a, b) => a.or(b)?,
    };
    let start = at + tag.len() + 2;
    let close = format!("</{tag}>");
    let end = start + s[start..].find(&close)?;
    Some((tag, &s[start..end], &s[end + close.len()..]))
}

fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = head.find(&key)? + key.len();
    Some(&head[start..start + head[start..].find('"')?])
}

/// XML text as gettext sees it: entities resolved, whitespace collapsed.
fn normalize(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const META: &str = r#"<component><releases>
    <release version="3.0" date="2026-03-01">
      <description translate="yes">
        <p>Newest &quot;intro&quot;.</p>
        <ul>
          <li>One</li>
          <li>Two
            lines</li>
        </ul>
      </description>
    </release>
    <release version="2.1" date="2026-02-10" type="development">
      <description translate="yes"><p>Beta only.</p></description>
    </release>
    <release version="2.0" date="2026-02-01">
      <description translate="yes"><p>Older &amp; wiser.</p></description>
    </release>
  </releases></component>"#;

    #[test]
    fn newest_bare_older_divided_under_their_version_betas_left_out() {
        assert_eq!(
            about_markup(META, |s| s),
            format!(
                "<p>Newest \"intro\".</p><ul><li>One</li><li>Two lines</li></ul>\
                 {DIVIDER}<p><em>2.0</em></p><p>Older &amp; wiser.</p>"
            )
        );
    }

    #[test]
    fn every_text_is_translated_then_escaped() {
        let out = about_markup(META, |s| format!("[{s}] <x>"));
        assert!(out.contains("<li>[Two lines] &lt;x&gt;</li>"));
        assert!(out.contains("<p>[Older &amp; wiser.] &lt;x&gt;</p>"));
        assert!(!out.contains("Beta only"));
    }

    #[test]
    fn the_real_metainfo_has_notes_for_the_newest_release() {
        let meta = include_str!("../../meditate-gtk/data/io.github.janekbt.Meditate.metainfo.xml.in");
        let out = about_markup(meta, |s| s);
        assert!(out.starts_with("<p>"));
        assert!(out.contains("<p><em>26.10.0</em></p>"), "older releases follow");
        assert!(!out.contains("<em>26.9.0</em>"), "the beta is left out");
    }
}
