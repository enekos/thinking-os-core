//! Repository workbench: local work, pull-request risk and delivery state.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use artalde_core::command::Command;
use artalde_core::github::{self, Deployment, PrRef, PrRow, Release, RepoDetail};
use artalde_core::model::{Session, Space};
use artalde_core::text::ago;
use eframe::egui::{self, Color32, RichText};

use crate::app::App;
use crate::kit::{self, Col};
use crate::style;
use crate::views::Tab;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepoSub {
    #[default]
    Overview,
    Pulls,
    Deployments,
    Releases,
}

#[derive(Default)]
pub struct RepoView {
    pub detail: Option<Arc<RepoDetail>>,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded_at: Option<Instant>,
    pub activity_loading: bool,
    pub activity_error: Option<String>,
    pub activity_loaded: bool,
    pub sub: RepoSub,
    pub filter: String,
}

#[derive(Default)]
struct RepoHealth {
    failing: usize,
    review: usize,
    drafts: usize,
}

fn health(detail: &RepoDetail) -> RepoHealth {
    RepoHealth {
        failing: detail
            .open
            .iter()
            .filter(|pr| matches!(pr.checks.as_str(), "FAILURE" | "ERROR"))
            .count(),
        review: detail
            .open
            .iter()
            .filter(|pr| pr.review.is_empty() || pr.review == "REVIEW_REQUIRED")
            .count(),
        drafts: detail.open.iter().filter(|pr| pr.draft).count(),
    }
}

fn deploy_color(theme: &artalde_core::theme::Theme, deployment: &Deployment) -> Color32 {
    let status = if deployment.status.is_empty() {
        deployment.state.as_str()
    } else {
        deployment.status.as_str()
    };
    style::c(match status {
        "SUCCESS" | "ACTIVE" => theme.done,
        "FAILURE" | "ERROR" => theme.danger,
        "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" => theme.working,
        _ => theme.dim,
    })
}

fn deploy_label(deployment: &Deployment) -> String {
    let status = if deployment.status.is_empty() {
        deployment.state.as_str()
    } else {
        deployment.status.as_str()
    };
    match status {
        "SUCCESS" => "live".into(),
        "IN_PROGRESS" => "deploying".into(),
        "INACTIVE" => "replaced".into(),
        other => other.to_lowercase().replace('_', " "),
    }
}

fn pr_attention(pr: &PrRow) -> u8 {
    if matches!(pr.checks.as_str(), "FAILURE" | "ERROR") {
        4
    } else if pr.review == "CHANGES_REQUESTED" {
        3
    } else if pr.review.is_empty() || pr.review == "REVIEW_REQUIRED" {
        2
    } else if pr.draft {
        1
    } else {
        0
    }
}

fn open_count(detail: &RepoDetail) -> usize {
    if detail.open_total == 0 {
        detail.open.len()
    } else {
        usize::try_from(detail.open_total).unwrap_or(usize::MAX)
    }
}

impl App {
    pub(crate) fn load_repo(&mut self, repo: &str) {
        if self.repos.get(repo).is_some_and(|view| view.loading) {
            return;
        }
        let sent = self.gh.send(crate::gh::Req::Repo(repo.to_owned()));
        let view = self.repos.entry(repo.to_owned()).or_default();
        match sent {
            Ok(()) => {
                view.loading = true;
                view.error = None;
            }
            Err(error) => view.error = Some(error),
        }
    }

    pub(crate) fn load_repo_activity(&mut self, repo: &str) {
        if self
            .repos
            .get(repo)
            .is_some_and(|view| view.activity_loading)
        {
            return;
        }
        let sent = self.gh.send(crate::gh::Req::RepoActivity(repo.to_owned()));
        let view = self.repos.entry(repo.to_owned()).or_default();
        match sent {
            Ok(()) => {
                view.activity_loading = true;
                view.activity_error = None;
            }
            Err(error) => view.activity_error = Some(error),
        }
    }

    fn repo_space(&self, repo: &str) -> Option<&Space> {
        self.snap
            .spaces
            .iter()
            .find(|space| space.slug.as_deref() == Some(repo))
    }

    pub(crate) fn local_space(&self, repo: &str) -> Option<PathBuf> {
        self.repo_space(repo).map(|space| space.root.clone())
    }

