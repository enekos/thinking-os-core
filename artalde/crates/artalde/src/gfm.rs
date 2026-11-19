//! GitHub's flavour of HTML-in-markdown, as bots write it (CodeRabbit,
//! Bender, SonarQube, Linear, GitHub Actions), turned into structure the
//! markdown renderer understands:
//!
//! - `<details><summary>…</summary>…</details>` → [`Segment::Details`]
//!   (nested, `open` respected), also inside `> [!NOTE]` alerts.
//! - `<table>` → a pipe table; `<a>`, `<code>`, `<strong>`, `<h1-6>`,
//!   `<br>` → their markdown; `<dl>/<dd>/<picture>/<img>/<source>` and
//!   `<!-- comments -->` disappear.

/// A piece of a comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Markdown(String),
    Details {
        summary: String,
        open: bool,
        body: Vec<Segment>,
    },
    /// A `> [!KIND]` alert or quote that holds structure of its own.
    Callout {
        kind: String,
        body: Vec<Segment>,
    },
}

/// Nesting deeper than this is flattened into text.
const MAX_DEPTH: usize = 8;

pub fn parse(src: &str) -> Vec<Segment> {
    let cleaned = strip_comments(src);
    segments(&cleaned, 0)
}

fn segments(src: &str, depth: usize) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut lines = src.lines().peekable();
    let flush = |text: &mut String, out: &mut Vec<Segment>| {
        let md = html_to_markdown(text);
        if !md.trim().is_empty() {
            out.push(Segment::Markdown(md));
        }
        text.clear();
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        // A quote block holding <details>: take the whole block, unquote it.
        if trimmed.starts_with('>') && depth < MAX_DEPTH {
            let mut block = vec![line];
            while let Some(next) = lines.peek() {
                if next.trim_start().starts_with('>') {
                    block.push(lines.next().unwrap_or_default());
                } else {
                    break;
                }
            }
            if block.iter().any(|l| l.contains("<details")) {
                flush(&mut text, &mut out);
                let inner: Vec<&str> = block
                    .iter()
                    .map(|l| {
                        let l = l.trim_start().trim_start_matches('>');
                        l.strip_prefix(' ').unwrap_or(l)
                    })
                    .collect();
                let (kind, body) = match inner.first().map(|l| l.trim()) {
                    Some(first) if first.starts_with("[!") && first.ends_with(']') => (
                        first
                            .trim_start_matches("[!")
                            .trim_end_matches(']')
                            .to_uppercase(),
                        inner.get(1..).unwrap_or_default().join("\n"),
                    ),
                    _ => (String::new(), inner.join("\n")),
                };
                out.push(Segment::Callout {
                    kind,
                    body: segments(&body, depth + 1),
                });
            } else {
                for l in block {
                    text.push_str(l);
                    text.push('\n');
                }
            }
            continue;
        }
        if let Some(at) = line.find("<details").filter(|_| depth < MAX_DEPTH) {
            text.push_str(line.get(..at).unwrap_or_default());
            text.push('\n');
            flush(&mut text, &mut out);
            // Gather until the matching </details>.
            let mut buf = line.get(at..).unwrap_or_default().to_owned();
            buf.push('\n');
            let mut level = count(&buf, "<details") as isize - count(&buf, "</details>") as isize;
            while level > 0 {
                let Some(next) = lines.next() else { break };
                level += count(next, "<details") as isize - count(next, "</details>") as isize;
                buf.push_str(next);
                buf.push('\n');
            }
            let (seg, rest) = details(&buf, depth);
            out.push(seg);
            if !rest.trim().is_empty() {
                text.push_str(&rest);
                text.push('\n');
            }
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    flush(&mut text, &mut out);
    out
}

fn count(s: &str, pat: &str) -> usize {
    s.matches(pat).count()
}

/// One `<details>…</details>` block (and whatever trails its close tag).
fn details(block: &str, depth: usize) -> (Segment, String) {
    let open_end = block.find('>').unwrap_or(0);
    let open = block.get(..open_end).is_some_and(|t| t.contains(" open"));
    let mut rest = block.get(open_end + 1..).unwrap_or_default();
    let mut summary = String::new();
    if let Some(s) = rest.find("<summary") {
        let after = rest.get(s..).unwrap_or_default();
        let body_start = after.find('>').map_or(0, |i| i + 1);
        let end = after.find("</summary>").unwrap_or(after.len());
        summary = plain(&inline_text(after.get(body_start..end).unwrap_or_default()));
        rest = after.get(end + "</summary>".len()..).unwrap_or_default();
    }
    // Find the matching close tag.
    let mut level = 1isize;
    let mut idx = 0usize;
    let bytes = rest;
    let mut close_at = rest.len();
    while idx < bytes.len() {
        let tail = bytes.get(idx..).unwrap_or_default();
        if tail.starts_with("<details") {
            level += 1;
        } else if tail.starts_with("</details>") {
            level -= 1;
            if level == 0 {
                close_at = idx;
                break;
            }
        }
        idx += tail.chars().next().map_or(1, char::len_utf8);
    }
    let body = rest.get(..close_at).unwrap_or_default();
    let trailing = rest
        .get(close_at + "</details>".len()..)
        .unwrap_or_default();
    let summary = if summary.trim().is_empty() {
        "Details".to_owned()
    } else {
        summary.trim().to_owned()
    };
    (
        Segment::Details {
            summary,
            open,
            body: segments(body, depth + 1),
        },
        trailing.to_owned(),
    )
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<!--") {
        out.push_str(rest.get(..i).unwrap_or_default());
        rest = rest
            .get(i..)
            .and_then(|r| r.find("-->").map(|e| r.get(e + 3..).unwrap_or_default()))
            .unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// Plain text of an inline HTML fragment (for summaries).
pub fn inline_text(s: &str) -> String {
    let md = html_to_markdown(s);
    md.replace("**", "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One line of markdown as plain text: images dropped, links to their
/// text, emphasis and code marks removed.
pub fn plain(md: &str) -> String {
    let mut out = String::new();
    let mut rest = md;
    while let Some(i) = rest.find(['!', '[']) {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        let image = tail.starts_with("![");
        let open = if image { 2 } else { 1 };
        let (Some(close), true) = (tail.find("]("), tail.starts_with('[') || image) else {
            out.push_str(tail.get(..1).unwrap_or_default());
            rest = tail.get(1..).unwrap_or_default();
            continue;
        };
        let text = tail.get(open..close).unwrap_or_default();
        let after = tail.get(close + 2..).unwrap_or_default();
        let end = after.find(')').map_or(after.len(), |e| e + 1);
        if !image {
            out.push_str(&plain(text));
        }
        rest = after.get(end..).unwrap_or_default();
    }
    out.push_str(rest);
    out.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rewrite the HTML GitHub allows into markdown; drop the rest.
pub fn html_to_markdown(s: &str) -> String {
    let s = tables(s);
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find('<') {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        let Some(end) = tail.find('>') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let tag = tail.get(1..end).unwrap_or_default();
        let after = tail.get(end + 1..).unwrap_or_default();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        // Not a tag (`a < b`, `<T>` generics in prose): keep it.
        let known = [
            "a",
            "b",
            "strong",
            "em",
            "i",
            "code",
            "br",
            "p",
            "div",
            "span",
            "img",
            "picture",
            "source",
            "dl",
            "dd",
            "dt",
            "sup",
            "sub",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "hr",
            "ul",
            "ol",
            "li",
            "blockquote",
            "summary",
            "details",
            "kbd",
            "samp",
            "del",
            "s",
        ];
        if !known.contains(&name.as_str()) {
            out.push('<');
            rest = tail.get(1..).unwrap_or_default();
            continue;
        }
        match (name.as_str(), closing) {
            ("a", false) => {
                let href = attr(tag, "href").unwrap_or_default();
                let close = after.find("</a>").unwrap_or(after.len());
                let inner = html_to_markdown(after.get(..close).unwrap_or_default());
                let inner = inner.trim();
                if !inner.is_empty() {
                    if href.is_empty() {
                        out.push_str(inner)
                    } else {
                        out.push_str(&format!("[{inner}]({href})"))
                    }
                }
                rest = after
                    .get((close + 4).min(after.len())..)
                    .unwrap_or_default();
                continue;
            }
            ("b" | "strong", _) => out.push_str("**"),
            ("em" | "i", _) => out.push('*'),
            ("del" | "s", _) => out.push_str("~~"),
            ("code" | "kbd" | "samp", _) => out.push('`'),
            ("br", _) => {
                // Inside a pipe-table row a line break would end the row.
                let line_start = out.rfind('\n').map_or(0, |i| i + 1);
                if out
                    .get(line_start..)
                    .is_some_and(|l| l.trim_start().starts_with('|'))
                {
                    out.push_str(" · ");
                } else {
                    out.push_str("  \n");
                }
            }
            ("p" | "div", true) => out.push_str("\n\n"),
            ("hr", _) => out.push_str("\n\n---\n\n"),
            ("li", false) => out.push_str("\n- "),
            (h, false) if h.len() == 2 && h.starts_with('h') => {
                let level = h
                    .get(1..)
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(3)
                    .clamp(1, 6);
                out.push_str(&format!("\n\n{} ", "#".repeat(level)));
            }
            (h, true) if h.len() == 2 && h.starts_with('h') => out.push_str("\n\n"),
            ("img", _) => {
                // Icons (small or alt-less) vanish; real images become links.
                let alt = attr(tag, "alt").unwrap_or_default();
                let small = attr(tag, "width")
                    .and_then(|w| w.parse::<u32>().ok())
                    .is_some_and(|w| w <= 32)
                    || attr(tag, "height")
                        .and_then(|w| w.parse::<u32>().ok())
                        .is_some_and(|h| h <= 32);
                if !small
                    && !alt.trim().is_empty()
                    && let Some(src) = attr(tag, "src")
                {
                    out.push_str(&format!("[🖼 {}]({src})", alt.trim()));
                }
            }
            _ => {}
        }
        rest = after;
    }
    out.push_str(rest);
    out.replace("&amp;", "&")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!("{name}=");
    let i = tag.find(&pat)?;
    let v = tag.get(i + pat.len()..)?;
    let (q, v) = match v.chars().next()? {
        c @ ('"' | '\'') => (c, v.get(1..)?),
        _ => (' ', v),
    };
    Some(v.split(q).next().unwrap_or_default().replace("&amp;", "&"))
}

/// `<table>` → pipe table, one row per `<tr>`.
fn tables(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("<table") {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i..).unwrap_or_default();
        let end = tail
            .find("</table>")
            .map_or(tail.len(), |e| e + "</table>".len());
        let table = tail.get(..end).unwrap_or_default();
        let rows: Vec<Vec<String>> = table
            .split("<tr")
            .skip(1)
            .map(|row| {
                let mut cells = Vec::new();
                let mut r = row;
                while let Some(c) = r
                    .find("<t")
                    .filter(|c| matches!(r.get(c + 2..c + 3), Some("d" | "h")))
                {
                    let after = r.get(c..).unwrap_or_default();
                    let open_end = after.find('>').map_or(after.len(), |e| e + 1);
                    let body = after.get(open_end..).unwrap_or_default();
                    let close = body.find("</t").unwrap_or(body.len());
                    let cell = html_to_markdown(body.get(..close).unwrap_or_default())
                        .replace('\n', " ")
                        .replace('|', "\\|");
                    cells.push(cell.split_whitespace().collect::<Vec<_>>().join(" "));
                    r = body
                        .get(close..)
                        .unwrap_or_default()
                        .get(3..)
                        .unwrap_or_default();
                }
                cells
            })
            .filter(|c| !c.is_empty())
            .collect();
        if let Some(head) = rows.first() {
            out.push_str("\n\n| ");
            out.push_str(&head.join(" | "));
            out.push_str(" |\n|");
            out.push_str(&" --- |".repeat(head.len()));
            out.push('\n');
            for r in rows.iter().skip(1) {
                out.push_str("| ");
                out.push_str(&r.join(" | "));
                out.push_str(" |\n");
            }
            out.push('\n');
        }
        rest = tail.get(end..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_nest_and_keep_open() {
        let src = "intro\n<details open>\n<summary><strong>Walk</strong>through</summary>\n<dl><dd><br>\n\nbody **x**\n\n<details>\n<summary>Inner</summary>\n\ndeep\n\n</details>\n</dd></dl>\n</details>\nafter";
        let segs = parse(src);
        assert_eq!(segs.len(), 3, "{segs:#?}");
        assert!(matches!(&segs[0], Segment::Markdown(m) if m.trim() == "intro"));
        let Segment::Details {
            summary,
            open,
            body,
        } = &segs[1]
        else {
            panic!("{segs:#?}")
        };
        assert_eq!((summary.as_str(), *open), ("Walkthrough", true));
        assert!(matches!(&body[0], Segment::Markdown(m) if m.contains("body **x**")));
        assert!(
            matches!(&body[1], Segment::Details { summary, open: false, .. } if summary == "Inner")
        );
        assert!(matches!(&segs[2], Segment::Markdown(m) if m.trim() == "after"));
    }

    /// Every real bot comment saved under `ARTALDE_COMMENT_SAMPLES` parses
    /// without leftover tags.
    #[test]
    fn real_comments_leave_no_markup() {
        let Some(dir) = std::env::var_os("ARTALDE_COMMENT_SAMPLES") else {
            return;
        };
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
            for line in text.lines() {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                let body = v["body"].as_str().unwrap_or_default();
                seen += 1;
                fn walk(segs: &[Segment], body: &str) {
                    for s in segs {
                        match s {
                            Segment::Markdown(m) => {
                                for tag in [
                                    "<details", "<summary", "<picture", "<img", "<dl>", "<dd>",
                                    "<!--", "<table", "<a href",
                                ] {
                                    assert!(
                                        !m.contains(tag),
                                        "{tag} left in:\n{m}\n--- from:\n{body}"
                                    );
                                }
                            }
                            Segment::Details { body: b, .. } | Segment::Callout { body: b, .. } => {
                                walk(b, body)
                            }
                        }
                    }
                }
                walk(&parse(body), body);
            }
        }
        assert!(seen > 0);
    }

    #[test]
    fn alerts_holding_details_become_callouts() {
        let src = "> [!IMPORTANT]\n> ## Review skipped\n> \n> <details>\n> <summary>Run configuration</summary>\n> \n> - **Mode**: Auto\n> </details>\n\nnext";
        let segs = parse(src);
        let Segment::Callout { kind, body } = &segs[0] else {
            panic!("{segs:#?}")
        };
        assert_eq!(kind, "IMPORTANT");
        assert!(matches!(&body[0], Segment::Markdown(m) if m.contains("## Review skipped")));
        assert!(
            matches!(&body[1], Segment::Details { summary, .. } if summary == "Run configuration")
        );
    }

    #[test]
    fn breaks_inside_table_rows_stay_on_the_row() {
        let md = html_to_markdown("| **data** | ✅ Deployed<br>2026-11-18 | x |\nline<br>next");
        assert!(
            md.starts_with("| **data** | ✅ Deployed · 2026-11-18 | x |"),
            "{md}"
        );
        assert!(md.ends_with("line  \nnext"), "{md}");
    }

    #[test]
    fn plain_drops_marks() {
        assert_eq!(
            plain("![Passed](https://x/p.png) Quality Gate **passed**"),
            "Quality Gate passed"
        );
        assert_eq!(
            plain("[THU-4823 Pre-hijack](https://linear.app/x) and `code`"),
            "THU-4823 Pre-hijack and code"
        );
        assert_eq!(plain("a ! b [c] d"), "a ! b [c] d");
    }

    #[test]
    fn html_becomes_markdown() {
        assert_eq!(strip_comments("a<!-- x\ny -->b"), "ab");
        assert_eq!(
            html_to_markdown(r#"<a href="https://x.y/z?a=1&amp;b=2">go</a>"#),
            "[go](https://x.y/z?a=1&b=2)"
        );
        assert_eq!(
            html_to_markdown("<h3>Bugbot</h3>rest").trim(),
            "### Bugbot\n\nrest".trim()
        );
        assert_eq!(
            html_to_markdown(r#"<picture><img src="i.svg" width="16" alt=""></picture> Minor"#),
            " Minor"
        );
        assert_eq!(
            html_to_markdown("Vec<T> and a < b"),
            "Vec<T> and a < b",
            "non-tags survive"
        );
        let t = html_to_markdown(
            "<table><tr><th>Build</th><th>Lint</th></tr><tr><td><code>✅ passed</code></td><td>ok<br>later</td></tr></table>",
        );
        assert!(t.contains("| Build | Lint |"), "{t}");
        assert!(t.contains("| `✅ passed` | ok later |"), "{t}");
    }
}
