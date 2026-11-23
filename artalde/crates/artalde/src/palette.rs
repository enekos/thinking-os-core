use std::collections::HashSet;

use artalde_core::command::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteKind {
    Action,
    Command,
    Agent,
    Terminal,
    Repository,
    PullRequest,
    Layout,
    Theme,
}

impl PaletteKind {
    pub(crate) fn section(self) -> &'static str {
        match self {
            Self::Action => "Actions",
            Self::Command => "Commands",
            Self::Agent => "Agents",
            Self::Terminal => "Terminals",
            Self::Repository => "Repositories",
            Self::PullRequest => "Pull requests",
            Self::Layout => "Layouts",
            Self::Theme => "Themes",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PaletteEntry {
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) hint: String,
    pub(crate) command: Command,
    pub(crate) kind: PaletteKind,
    pub(crate) keywords: String,
    pub(crate) boost: i32,
    pub(crate) recent: Option<usize>,
}

impl PaletteEntry {
    pub(crate) fn new(
        kind: PaletteKind,
        title: impl Into<String>,
        detail: impl Into<String>,
        command: Command,
    ) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            hint: String::new(),
            command,
            kind,
            keywords: String::new(),
            boost: 0,
            recent: None,
        }
    }

    pub(crate) fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    pub(crate) fn keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = keywords.into();
        self
    }

    pub(crate) fn boost(mut self, boost: i32) -> Self {
        self.boost = boost;
        self
    }

    pub(crate) fn section(&self, query: &PaletteQuery) -> &'static str {
        if query.text.is_empty() && self.recent.is_some() {
            "Recent"
        } else {
            self.kind.section()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteScope {
    All,
    Commands,
    Agents,
    PullRequests,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaletteQuery {
    pub(crate) scope: PaletteScope,
    pub(crate) text: String,
}

impl PaletteQuery {
    pub(crate) fn parse(raw: &str) -> Self {
        let raw = raw.trim_start();
        let (scope, text) = match raw.chars().next() {
            Some('>') => (PaletteScope::Commands, &raw[1..]),
            Some('@') => (PaletteScope::Agents, &raw[1..]),
            Some('#') => (PaletteScope::PullRequests, &raw[1..]),
            _ => (PaletteScope::All, raw),
        };
        Self {
            scope,
            text: text.trim().to_owned(),
        }
    }

    fn includes(&self, kind: PaletteKind) -> bool {
        match self.scope {
            PaletteScope::All => true,
            PaletteScope::Commands => matches!(
                kind,
                PaletteKind::Action
                    | PaletteKind::Command
                    | PaletteKind::Layout
                    | PaletteKind::Theme
            ),
            PaletteScope::Agents => matches!(kind, PaletteKind::Agent | PaletteKind::Terminal),
            PaletteScope::PullRequests => kind == PaletteKind::PullRequest,
        }
    }
}

pub(crate) fn rank(entries: Vec<PaletteEntry>, raw: &str) -> Vec<PaletteEntry> {
    let query = PaletteQuery::parse(raw);
    let mut scored: Vec<(i32, PaletteEntry)> = entries
        .into_iter()
        .filter(|entry| query.includes(entry.kind))
        .filter_map(|entry| score(&query, &entry).map(|score| (score, entry)))
        .collect();
    scored.sort_by(|(score_a, a), (score_b, b)| {
        score_b
            .cmp(score_a)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
            .then_with(|| a.command.to_string().cmp(&b.command.to_string()))
    });
    let mut seen = HashSet::new();
    scored
        .into_iter()
        .filter(|(_, entry)| seen.insert(entry.command.clone()))
        .map(|(_, entry)| entry)
        .collect()
}

fn score(query: &PaletteQuery, entry: &PaletteEntry) -> Option<i32> {
    let recent = entry.recent.map_or(0, |index| {
        24_000 - i32::try_from(index).unwrap_or(i32::MAX) * 100
    });
    if query.text.is_empty() {
        return Some(entry.boost + recent);
    }
    let command = entry.command.to_string();
    let fields = [
        (entry.title.as_str(), 0),
        (entry.keywords.as_str(), -1_000),
        (entry.detail.as_str(), -1_800),
        (command.as_str(), -2_400),
    ];
    let mut total = entry.boost + recent / 12;
    let mut tokens = 0;
    for token in query.text.split_whitespace() {
        let best = fields
            .iter()
            .filter_map(|(text, penalty)| fuzzy(token, text).map(|score| score + penalty))
            .max()?;
        total += best;
        tokens += 1;
    }
    Some(total + tokens * 250)
}

/// Ordered, deterministic fuzzy matching with exact and word-prefix matches first.
fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_lowercase();
    if text == query {
        return Some(16_000);
    }
    if text.starts_with(&query) {
        return Some(11_000 - i32::try_from(text.len().min(1_000)).unwrap_or(1_000));
    }
    if let Some(index) = text.match_indices(&query).find_map(|(index, _)| {
        text[..index]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
            .then_some(index)
    }) {
        return Some(8_000 - i32::try_from(index.min(1_000)).unwrap_or(1_000));
    }
    if let Some(index) = text.find(&query) {
        return Some(6_000 - i32::try_from(index.min(1_000)).unwrap_or(1_000));
    }
    let mut chars = text.chars();
    let mut gap = 0_i32;
    for expected in query.chars() {
        loop {
            match chars.next() {
                Some(actual) if actual == expected => break,
                Some(_) => gap += 1,
                None => return None,
            }
        }
    }
    Some(1_500 - gap * 12 - i32::try_from(text.len().min(500)).unwrap_or(500))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: PaletteKind, title: &str, detail: &str, command: &str) -> PaletteEntry {
        PaletteEntry::new(kind, title, detail, Command::parse(command))
    }

    #[test]
    fn scopes_have_a_compact_syntax() {
        assert_eq!(
            PaletteQuery::parse("> repo"),
            PaletteQuery {
                scope: PaletteScope::Commands,
                text: "repo".into(),
            }
        );
        assert_eq!(PaletteQuery::parse("@ claude").scope, PaletteScope::Agents);
        assert_eq!(
            PaletteQuery::parse("#123").scope,
            PaletteScope::PullRequests
        );
        assert_eq!(PaletteQuery::parse("repo").text, "repo");
    }

    #[test]
    fn exact_and_prefix_matches_win() {
        let entries = vec![
            entry(PaletteKind::Command, "Open repository", "", "repo.open"),
            entry(
                PaletteKind::Repository,
                "Repo",
                "acme/repo",
                "repo.open:acme/repo",
            ),
            entry(
                PaletteKind::Layout,
                "Repository radar",
                "",
                "layout.select:radar",
            ),
        ];
        let ranked = rank(entries, "repo");
        assert_eq!(ranked[0].title, "Repo");
        assert_eq!(ranked[1].title, "Repository radar");
    }

    #[test]
    fn tokens_can_match_across_title_and_detail() {
        let entries = vec![
            entry(PaletteKind::Agent, "Fix auth", "acme/api", "agent.select:1"),
            entry(
                PaletteKind::Agent,
                "Write docs",
                "acme/web",
                "agent.select:2",
            ),
        ];
        let ranked = rank(entries, "auth api");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].title, "Fix auth");
    }

    #[test]
    fn scopes_filter_before_ranking() {
        let entries = vec![
            entry(PaletteKind::Command, "Focus agent", "", "session.focus"),
            entry(
                PaletteKind::Agent,
                "Focus agent",
                "acme/api",
                "agent.select:1",
            ),
        ];
        let ranked = rank(entries, "@focus");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].kind, PaletteKind::Agent);
    }

    #[test]
    fn command_scope_keeps_actions_and_display_choices() {
        let entries = vec![
            entry(
                PaletteKind::Action,
                "Refresh repository",
                "",
                "repo.refresh",
            ),
            entry(
                PaletteKind::Layout,
                "Repository radar",
                "",
                "layout.select:radar",
            ),
            entry(PaletteKind::Theme, "Graphite", "", "theme.select:graphite"),
            entry(
                PaletteKind::Repository,
                "acme/api",
                "",
                "repo.open:acme/api",
            ),
            entry(PaletteKind::Agent, "Refresh docs", "", "agent.select:1"),
            entry(
                PaletteKind::PullRequest,
                "Refresh API",
                "acme/api#12",
                "pr.open:acme/api#12",
            ),
        ];
        let ranked = rank(entries, ">refresh");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].kind, PaletteKind::Action);

        let entries = vec![
            entry(
                PaletteKind::Layout,
                "Repository radar",
                "",
                "layout.select:radar",
            ),
            entry(PaletteKind::Theme, "Graphite", "", "theme.select:graphite"),
            entry(
                PaletteKind::Repository,
                "acme/api",
                "",
                "repo.open:acme/api",
            ),
        ];
        assert_eq!(rank(entries, ">").len(), 2);
    }

    #[test]
    fn empty_query_surfaces_recents() {
        let mut recent = entry(PaletteKind::Command, "Open settings", "", "settings.open");
        recent.recent = Some(0);
        let entries = vec![
            entry(
                PaletteKind::Repository,
                "acme/api",
                "",
                "repo.open:acme/api",
            )
            .boost(1_000),
            recent,
        ];
        let ranked = rank(entries, "");
        assert_eq!(ranked[0].title, "Open settings");
        assert_eq!(ranked[0].section(&PaletteQuery::parse("")), "Recent");
    }

    #[test]
    fn direct_destinations_do_not_duplicate_known_entries() {
        let known = entry(
            PaletteKind::PullRequest,
            "#11 Fix auth",
            "acme/api",
            "pr.open:acme/api#11",
        );
        let direct = entry(
            PaletteKind::PullRequest,
            "Open pull request #11",
            "acme/api",
            "pr.open:acme/api#11",
        )
        .boost(70_000);
        let ranked = rank(vec![known, direct], "#11");
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].title, "Open pull request #11");
    }
}
