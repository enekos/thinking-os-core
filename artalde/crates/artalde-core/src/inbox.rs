//! zaindu's proposal files: `<wiki>/inbox/events/YYYY-MM-DD.md`, one
//! checkbox line per watched state change:
//!
//! ```text
//! - [ ] 08:00Z review join-com/auth#243 · [[hris]] · +6 more — you are blocking @x
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::InboxItem;

/// Proposal files larger than this are skipped.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The newest `days` proposal files, newest first.
pub fn files(wiki: &Path, days: usize) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(wiki.join("inbox/events")) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "md")
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().starts_with("20"))
        })
        .collect();
    out.sort();
    out.reverse();
    out.truncate(days);
    out
}

pub fn read(file: &Path) -> Vec<InboxItem> {
    if fs::metadata(file).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
        return Vec::new();
    }
    fs::read_to_string(file)
        .map(|text| parse(file, &text))
        .unwrap_or_default()
}

pub fn parse(file: &Path, text: &str) -> Vec<InboxItem> {
    text.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            parse_line(line).map(|item| InboxItem {
                file: file.to_path_buf(),
                line: i,
                ..item
            })
        })
        .collect()
}

fn parse_line(line: &str) -> Option<InboxItem> {
    let (done, rest) = if let Some(r) = line.strip_prefix("- [ ] ") {
        (false, r)
    } else {
        (
            true,
            line.strip_prefix("- [x] ")
                .or_else(|| line.strip_prefix("- [X] "))?,
        )
    };
    let (head, detail) = rest.split_once(" — ").unwrap_or((rest, ""));
    let mut parts = head.split(" · ");
    let mut words = parts.next()?.split_whitespace();
    let time = words.next()?.to_owned();
    let kind = words.next()?.to_owned();
    let target = words.collect::<Vec<_>>().join(" ");
    let links = parts
        .filter_map(|p| p.strip_prefix("[[").and_then(|p| p.strip_suffix("]]")))
        .map(str::to_owned)
        .collect();
    Some(InboxItem {
        done,
        url: github_url(&target),
        time,
        kind,
        target,
        links,
        detail: detail.trim().to_owned(),
        ..Default::default()
    })
}

/// `owner/repo#12` → its GitHub page (GitHub redirects `/pull/` for issues).
fn github_url(target: &str) -> Option<String> {
    let (repo, num) = target.split_once('#')?;
    let ok = repo.split('/').count() == 2
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
        && !num.is_empty()
        && num.chars().all(|c| c.is_ascii_digit());
    ok.then(|| format!("https://github.com/{repo}/pull/{num}"))
}

/// Tick (or untick) `item` in its file. Refuses if the line changed since
/// it was read, so a concurrent zaindu write is never clobbered.
pub fn set_done(item: &InboxItem, done: bool) -> io::Result<()> {
    let text = fs::read_to_string(&item.file)?;
    let mut lines: Vec<&str> = text.split('\n').collect();
    let current = lines
        .get(item.line)
        .and_then(|l| parse_line(l))
        .ok_or_else(|| io::Error::other("proposal line moved"))?;
    if current.target != item.target || current.time != item.time || current.kind != item.kind {
        return Err(io::Error::other("proposal line changed"));
    }
    let Some(line) = lines.get(item.line).copied() else {
        return Err(io::Error::other("proposal line moved"));
    };
    let rest = line.get(6..).unwrap_or_default();
    let new = format!("- [{}] {rest}", if done { 'x' } else { ' ' });
    if let Some(slot) = lines.get_mut(item.line) {
        *slot = &new;
    }
    let tmp = item.file.with_extension("md.artalde.tmp");
    fs::write(&tmp, lines.join("\n"))?;
    fs::rename(&tmp, &item.file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_parse() {
        let items = parse(
            Path::new("/w/x.md"),
            "# events\n\n- [ ] 08:00Z review join-com/data#7939 · [[cqf]] · [[cqf-a]] · +6 more — you are blocking @a\n- [x] 08:53Z merged enekos/zin#10 · [[zin]] — tick Next up\n- [ ] 09:00Z stale THU-12 — no url\n",
        );
        assert_eq!(items.len(), 3);
        let first = &items[0];
        assert_eq!(
            (first.line, first.done, first.kind.as_str()),
            (2, false, "review")
        );
        assert_eq!(first.target, "join-com/data#7939");
        assert_eq!(first.links, ["cqf", "cqf-a"]);
        assert_eq!(first.detail, "you are blocking @a");
        assert_eq!(
            first.url.as_deref(),
            Some("https://github.com/join-com/data/pull/7939")
        );
        assert!(items[1].done);
        assert_eq!(items[2].url, None);
    }

    #[test]
    fn ticking_rewrites_one_line_only() {
        let dir = std::env::temp_dir().join(format!("artalde-inbox-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("2026-11-19.md");
        fs::write(
            &file,
            "# e\n- [ ] 08:00Z red o/r#1 — red\n- [ ] 08:01Z red o/r#2 — red\n",
        )
        .unwrap();
        let items = read(&file);
        set_done(&items[1], true).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "# e\n- [ ] 08:00Z red o/r#1 — red\n- [x] 08:01Z red o/r#2 — red\n"
        );
        fs::write(&file, "# e\n- [ ] 08:00Z red o/r#9 — red\n").unwrap();
        assert!(
            set_done(&items[0], true).is_err(),
            "changed line is refused"
        );
    }
}
