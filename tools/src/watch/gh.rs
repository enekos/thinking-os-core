use crate::watch::model::{Checks, Lifecycle, Pr, Review};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::process::Command;
use std::thread;
use std::time::Duration;

const QUERY: &str = r#"
query($q:String!, $after:String) {
  hits: search(query:$q, type:ISSUE, first:40, after:$after) {
    pageInfo { hasNextPage endCursor }
    nodes { ...pr }
  }
}
fragment pr on PullRequest {
  number title url state isDraft createdAt updatedAt mergedAt reviewDecision
  headRefName baseRefName
  repository { nameWithOwner }
  author { login }
  mergedBy { login }
  latestReviews(first:8) { nodes { author { login } state } }
  commits(last:1) { nodes { commit { statusCheckRollup { state contexts(first:20) { nodes {
    __typename
    ... on CheckRun { name conclusion }
    ... on StatusContext { context state }
  } } } } } }
}
"#;

const RED: [&str; 3] = ["FAILURE", "ERROR", "TIMED_OUT"];
const WAITING: [&str; 5] = ["PENDING", "IN_PROGRESS", "QUEUED", "EXPECTED", "WAITING"];

pub use crate::kg::run_capture;

pub fn login() -> Result<String, String> {
    if let Ok(l) = std::env::var("ZAINDU_LOGIN") {
        if !l.trim().is_empty() {
            return Ok(l.trim().to_string());
        }
    }
    let out = run_capture(
        Command::new("gh").args(["api", "user", "--jq", ".login"]),
        Duration::from_secs(30),
    )?;
    let l = out.trim().to_string();
    if l.is_empty() {
        return Err("gh api user returned no login".into());
    }
    Ok(l)
}

pub const BUCKETS: [&str; 3] = ["open", "merged", "review"];

fn query_for(bucket: &str, login: &str, since_days: i64) -> String {
    match bucket {
        "merged" => format!(
            "type:pr author:{login} is:merged merged:>={}",
            crate::clock::days_ago(since_days)
        ),
        "review" => format!("type:pr review-requested:{login} is:open"),
        _ => format!("type:pr author:{login} is:open"),
    }
}

/// GitHub search stops at 1000 results; 25 pages of 40 reaches it.
const MAX_PAGES: usize = 25;

fn fetch_page(query: &str, after: Option<&str>) -> Result<Value, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_secs(3 * attempt));
        }
        let mut cmd = Command::new("gh");
        cmd.args([
            "api",
            "graphql",
            "-f",
            &format!("query={QUERY}"),
            "-f",
            &format!("q={query}"),
        ]);
        if let Some(cursor) = after {
            cmd.args(["-f", &format!("after={cursor}")]);
        }
        match run_capture(&mut cmd, Duration::from_secs(90)) {
            Ok(body) => match serde_json::from_str::<Value>(&body) {
                Ok(v) if v.pointer("/data/hits/nodes").is_some() => return Ok(v),
                Ok(v) => {
                    last = v
                        .pointer("/errors/0/message")
                        .and_then(Value::as_str)
                        .unwrap_or("response carried no hits")
                        .to_string()
                }
                Err(e) => last = format!("unparseable response: {e}"),
            },
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Every page of one search. A bucket read short of its end is an error, not
/// a shorter list: a PR missing from the window reads as "closed unmerged".
fn collect_pages(
    mut page: impl FnMut(Option<&str>) -> Result<Value, String>,
) -> Result<Vec<Value>, String> {
    let mut nodes = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let v = page(cursor.as_deref())?;
        if let Some(Value::Array(batch)) = v.pointer("/data/hits/nodes") {
            nodes.extend(batch.iter().cloned());
        }
        let more = v
            .pointer("/data/hits/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !more {
            return Ok(nodes);
        }
        cursor = v
            .pointer("/data/hits/pageInfo/endCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            return Err("more pages promised but no cursor given".into());
        }
    }
    Err(format!("more than {} results; window truncated", MAX_PAGES * 40))
}

pub fn fetch(login: &str, since_days: i64) -> Result<(Value, Vec<String>), String> {
    let mut buckets = serde_json::Map::new();
    let mut warnings = Vec::new();
    for bucket in BUCKETS {
        let query = query_for(bucket, login, since_days);
        match collect_pages(|after| fetch_page(&query, after)) {
            Ok(nodes) => {
                buckets.insert(bucket.to_string(), json!({ "nodes": nodes }));
            }
            Err(e) => warnings.push(format!("{bucket}: {e}")),
        }
    }
    if buckets.is_empty() {
        return Err(warnings.join(" | "));
    }
    Ok((json!({ "data": buckets }), warnings))
}

