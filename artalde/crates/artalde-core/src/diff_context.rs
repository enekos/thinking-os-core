//! Expand omitted context from the post-image while keeping original changes and review anchors.
use std::io::Read;
use std::path::Path;

use serde_json::json;

use crate::github::{DiffFile, DiffLine, LineKind, PrRef, graphql_exec};

const MAX_BYTES: usize = 1024 * 1024;
const MAX_LINES: usize = 20_000;

pub fn load(pr: &PrRef, file: &DiffFile, revision: &str) -> Result<Vec<DiffLine>, String> {
    if file.truncated {
        return Err("The patch is truncated; full context is unavailable.".into());
    }
    let text = if pr.number == 0 {
        let root = crate::changes::root(Path::new(&pr.repo))?;
        let path = root.join(&file.path);
        let real = path.canonicalize().map_err(|e| e.to_string())?;
        if !real.starts_with(&root) || path.is_symlink() || !real.is_file() {
            return Err("Full context is available for regular files in this worktree.".into());
        }
        let mut text = String::new();
        std::fs::File::open(real)
            .and_then(|f| f.take((MAX_BYTES + 1) as u64).read_to_string(&mut text))
            .map_err(|e| format!("Reading file text: {e}"))?;
        text
    } else {
        let (owner, name) = pr.owner_name();
        let answer = graphql_exec(&json!({
            "query": "query($owner:String!, $name:String!, $rev:String!) { repository(owner:$owner, name:$name) { object(expression:$rev) { ... on Blob { isBinary isTruncated text } } } }",
            "variables": {"owner":owner,"name":name,"rev":revision}
        }))?;
        let blob = answer
            .pointer("/data/repository/object")
            .filter(|v| v.is_object())
            .ok_or("File content is unavailable at this revision.")?;
        if blob["isBinary"] == true || blob["isTruncated"] == true {
            return Err("GitHub cannot display full text for this binary or large file.".into());
        }
        blob["text"]
            .as_str()
            .ok_or("GitHub did not return file text.")?
            .to_owned()
    };
    expand(file, &text)
}

// A range with zero lines starts just after its anchor, e.g. +3,0.
fn start(range: &str) -> Option<u32> {
    let range = range.get(1..)?;
    let (line, len) = range.split_once(',').unwrap_or((range, "1"));
    line.parse::<u32>().ok()?.checked_add(u32::from(len == "0"))
}

fn expand(file: &DiffFile, text: &str) -> Result<Vec<DiffLine>, String> {
    if text.len() > MAX_BYTES || text.lines().count() > MAX_LINES {
        return Err("Full context is limited to 1 MiB and 20,000 lines per file.".into());
    }
    if text.contains('\0') {
        return Err("Binary files have no text context.".into());
    }
    let mut content = text.lines();
    let mut old = 1u32;
    let mut new = 1u32;
    let mut out = Vec::new();
    let stale = "The file and patch no longer match. Reload the diff.";
    let append = |out: &mut Vec<DiffLine>, old: &mut u32, new: &mut u32, line: &str| {
        out.push(DiffLine {
            kind: LineKind::Expanded,
            text: format!(" {line}"),
            old: Some(*old),
            new: Some(*new),
        });
        *old += 1;
        *new += 1;
    };
    for line in &file.lines {
        if line.kind == LineKind::Hunk {
            let mut ranges = line.text.split_whitespace().skip(1);
            let from = ranges.next().and_then(start).ok_or(stale)?;
            let to = ranges.next().and_then(start).ok_or(stale)?;
            if to
                .checked_sub(new)
                .zip(from.checked_sub(old))
                .is_none_or(|(a, b)| a != b)
            {
                return Err(stale.into());
            }
            while new < to {
                append(&mut out, &mut old, &mut new, content.next().ok_or(stale)?);
            }
            continue;
        }
        if let Some(n) = line.new {
            if n != new || content.next() != line.text.get(1..) {
                return Err(stale.into());
            }
            new += 1;
        }
        if let Some(n) = line.old {
            if n != old {
                return Err(stale.into());
            }
            old += 1;
        }
        out.push(line.clone());
    }
    for line in content {
        append(&mut out, &mut old, &mut new, line);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_diff;

    #[test]
    fn fills_leading_middle_and_trailing_gaps_without_creating_review_anchors() {
        let files = parse_diff(
            "diff --git a/f b/f\n@@ -2 +2,2 @@\n-old\n+new\n+extra\n@@ -5 +6,0 @@\n-removed\n",
        );
        // Deleting old line 5 leaves the new side anchored at line 5.
        let files2 = parse_diff(
            "diff --git a/f b/f\n@@ -2 +2,2 @@\n-old\n+new\n+extra\n@@ -5 +5,0 @@\n-removed\n",
        );
        let text = "first\nnew\nextra\nmiddle 1\nmiddle 2\nlast\n";
        assert!(expand(&files[0], text).is_err());
        let expanded = expand(&files2[0], text).unwrap();
        let gaps: Vec<_> = expanded
            .iter()
            .filter(|l| l.kind == LineKind::Expanded)
            .map(|l| (l.text.as_str(), l.old, l.new))
            .collect();
        assert_eq!(
            gaps,
            [
                (" first", Some(1), Some(1)),
                (" middle 1", Some(3), Some(4)),
                (" middle 2", Some(4), Some(5)),
                (" last", Some(6), Some(6))
            ]
        );
        assert!(expand(&files2[0], "different\ncontent").is_err());
    }
}
