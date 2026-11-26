//! What a worktree has changed since `HEAD`: tracked edits plus untracked
//! files, as the same [`DiffFile`]s a pull request's diff uses.
//!
//! Read-only, and safe next to a running agent: git takes no optional locks
//! and starts no fsmonitor.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::github::{DiffFile, DiffLine, FileStatus, LineKind, parse_diff};

/// Untracked files listed; a fresh `node_modules` should not fill the tree.
const MAX_UNTRACKED: usize = 200;
/// Untracked files larger than this show as a name only.
const MAX_UNTRACKED_BYTES: u64 = 256 * 1024;
const MAX_UNTRACKED_LINES: usize = 2000;

fn git(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn run(cmd: &mut Command) -> Result<Vec<u8>, String> {
    let out = cmd.output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().next().unwrap_or("git failed").to_owned())
    }
}

pub(crate) fn root(dir: &Path) -> Result<std::path::PathBuf, String> {
    let top = run(git(dir).args(["rev-parse", "--show-toplevel"]))?;
    Path::new(String::from_utf8_lossy(&top).trim())
        .canonicalize()
        .map_err(|e| e.to_string())
}

/// The worktree at `dir` against `HEAD`, sorted by path.
pub fn local(dir: &Path) -> Result<Vec<DiffFile>, String> {
    let top = root(dir)?;
    let top = top.as_path();
    // A repository without commits has no HEAD to compare with.
    let has_head = run(git(top).args(["rev-parse", "--verify", "--quiet", "HEAD"])).is_ok();
    let mut diff = git(top);
    diff.args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M"]);
    if has_head {
        diff.arg("HEAD");
    }
    let mut files = parse_diff(&String::from_utf8_lossy(&run(&mut diff)?));
    let untracked = run(git(top).args(["ls-files", "--others", "--exclude-standard", "-z"]))?;
    files.extend(
        untracked
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .take(MAX_UNTRACKED)
            .map(|p| untracked_file(top, &String::from_utf8_lossy(p))),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn untracked_file(top: &Path, path: &str) -> DiffFile {
    let mut file = DiffFile {
        path: path.to_owned(),
        status: FileStatus::Added,
        ..DiffFile::default()
    };
    let full = top.join(path);
    let small =
        std::fs::metadata(&full).is_ok_and(|m| m.is_file() && m.len() <= MAX_UNTRACKED_BYTES);
    let Some(text) = small.then(|| std::fs::read_to_string(&full).ok()).flatten() else {
        file.lines.push(DiffLine {
            kind: LineKind::Meta,
            text: "Untracked — binary or too large to show".into(),
            old: None,
            new: None,
        });
        return file;
    };
    for (i, line) in text.lines().enumerate() {
        file.added = file.added.saturating_add(1);
        if i < MAX_UNTRACKED_LINES {
            file.lines.push(DiffLine {
                kind: LineKind::Add,
                text: format!("+{line}"),
                old: None,
                new: u32::try_from(i + 1).ok(),
            });
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::io::Result<std::path::PathBuf> {
        let dir =
            std::env::temp_dir().join(format!("artalde-changes-{name}-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn tracked_edits_and_untracked_files() -> Result<(), Box<dyn std::error::Error>> {
        let dir = &scratch("edits")?;
        sh(dir, &["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n")?;
        std::fs::write(dir.join("gone.txt"), "x\n")?;
        sh(dir, &["add", "."]);
        sh(dir, &["commit", "-qm", "init"]);
        std::fs::write(dir.join("a.txt"), "one\n2\n")?;
        std::fs::remove_file(dir.join("gone.txt"))?;
        std::fs::write(dir.join("new.txt"), "hello\nworld\n")?;
        std::fs::write(dir.join("blob.bin"), [0xff, 0xfe, 0x00])?;

        let files = local(dir)?;
        let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, ["a.txt", "blob.bin", "gone.txt", "new.txt"]);
        let a = &files[0];
        assert_eq!((a.added, a.removed, a.status), (1, 1, FileStatus::Modified));
        assert_eq!(files[1].lines[0].kind, LineKind::Meta);
        assert_eq!(files[2].status, FileStatus::Deleted);
        let new = &files[3];
        assert_eq!((new.added, new.status), (2, FileStatus::Added));
        assert_eq!(new.lines[1].text, "+world");
        assert_eq!(new.lines[1].new, Some(2));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn a_repository_without_commits() -> Result<(), Box<dyn std::error::Error>> {
        let dir = &scratch("empty")?;
        sh(dir, &["init", "-q"]);
        std::fs::write(dir.join("first.txt"), "a\n")?;
        let files = local(dir)?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn not_a_repository_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = &scratch("plain")?;
        assert!(local(dir).is_err());
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
