//! Live, read-only GitHub data for a PR the user has opened. Patches never enter the graph.
use crate::kg::run_capture;
use serde_json::Value;
use std::{process::Command, time::Duration};

const SUMMARY: &str = r#"query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      state isDraft reviewDecision headRefName baseRefName additions deletions changedFiles
      commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
    }
  }
}"#;

pub(super) struct PullRequest<'a> {
    owner: &'a str,
    repo: &'a str,
    number: u32,
}

impl<'a> PullRequest<'a> {
    pub fn from_id(id: &'a str) -> Option<Self> {
        let (slug, number) = id.strip_prefix("gh:")?.split_once('#')?;
        let (owner, repo) = slug.split_once('/')?;
        let valid = |s: &str| {
            !s.is_empty()
                && !matches!(s, "." | "..")
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        };
        if !valid(owner) || !valid(repo) || !number.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let number = number.parse().ok()?;
        (number > 0).then_some(Self {
            owner,
            repo,
            number,
        })
    }

    pub fn status(&self) -> Result<Value, String> {
        github(&[
            "graphql",
            "-f",
            &format!("query={SUMMARY}"),
            "-f",
            &format!("owner={}", self.owner),
            "-f",
            &format!("repo={}", self.repo),
            "-F",
            &format!("number={}", self.number),
            "--jq",
            ".data.repository.pullRequest | select(. != null) | {state,isDraft,reviewDecision,headRefName,baseRefName,additions,deletions,changedFiles,checks:.commits.nodes[0].commit.statusCheckRollup.state}",
        ])
    }

    pub fn files(&self, page: usize) -> Result<Value, String> {
        // GitHub omits patch for binary and some large files. Limit one preview as well
        // as the page size so a generated file cannot fill the reader with megabytes.
        let path = format!(
            "repos/{}/{}/pulls/{}/files?per_page=10&page={page}",
            self.owner, self.repo, self.number
        );
        github(&[
            &path,
            "--jq",
            "map({filename,previous_filename,status,additions,deletions,patch:(.patch[:65536]),truncated:((.patch|length)>65536)})",
        ])
    }
}

fn github(args: &[&str]) -> Result<Value, String> {
    let mut cmd = Command::new("gh");
    cmd.args(["api", "--hostname", "github.com"])
        .args(args)
        .env("GH_PROMPT_DISABLED", "1");
    let text = run_capture(&mut cmd, Duration::from_secs(15))
        .map_err(|_| "Check the network and GitHub CLI access to this repository.".to_owned())?;
    serde_json::from_str(&text)
        .map_err(|_| "GitHub returned no readable data for this pull request.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_canonical_github_pr_nodes_can_select_a_remote_resource() {
        let pr = PullRequest::from_id("gh:org-name/repo.name_2#42").unwrap();
        assert_eq!(
            (pr.owner, pr.repo, pr.number),
            ("org-name", "repo.name_2", 42)
        );
        for id in [
            "gh:a/b",
            "gh:a/b#0",
            "gh:a/b#-1",
            "gh:a/b/c#1",
            "gh:/b#2",
            "gh:a/..#1",
            "gh:a/b?x#1",
            "gh:a/b#1/extra",
            "gh-issue:a/b#1",
        ] {
            assert!(PullRequest::from_id(id).is_none(), "{id}");
        }
    }
}