fn check_rows(node: &Value) -> (Option<String>, Vec<(String, String)>) {
    let rollup = node
        .pointer("/commits/nodes/0/commit/statusCheckRollup")
        .cloned()
        .unwrap_or(Value::Null);
    let state = rollup
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut rows = Vec::new();
    if let Some(items) = rollup.pointer("/contexts/nodes").and_then(Value::as_array) {
        for it in items {
            let (name, verdict) = match it.get("__typename").and_then(Value::as_str) {
                Some("CheckRun") => (it.get("name"), it.get("conclusion")),
                _ => (it.get("context"), it.get("state")),
            };
            let name = name.and_then(Value::as_str).unwrap_or("").to_string();
            let verdict = verdict.and_then(Value::as_str).unwrap_or("").to_string();
            if !name.is_empty() {
                rows.push((name, verdict));
            }
        }
    }
    (state, rows)
}

pub fn grade_checks(rollup_state: Option<&str>, rows: &[(String, String)]) -> (Checks, Vec<String>) {
    if rows.is_empty() {
        let graded = match rollup_state {
            Some(s) if RED.contains(&s) => Checks::Red,
            Some("SUCCESS") => Checks::Green,
            Some(s) if WAITING.contains(&s) => Checks::Pending,
            _ => Checks::Unknown,
        };
        return (graded, Vec::new());
    }
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, verdict) in rows {
        by_name.entry(name.as_str()).or_default().push(verdict.as_str());
    }
    let mut failing = Vec::new();
    let mut waiting = false;
    for (name, verdicts) in &by_name {
        let superseded = verdicts.iter().any(|v| *v == "SUCCESS");
        if verdicts.iter().any(|v| RED.contains(v)) && !superseded {
            failing.push((*name).to_string());
        } else if verdicts.iter().any(|v| WAITING.contains(v)) && !superseded {
            waiting = true;
        }
    }
    if !failing.is_empty() {
        return (Checks::Red, failing);
    }
    if waiting {
        return (Checks::Pending, Vec::new());
    }
    (Checks::Green, Vec::new())
}

pub fn grade_review(decision: Option<&str>, reviews: &[(String, String)]) -> (Review, Vec<String>) {
    let pick = |want: &str| -> Vec<String> {
        reviews
            .iter()
            .filter(|(_, state)| state == want)
            .map(|(who, _)| who.clone())
            .collect()
    };
    let blockers = pick("CHANGES_REQUESTED");
    if !blockers.is_empty() {
        return (Review::ChangesRequested, blockers);
    }
    if decision == Some("CHANGES_REQUESTED") {
        return (Review::ChangesRequested, Vec::new());
    }
    let approvers = pick("APPROVED");
    if !approvers.is_empty() {
        return (Review::Approved, approvers);
    }
    if decision == Some("APPROVED") {
        return (Review::Approved, Vec::new());
    }
    (Review::None, Vec::new())
}

#[cfg(test)]
pub fn parse(payload: &Value, login: &str) -> Result<Vec<Pr>, String> {
    let (prs, missing) = parse_partial(payload, login)?;
    if !missing.is_empty() {
        return Err(format!("graphql response has no {} bucket", missing.join("/")));
    }
    Ok(prs)
}