    fn repo_sessions(&self, repo: &str) -> Vec<Session> {
        let Some(space) = self.repo_space(repo) else {
            return Vec::new();
        };
        self.snap
            .sessions
            .iter()
            .filter(|session| {
                session.repo.as_deref() == Some(&space.repo)
                    || session.worktree.as_ref().is_some_and(|worktree| {
                        space.worktrees.iter().any(|item| &item.path == worktree)
                    })
            })
            .cloned()
            .collect()
    }

    pub(crate) fn repo_view(&mut self, ui: &mut egui::Ui, repo: &str) {
        let should_load = self
            .repos
            .get(repo)
            .is_none_or(|view| view.detail.is_none() && !view.loading && view.error.is_none());
        if should_load {
            self.load_repo(repo);
        }
        let should_load_activity = self.repos.get(repo).is_some_and(|view| {
            view.detail.is_some()
                && !view.activity_loaded
                && !view.activity_loading
                && view.activity_error.is_none()
        });
        if should_load_activity {
            self.load_repo_activity(repo);
        }

        let mut view = self.repos.remove(repo).unwrap_or_default();
        let placeholder = RepoDetail {
            name: repo.to_owned(),
            url: format!("https://github.com/{repo}"),
            ..RepoDetail::default()
        };
        let detail_snapshot = view.detail.clone();
        let detail = detail_snapshot.as_deref().unwrap_or(&placeholder);
        let has_detail = view.detail.is_some();
        let theme = self.theme.clone();
        let mut reload = false;

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&detail.name).font(crate::fonts::display(22.0)));
            if !detail.default_branch.is_empty() {
                kit::pill(ui, &theme, &detail.default_branch, style::c(theme.dim));
            }
            if detail.private {
                kit::pill(ui, &theme, "private", style::c(theme.dim));
            }
            if view.loading {
                ui.spinner();
            } else if let Some(at) = view.loaded_at {
                kit::text(
                    ui,
                    &format!("updated {}", ago(at.elapsed().as_secs())),
                    10.5,
                    style::c(theme.faint),
                );
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(path) = self.local_space(repo) {
                let path = path.display().to_string();
                if kit::button(ui, &theme, "New agent", None).clicked() {
                    self.queue.push(Command::new("spawn.open", Some(&path)));
                }
                if kit::button(ui, &theme, "Editor", None)
                    .on_hover_text(&path)
                    .clicked()
                {
                    self.queue.push(Command::new("path.edit", Some(&path)));
                }
            }
            if kit::button(ui, &theme, "GitHub ↗", None).clicked() {
                self.queue.push(Command::new("url.open", Some(&detail.url)));
            }
            if kit::button(ui, &theme, "Reload", Some("⌘R")).clicked() {
                reload = true;
            }
            if kit::button(ui, &theme, "Clone", None).clicked() {
                ui.ctx().copy_text(format!("gh repo clone {}", detail.name));
            }
        });
        if !detail.description.is_empty() {
            kit::text(ui, &detail.description, 12.5, style::c(theme.dim));
        }
        if let Some(error) = &view.error {
            ui.horizontal_wrapped(|ui| {
                kit::text(ui, error, 11.5, style::c(theme.danger));
                if kit::button(ui, &theme, "Retry", None).clicked() {
                    reload = true;
                }
            });
        }
        ui.add_space(kit::S2);

        let subs = [
            RepoSub::Overview,
            RepoSub::Pulls,
            RepoSub::Deployments,
            RepoSub::Releases,
        ];
        let labels = vec![
            ("Overview".to_owned(), None),
            (
                "Pull requests".to_owned(),
                has_detail.then(|| open_count(detail)),
            ),
            (
                "Deployments".to_owned(),
                view.activity_loaded.then_some(detail.deployments.len()),
            ),
            (
                "Releases".to_owned(),
                view.activity_loaded.then_some(detail.releases.len()),
            ),
        ];
        let selected = subs
            .iter()
            .position(|sub| *sub == view.sub)
            .unwrap_or_default();
        if let Some(index) = kit::tabs(ui, &theme, &labels, selected) {
            view.sub = subs[index.min(subs.len() - 1)];
        }
        ui.add_space(kit::S2);

        let now = artalde_core::now();
        egui::ScrollArea::vertical()
            .id_salt(("repo", repo))
            .auto_shrink([false, false])
            .show(ui, |ui| match view.sub {
                RepoSub::Overview => {
                    self.repo_overview(ui, repo, detail, &mut view, has_detail, now)
                }
                RepoSub::Pulls => self.repo_pulls(ui, detail, &mut view, has_detail, now),
                RepoSub::Deployments => self.repo_deployments(ui, detail, &mut view, now),
                RepoSub::Releases => self.repo_releases(ui, detail, &view, now),
            });
        self.repos.insert(repo.to_owned(), view);
        if reload {
            self.load_repo(repo);
            self.load_repo_activity(repo);
        }
    }

    fn repo_overview(
        &mut self,
        ui: &mut egui::Ui,
        repo: &str,
        detail: &RepoDetail,
        view: &mut RepoView,
        has_detail: bool,
        now: u64,
    ) {
        let theme = self.theme.clone();
        let health = health(detail);
        let sessions = self.repo_sessions(repo);
        let filter_repo = self
            .repo_space(repo)
            .map(|space| space.repo.clone())
            .unwrap_or_else(|| repo.to_owned());
        let metrics = [
            ("Open", Some(open_count(detail)), ""),
            ("CI failing", Some(health.failing), "ci:red"),
            ("Review required", Some(health.review), "review:none"),
            ("Drafts", Some(health.drafts), "is:draft"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (label, count, filter) in metrics {
                let badge = if has_detail {
                    count.map(|count| count.to_string())
                } else {
                    Some("…".into())
                };
                if kit::pill_tab(
                    ui,
                    &theme,
                    label,
                    badge.as_deref(),
                    false,
                    style::c(theme.accent),
                )
                .clicked()
                {
                    view.sub = RepoSub::Pulls;
                    view.filter = filter.to_owned();
                }
            }
            if kit::pill_tab(
                ui,
                &theme,
                "Local agents",
                Some(&sessions.len().to_string()),
                false,
                style::c(theme.accent),
            )
            .clicked()
            {
                self.queue
                    .push(Command::new("space.filter", Some(&filter_repo)));
            }
        });
        ui.add_space(kit::S3);

        let wide = ui.available_width() >= 980.0;
        if wide {
            let right = 310.0;
            let left = (ui.available_width() - right - kit::S4).max(360.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.repo_workspace(ui, repo, now);
                        ui.add_space(kit::S3);
                        self.repo_pr_preview(ui, detail, has_detail, now);
                    },
                );
                ui.add_space(kit::S4);
                ui.allocate_ui_with_layout(
                    egui::vec2(right, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.repo_delivery_summary(ui, detail, view, now),
                );
            });
        } else {
            self.repo_workspace(ui, repo, now);
            ui.add_space(kit::S3);
            self.repo_pr_preview(ui, detail, has_detail, now);
            ui.add_space(kit::S3);
            self.repo_delivery_summary(ui, detail, view, now);
        }
    }

    fn repo_workspace(&mut self, ui: &mut egui::Ui, repo: &str, now: u64) {
        let theme = self.theme.clone();
        let Some(space) = self.repo_space(repo).cloned() else {
            kit::card(ui, &theme, "Local workspace", None, |ui| {
                kit::text(
                    ui,
                    "No local checkout is active in Artalde. Clone it or open an agent there to see branches and worktrees.",
                    12.0,
                    style::c(theme.dim),
                );
            });
            return;
        };
        let sessions = self.repo_sessions(repo);
        let dirty = space.worktrees.iter().map(|item| item.dirty).sum::<u32>();
        let ahead = space.worktrees.iter().map(|item| item.ahead).sum::<u32>();
        let behind = space.worktrees.iter().map(|item| item.behind).sum::<u32>();
        let summary = format!(
            "{} worktrees · {dirty} changed · {ahead} ahead · {behind} behind",
            space.worktrees.len()
        );
        kit::card(ui, &theme, "Local workspace", Some(&summary), |ui| {
            kit::mono(
                ui,
                &space.root.display().to_string(),
                10.5,
                style::c(theme.dim),
            );
            ui.add_space(kit::S1);
            for worktree in space.worktrees.iter().take(4) {
                let branch = worktree.branch.as_deref().unwrap_or("detached");
                let detail = format!(
                    "{} changed · {} ahead · {} behind",
                    worktree.dirty, worktree.ahead, worktree.behind
                );
                let response = kit::row(
                    ui,
                    &theme,
                    ("workspace", &worktree.path),
                    kit::ROW,
                    false,
                    &[Col::Px(14.0), Col::Flex, Col::Px(142.0)],
                    |column, ui| match column {
                        0 => kit::icon(ui, kit::Icon::Branch, 13.0, style::c(theme.dim)),
                        1 => {
                            ui.vertical(|ui| {
                                kit::mono(ui, branch, 11.5, style::c(theme.text));
                                kit::text(
                                    ui,
                                    &worktree.path.display().to_string(),
                                    10.0,
                                    style::c(theme.faint),
                                );
                            });
                        }
                        _ => kit::right(ui, &detail, 10.5, style::c(theme.dim)),
                    },
                );
                if response
                    .on_hover_text(worktree.path.display().to_string())
                    .clicked()
                {
                    self.queue.push(Command::new(
                        "path.edit",
                        Some(&worktree.path.display().to_string()),
                    ));
                }
            }
        });
        if sessions.is_empty() {
            return;
        }
        ui.add_space(kit::S2);
        kit::card(
            ui,
            &theme,
            "Agents in this repository",
            Some(&sessions.len().to_string()),
            |ui| {
                for session in sessions.into_iter().take(5) {
                    let color = style::state(&theme, session.rec.state);
                    let response = kit::row(
                        ui,
                        &theme,
                        ("repo-agent", &session.rec.id),
                        kit::ROW2,
                        session.rec.id == self.selected.as_deref().unwrap_or_default(),
                        &[Col::Px(14.0), Col::Flex, Col::Px(86.0)],
                        |column, ui| match column {
                            0 => kit::dot(ui, color, 4.0),
                            1 => {
                                ui.vertical(|ui| {
                                    kit::text(ui, &session.name(), 12.0, style::c(theme.text));
                                    kit::text(
                                        ui,
                                        session.branch.as_deref().unwrap_or("no branch"),
                                        10.5,
                                        style::c(theme.dim),
                                    );
                                });
                            }
                            _ => kit::right(
                                ui,
                                &format!(
                                    "{} · {}",
                                    session.rec.state.as_str(),
                                    ago(now.saturating_sub(session.rec.since))
                                ),
                                10.5,
                                color,
                            ),
                        },
                    );
                    if response.clicked() {
                        self.queue
                            .push(Command::new("agent.select", Some(&session.rec.id)));
                    }
                }
            },
        );
    }

    fn repo_pr_preview(
        &mut self,
        ui: &mut egui::Ui,
        detail: &RepoDetail,
        has_detail: bool,
        now: u64,
    ) {
        let theme = self.theme.clone();
        let mut pulls: Vec<&PrRow> = detail.open.iter().collect();
        pulls.sort_by(|a, b| {
            pr_attention(b)
                .cmp(&pr_attention(a))
                .then_with(|| b.updated.cmp(&a.updated))
        });
        let attention = pulls.iter().any(|pr| pr_attention(pr) > 0);
        if attention {
            pulls.retain(|pr| pr_attention(pr) > 0);
        }
        kit::section(
            ui,
            &theme,
            if attention {
                "Needs attention"
            } else {
                "Recently updated"
            },
            has_detail.then_some(pulls.len()),
            |_| {},
        );
        if !has_detail {
            ui.horizontal(|ui| {
                ui.spinner();
                kit::text(ui, "Loading pull requests…", 12.0, style::c(theme.dim));
            });
            return;
        }
        if pulls.is_empty() {
            kit::text(
                ui,
                "No open pull requests need attention.",
                12.0,
                style::c(theme.dim),
            );
        }
        for pr in pulls.into_iter().take(6) {
            self.repo_pr_row(ui, pr, false, now);
        }
    }

    fn repo_delivery_summary(
        &mut self,
        ui: &mut egui::Ui,
        detail: &RepoDetail,
        view: &mut RepoView,
        now: u64,
    ) {
        let theme = self.theme.clone();
        kit::card(ui, &theme, "Environments", None, |ui| {
            if view.activity_loading && !view.activity_loaded {
                ui.horizontal(|ui| {
                    ui.spinner();
                    kit::text(ui, "Loading activity…", 11.5, style::c(theme.dim));
                });
            }
            if let Some(error) = view.activity_error.clone() {
                ui.horizontal_wrapped(|ui| {
                    kit::text(ui, &error, 10.5, style::c(theme.danger));
                    if kit::button(ui, &theme, "Retry", None).clicked() {
                        view.activity_error = None;
                        view.activity_loaded = false;
                    }
                });
            }
            for deployment in detail.environments().into_iter().take(5) {
                self.repo_deployment_row(ui, deployment, now, true);
            }
            if view.activity_loaded && detail.deployments.is_empty() {
                kit::text(ui, "No GitHub deployments", 11.5, style::c(theme.dim));
            }
        });
        if let Some(release) = detail.releases.first() {
            ui.add_space(kit::S2);
            kit::card(
                ui,
                &theme,
                "Latest release",
                Some(&github::age(&release.published, now)),
                |ui| self.repo_release_summary(ui, release, now, true),
            );
        }
        if !detail.merged.is_empty() {
            ui.add_space(kit::S2);
            kit::card(
                ui,
                &theme,
                "Recently merged",
                Some(&detail.merged.len().to_string()),
                |ui| {
                    for pr in detail.merged.iter().take(4) {
                        self.repo_pr_row(ui, pr, true, now);
                    }
                },
            );
        }
    }

    fn repo_pulls(
        &mut self,
        ui: &mut egui::Ui,
        detail: &RepoDetail,
        view: &mut RepoView,
        has_detail: bool,
        now: u64,
    ) {
        let theme = self.theme.clone();
        let width = ui.available_width();
        kit::search(
            ui,
            &theme,
            &mut view.filter,
            "Search titles, branches, authors…",
            width,
        );
        ui.add_space(kit::S1);
        ui.horizontal_wrapped(|ui| {
            for (label, filter) in [
                ("All", ""),
                ("CI red", "ci:red"),
                ("Needs review", "review:none"),
                ("Changes", "review:changes"),
                ("Drafts", "is:draft"),
                ("Mine", "is:mine"),
            ] {
                if kit::pill_tab(
                    ui,
                    &theme,
                    label,
                    None,
                    view.filter == filter,
                    style::c(theme.accent),
                )
                .clicked()
                {
                    view.filter = filter.to_owned();
                }
            }
        });
        ui.add_space(kit::S2);
        if !has_detail {
            ui.horizontal(|ui| {
                ui.spinner();
                kit::text(ui, "Loading pull requests…", 12.0, style::c(theme.dim));
            });
            return;
        }
        let filter = github::Filter::parse(&view.filter);
        for error in &filter.errors {
            kit::text(ui, error, 11.0, style::c(theme.danger));
        }
        let me = self.inbox.login.clone();
        let open: Vec<&PrRow> = detail
            .open
            .iter()
            .filter(|pr| filter.matches(pr, github::Bucket::Involved, &me))
            .collect();
        kit::section(ui, &theme, "Open", Some(open.len()), |_| {});
        if open.is_empty() {
            kit::text(
                ui,
                "No open pull requests match.",
                12.0,
                style::c(theme.dim),
            );
        }
        for pr in open {
            self.repo_pr_row(ui, pr, false, now);
        }
        ui.add_space(kit::S3);
        let merged: Vec<&PrRow> = detail
            .merged
            .iter()
            .filter(|pr| filter.matches(pr, github::Bucket::Merged, &me))
            .collect();
        kit::section(ui, &theme, "Recently merged", Some(merged.len()), |_| {});
        if merged.is_empty() {
            kit::text(
                ui,
                "No merged pull requests match.",
                12.0,
                style::c(theme.dim),
            );
        }
        for pr in merged {
            self.repo_pr_row(ui, pr, true, now);
        }
    }

    fn repo_deployments(
        &mut self,
        ui: &mut egui::Ui,
        detail: &RepoDetail,
        view: &mut RepoView,
        now: u64,
    ) {
        let theme = self.theme.clone();
        if view.activity_loading && !view.activity_loaded {
            ui.horizontal(|ui| {
                ui.spinner();
                kit::text(ui, "Loading deployments…", 12.0, style::c(theme.dim));
            });
        }
        if let Some(error) = view.activity_error.clone() {
            ui.horizontal_wrapped(|ui| {
                kit::text(ui, &error, 11.5, style::c(theme.danger));
                if kit::button(ui, &theme, "Retry", None).clicked() {
                    view.activity_error = None;
                    view.activity_loaded = false;
                }
            });
        }
        kit::section(
            ui,
            &theme,
            "Deployment history",
            view.activity_loaded.then_some(detail.deployments.len()),
            |_| {},
        );
        if view.activity_loaded && detail.deployments.is_empty() {
            kit::empty(
                ui,
                &theme,
                "No deployments",
                "This repository records no GitHub deployments.",
            );
        }
        for deployment in &detail.deployments {
            self.repo_deployment_row(ui, deployment, now, false);
        }
    }

    fn repo_deployment_row(
        &mut self,
        ui: &mut egui::Ui,
        deployment: &Deployment,
        now: u64,
        compact: bool,
    ) {
        let theme = self.theme.clone();
        let color = deploy_color(&theme, deployment);
        let title = if deployment.reference.is_empty() {
            &deployment.sha
        } else {
            &deployment.reference
        };
        let description = if deployment.headline.is_empty() {
            &deployment.description
        } else {
            &deployment.headline
        };
        let response = kit::row(
            ui,
            &theme,
            ("deployment", &deployment.created, &deployment.environment),
            kit::ROW2,
            false,
            &[Col::Px(14.0), Col::Flex, Col::Px(82.0)],
            |column, ui| match column {
                0 => kit::dot(ui, color, 4.0),
                1 => {
                    ui.vertical(|ui| {
                        kit::text(
                            ui,
                            &format!("{} · {}", deployment.environment, title),
                            11.5,
                            style::c(theme.text),
                        );
                        if !compact {
                            kit::text(ui, description, 10.5, style::c(theme.dim));
                        } else {
                            kit::text(
                                ui,
                                &format!(
                                    "{} · {}",
                                    deployment.creator,
                                    github::age(&deployment.created, now)
                                ),
                                10.0,
                                style::c(theme.dim),
                            );
                        }
                    });
                }
                _ => kit::right(ui, &deploy_label(deployment), 10.5, color),
            },
        );
        let link = if deployment.log_url.is_empty() {
            &deployment.env_url
        } else {
            &deployment.log_url
        };
        if response
            .on_hover_text(format!(
                "{}\n{} · {}",
                description, deployment.sha, deployment.created
            ))
            .clicked()
            && !link.is_empty()
        {
            self.queue.push(Command::new("url.open", Some(link)));
        }
    }

    fn repo_releases(&mut self, ui: &mut egui::Ui, detail: &RepoDetail, view: &RepoView, now: u64) {
        let theme = self.theme.clone();
        if view.activity_loading && !view.activity_loaded {
            ui.horizontal(|ui| {
                ui.spinner();
                kit::text(ui, "Loading releases…", 12.0, style::c(theme.dim));
            });
        }
        kit::section(
            ui,
            &theme,
            "Release history",
            view.activity_loaded.then_some(detail.releases.len()),
            |_| {},
        );
        if view.activity_loaded && detail.releases.is_empty() {
            kit::empty(
                ui,
                &theme,
                "No releases",
                "Nothing published on GitHub Releases.",
            );
        }
        for (index, release) in detail.releases.iter().enumerate() {
            kit::card(
                ui,
                &theme,
                &release.tag,
                Some(&github::age(&release.published, now)),
                |ui| {
                    self.repo_release_summary(ui, release, now, false);
                    if !release.body.trim().is_empty() {
                        egui::CollapsingHeader::new(RichText::new("Release notes").size(11.5))
                            .id_salt(("release", &release.tag))
                            .default_open(index == 0)
                            .show(ui, |ui| self.md(ui, &release.body, 12.0));
                    }
                },
            );
            ui.add_space(kit::S1);
        }
    }

    fn repo_release_summary(
        &mut self,
        ui: &mut egui::Ui,
        release: &Release,
        now: u64,
        show_tag: bool,
    ) {
        let theme = self.theme.clone();
        ui.vertical(|ui| {
            if show_tag {
                kit::mono(ui, &release.tag, 12.5, style::c(theme.text));
            }
            if release.name != release.tag && !release.name.is_empty() {
                kit::text(ui, &release.name, 12.5, style::c(theme.text));
            }
            if release.prerelease {
                kit::pill(ui, &theme, "pre-release", style::c(theme.blocked));
            }
            ui.horizontal(|ui| {
                kit::text(
                    ui,
                    &format!(
                        "{} · @{}",
                        github::age(&release.published, now),
                        release.author
                    ),
                    10.5,
                    style::c(theme.dim),
                );
                if kit::icon_button(ui, &theme, kit::Icon::External, &release.url).clicked() {
                    self.queue
                        .push(Command::new("url.open", Some(&release.url)));
                }
            });
        });
    }

    fn repo_pr_row(&mut self, ui: &mut egui::Ui, pr: &PrRow, merged: bool, now: u64) {
        let theme = self.theme.clone();
        let Some(reference) = pr.pr.clone() else {
            return;
        };
        let status = if merged {
            "merged"
        } else if matches!(pr.checks.as_str(), "FAILURE" | "ERROR") {
            "CI red"
        } else if pr.review == "CHANGES_REQUESTED" {
            "changes"
        } else if pr.review == "REVIEW_REQUIRED" || pr.review.is_empty() {
            "review"
        } else if pr.review == "APPROVED" {
            "approved"
        } else if matches!(pr.checks.as_str(), "PENDING" | "EXPECTED") {
            "CI pending"
        } else {
            ""
        };
        let status_color = if merged || pr.review == "APPROVED" {
            style::c(if merged { theme.accent } else { theme.done })
        } else if pr.review == "CHANGES_REQUESTED" || status == "CI red" {
            style::c(theme.danger)
        } else {
            crate::views::check_color(&theme, &pr.checks)
        };
        let timestamp = if merged && !pr.merged_at.is_empty() {
            &pr.merged_at
        } else {
            &pr.updated
        };
        let metadata = format!(
            "{} -> {} · @{} · +{} -{} · {} comments · {}",
            pr.head,
            pr.base,
            pr.author,
            pr.additions,
            pr.deletions,
            pr.comments,
            github::age(timestamp, now)
        );
        let open_tab = self.tabs.contains(&Tab::Pr(reference.clone()));
        let response = kit::row(
            ui,
            &theme,
            ("repo-pr", &reference),
            kit::ROW2,
            open_tab,
            &[Col::Px(14.0), Col::Px(48.0), Col::Flex, Col::Px(76.0)],
            |column, ui| match column {
                0 => kit::dot(
                    ui,
                    if merged {
                        style::c(theme.accent)
                    } else {
                        crate::views::check_color(&theme, &pr.checks)
                    },
                    4.0,
                ),
                1 => {
                    kit::mono(
                        ui,
                        &format!("#{}", reference.number),
                        10.5,
                        style::c(theme.dim),
                    );
                }
                2 => {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if pr.draft {
                                kit::pill(ui, &theme, "draft", style::c(theme.unknown));
                            }
                            kit::text(ui, &pr.title, 12.0, style::c(theme.text));
                        });
                        kit::text(ui, &metadata, 10.0, style::c(theme.dim));
                    });
                }
                _ => kit::right(ui, status, 10.5, status_color),
            },
        );
        if response.on_hover_text(&metadata).clicked() {
            self.open_tab(Tab::Pr(reference));
        }
    }
}