pub fn parse_partial(payload: &Value, login: &str) -> Result<(Vec<Pr>, Vec<&'static str>), String> {
    if let Some(errors) = payload.get("errors").and_then(Value::as_array) {
        let first = errors
            .first()
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown graphql error");
        return Err(first.to_string());
    }
    let mut by_id: BTreeMap<String, Pr> = BTreeMap::new();
    let mut missing = Vec::new();
    for bucket in BUCKETS {
        let nodes = match payload.pointer(&format!("/data/{bucket}/nodes")) {
            Some(Value::Array(v)) => v,
            _ => {
                missing.push(bucket);
                continue;
            }
        };
        for node in nodes {
            let number = match node.get("number").and_then(Value::as_u64) {
                Some(n) => n,
                None => continue,
            };
            let nwo = node
                .pointer("/repository/nameWithOwner")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if nwo.is_empty() {
                continue;
            }
            let author = node
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let head = node
                .get("headRefName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let title = node.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            let reviews: Vec<(String, String)> = node
                .pointer("/latestReviews/nodes")
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .map(|r| {
                            (
                                r.pointer("/author/login")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                r.get("state").and_then(Value::as_str).unwrap_or("").to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let (review, review_by) = grade_review(
                node.get("reviewDecision").and_then(Value::as_str),
                &reviews,
            );
            let (rollup, rows) = check_rows(node);
            let (checks, failing) = grade_checks(rollup.as_deref(), &rows);
            let lifecycle = match node.get("state").and_then(Value::as_str) {
                Some("MERGED") => Lifecycle::Merged,
                Some("CLOSED") => Lifecycle::Closed,
                _ => Lifecycle::Open,
            };
            let mine = author.eq_ignore_ascii_case(login);
            let id = format!("gh:{nwo}#{number}");
            let pr = Pr {
                id: id.clone(),
                repo: format!("gh:{nwo}"),
                number,
                title,
                url: node.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
                lifecycle,
                draft: node.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
                mine,
                author,
                head,
                base: node.get("baseRefName").and_then(Value::as_str).unwrap_or("").to_string(),
                updated: node
                    .get("updatedAt")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
                created: node.get("createdAt").and_then(Value::as_str).unwrap_or("").to_string(),
                merged_at: node
                    .get("mergedAt")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(10)
                    .collect(),
                merged_by: node
                    .pointer("/mergedBy/login")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                review,
                review_by,
                checks,
                failing,
                review_requested_of_me: bucket == "review" && !mine,
            };
            by_id
                .entry(id)
                .and_modify(|existing| {
                    existing.review_requested_of_me |= pr.review_requested_of_me;
                })
                .or_insert(pr);
        }
    }
    Ok((by_id.into_values().collect(), missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watch::fixtures::*;
    use crate::watch::model::Lifecycle;

    fn page(n: usize, next: Option<&str>) -> Value {
        json!({ "data": { "hits": {
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
            "nodes": (0..n).map(|i| json!({ "number": i })).collect::<Vec<_>>(),
        } } })
    }

    #[test]
    fn every_page_of_a_search_is_read() {
        let mut asked = Vec::new();
        let nodes = collect_pages(|after| {
            asked.push(after.map(str::to_string));
            Ok(match after {
                None => page(40, Some("c1")),
                Some("c1") => page(40, Some("c2")),
                _ => page(3, None),
            })
        })
        .unwrap();
        assert_eq!(nodes.len(), 83);
        assert_eq!(asked, vec![None, Some("c1".into()), Some("c2".into())]);
    }

    #[test]
    fn a_search_that_never_ends_is_an_error_not_a_short_list() {
        let err = collect_pages(|_| Ok(page(40, Some("again")))).unwrap_err();
        assert!(err.contains("truncated"), "{err}");
    }

    #[test]
    fn a_failed_page_fails_the_bucket() {
        let err = collect_pages(|after| match after {
            None => Ok(page(40, Some("c1"))),
            _ => Err("rate limited".into()),
        })
        .unwrap_err();
        assert_eq!(err, "rate limited");
    }

    #[test]
    fn a_rerun_that_passed_clears_the_earlier_failure() {
        let rows = vec![
            ("execute / lint-pr-title".to_string(), "FAILURE".to_string()),
            ("execute / lint-pr-title".to_string(), "SUCCESS".to_string()),
            ("ci/circleci: test".to_string(), "SUCCESS".to_string()),
        ];
        assert_eq!(grade_checks(Some("FAILURE"), &rows), (Checks::Green, vec![]));
    }

    #[test]
    fn a_failure_with_no_passing_rerun_is_red() {
        let rows = vec![
            ("ci/circleci: test".to_string(), "FAILURE".to_string()),
            ("ci/circleci: lint".to_string(), "SUCCESS".to_string()),
        ];
        assert_eq!(
            grade_checks(Some("FAILURE"), &rows),
            (Checks::Red, vec!["ci/circleci: test".to_string()])
        );
    }

    #[test]
    fn a_cancelled_check_is_not_a_failure() {
        let rows = vec![("e2e".to_string(), "CANCELLED".to_string())];
        assert_eq!(grade_checks(Some("FAILURE"), &rows).0, Checks::Green);
    }

    #[test]
    fn with_no_contexts_the_rollup_decides() {
        assert_eq!(grade_checks(Some("FAILURE"), &[]).0, Checks::Red);
        assert_eq!(grade_checks(Some("SUCCESS"), &[]).0, Checks::Green);
        assert_eq!(grade_checks(Some("PENDING"), &[]).0, Checks::Pending);
        assert_eq!(grade_checks(None, &[]).0, Checks::Unknown);
    }

    #[test]
    fn a_pending_check_is_not_graded_yet() {
        let rows = vec![
            ("ci/circleci: test".to_string(), "PENDING".to_string()),
            ("lint".to_string(), "SUCCESS".to_string()),
        ];
        assert_eq!(grade_checks(Some("PENDING"), &rows).0, Checks::Pending);
    }

    #[test]
    fn an_approval_counts_even_when_review_decision_is_null() {
        let reviews = vec![
            ("cursor".to_string(), "COMMENTED".to_string()),
            ("mohammed-ali-1".to_string(), "APPROVED".to_string()),
        ];
        assert_eq!(
            grade_review(None, &reviews),
            (Review::Approved, vec!["mohammed-ali-1".to_string()])
        );
    }

    #[test]
    fn changes_requested_outranks_an_approval() {
        let reviews = vec![
            ("a".to_string(), "APPROVED".to_string()),
            ("b".to_string(), "CHANGES_REQUESTED".to_string()),
        ];
        assert_eq!(
            grade_review(Some("APPROVED"), &reviews),
            (Review::ChangesRequested, vec!["b".to_string()])
        );
    }

    #[test]
    fn a_branch_protected_decision_is_enough_on_its_own() {
        assert_eq!(grade_review(Some("APPROVED"), &[]), (Review::Approved, vec![]));
        assert_eq!(grade_review(Some("REVIEW_REQUIRED"), &[]), (Review::None, vec![]));
    }

    #[test]
    fn parsing_splits_my_prs_from_the_ones_i_owe_a_review() {
        let mut theirs = node("join-com/frontend", 13352, "fix: THU-3459 archive redirect", "OPEN");
        theirs["author"]["login"] = serde_json::json!("someone-else");
        let payload = payload(
            vec![node("join-com/data", 7276, "fix: THU-3461 anchor", "OPEN")],
            vec![node("join-com/data", 7275, "fix: THU-3400 emojis", "MERGED")],
            vec![theirs],
        );
        let prs = parse(&payload, "enekos").expect("parses");
        assert_eq!(prs.len(), 3);
        let by_id = |id: &str| prs.iter().find(|p| p.id == id).unwrap().clone();

        let mine = by_id("gh:join-com/data#7276");
        assert!(mine.mine && !mine.review_requested_of_me);

        let merged = by_id("gh:join-com/data#7275");
        assert_eq!(merged.lifecycle, Lifecycle::Merged);
        assert_eq!(merged.merged_by, "mergequeue");

        let theirs = by_id("gh:join-com/frontend#13352");
        assert!(!theirs.mine && theirs.review_requested_of_me);
    }

    #[test]
    fn a_pr_in_two_buckets_is_one_node() {
        let n = node("join-com/data", 7276, "fix: THU-3461 anchor", "OPEN");
        let prs = parse(&payload(vec![n.clone()], vec![], vec![n]), "enekos").unwrap();
        assert_eq!(prs.len(), 1);
    }

    #[test]
    fn a_graphql_error_is_reported_not_swallowed() {
        let payload = serde_json::json!({"errors": [{"message": "Bad credentials"}]});
        assert_eq!(parse(&payload, "enekos"), Err("Bad credentials".into()));
    }

    #[test]
    fn a_truncated_response_is_an_error() {
        let payload = serde_json::json!({"data": {"open": {"nodes": []}}});
        assert!(parse(&payload, "enekos").is_err());
    }

    #[test]
    fn a_bucket_that_failed_is_named_not_treated_as_empty() {
        let payload = serde_json::json!({"data": {
            "open": {"nodes": [node("join-com/data", 7276, "fix: anchor", "OPEN")]},
            "review": {"nodes": []}
        }});
        let (prs, missing) = parse_partial(&payload, "enekos").unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(missing, vec!["merged"]);
    }

    #[test]
    fn each_bucket_asks_github_a_different_question() {
        assert_eq!(query_for("open", "enekos", 14), "type:pr author:enekos is:open");
        assert_eq!(query_for("review", "enekos", 14), "type:pr review-requested:enekos is:open");
        assert!(query_for("merged", "enekos", 14).starts_with("type:pr author:enekos is:merged merged:>=20"));
    }

    #[test]
    fn checks_survive_the_round_trip_from_graphql() {
        let node = with_checks(
            node("join-com/data", 7280, "feat: THU-3468 prompts", "OPEN"),
            rollup(
                "FAILURE",
                vec![
                    check_run("execute / lint-pr-title", "FAILURE"),
                    check_run("execute / lint-pr-title", "SUCCESS"),
                    status_context("ci/circleci: test", "SUCCESS"),
                ],
            ),
        );
        let prs = parse(&payload(vec![node], vec![], vec![]), "enekos").unwrap();
        assert_eq!(prs[0].checks, Checks::Green);
        assert!(prs[0].failing.is_empty());
    }
}