/// `owner/repo` from user input: a slug, a repo URL, or a PR reference.
pub fn parse_repo(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(pr) = PrRef::parse(input) {
        return Some(pr.repo);
    }
    if input.contains("github.com") {
        return github::slug_from_remote(input);
    }
    let mut parts = input.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (valid(owner) && valid(repo) && parts.next().is_none()).then(|| format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::{parse_repo, pr_attention};
    use artalde_core::github::PrRow;

    #[test]
    fn repos_parse_from_input() {
        assert_eq!(
            parse_repo("join-com/data").as_deref(),
            Some("join-com/data")
        );
        assert_eq!(
            parse_repo("https://github.com/join-com/data/pulls").as_deref(),
            None,
            "a sub-page is not a repo URL"
        );
        assert_eq!(
            parse_repo("https://github.com/join-com/data").as_deref(),
            Some("join-com/data")
        );
        assert_eq!(
            parse_repo("join-com/data#12").as_deref(),
            Some("join-com/data")
        );
        assert_eq!(parse_repo("not a repo"), None);
    }

    #[test]
    fn failing_checks_are_the_highest_pr_attention_signal() {
        let mut pr = PrRow {
            review: "APPROVED".into(),
            ..PrRow::default()
        };
        assert_eq!(pr_attention(&pr), 0);
        pr.review = "REVIEW_REQUIRED".into();
        assert_eq!(pr_attention(&pr), 2);
        pr.checks = "FAILURE".into();
        assert_eq!(pr_attention(&pr), 4);
    }
}
