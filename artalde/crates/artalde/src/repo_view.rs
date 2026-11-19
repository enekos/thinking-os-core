//! A repository tab: what is open, what merged, what is deployed where,
//! and what was released. Rows open PR tabs; environments link to their
//! consoles and logs.

use artalde_core::command::Command;
use artalde_core::github::{self, Deployment, PrRef, PrRow, RepoDetail};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::app::App;
use crate::kit::{self, Col};
use crate::style::{self, tint};
use crate::views::Tab;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepoSub {
    #[default]
    Pulls,
    Deployments,
    Releases,
}

#[derive(Default)]
pub struct RepoView {
    pub detail: Option<Box<RepoDetail>>,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded_at: Option<std::time::Instant>,
    pub sub: RepoSub,
    pub filter: String,
}

fn deploy_color(theme: &artalde_core::theme::Theme, d: &Deployment) -> Color32 {
    let s = if d.status.is_empty() {
        d.state.as_str()
    } else {
        d.status.as_str()
    };
    style::c(match s {
        "SUCCESS" | "ACTIVE" => theme.done,
        "FAILURE" | "ERROR" => theme.danger,
        "IN_PROGRESS" | "QUEUED" | "PENDING" | "WAITING" => theme.working,
        _ => theme.dim,
    })
}

fn deploy_label(d: &Deployment) -> String {
    let s = if d.status.is_empty() {
        d.state.as_str()
    } else {
        d.status.as_str()
    };
    match s {
        "SUCCESS" => "live".into(),
        "IN_PROGRESS" => "deploying".into(),
        "INACTIVE" => "replaced".into(),
        other => other.to_lowercase().replace('_', " "),
    }
}

impl App {
    pub(crate) fn load_repo(&mut self, repo: &str) {
        let sent = self.gh.send(crate::gh::Req::Repo(repo.to_owned()));
        let v = self.repos.entry(repo.to_owned()).or_default();
        match sent {
            Ok(()) => v.loading = true,
            Err(e) => v.error = Some(e),
        }
    }

    /// The local checkout of `repo`, if it is one of the spaces.
    fn local_space(&self, repo: &str) -> Option<std::path::PathBuf> {
        self.snap
            .spaces
            .iter()
            .find(|s| s.slug.as_deref() == Some(repo))
            .map(|s| s.root.clone())
    }

    pub(crate) fn repo_view(&mut self, ui: &mut egui::Ui, repo: &str) {
        let theme = self.theme.clone();
        let mut view = self.repos.remove(repo).unwrap_or_default();
        if view.detail.is_none() && !view.loading && view.error.is_none() {
            self.repos.insert(repo.to_owned(), view);
            self.load_repo(repo);
            view = self.repos.remove(repo).unwrap_or_default();
        }
        let Some(r) = view.detail.clone() else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(repo).font(crate::fonts::display(20.0)));
                match &view.error {
                    Some(e) => {
                        ui.label(RichText::new(e).color(style::c(theme.danger)));
                        if kit::button(ui, &theme, "Retry", None).clicked() {
                            view.error = None;
                        }
                    }
                    None => {
                        ui.spinner();
                    }
                }
            });
            self.repos.insert(repo.to_owned(), view);
            return;
        };
        let now = artalde_core::now();
        // Header.
        ui.horizontal(|ui| {
            ui.label(RichText::new(&r.name).font(crate::fonts::display(22.0)));
            kit::pill(ui, &theme, &r.default_branch, style::c(theme.dim));
            if r.private {
                kit::pill(ui, &theme, "private", style::c(theme.dim));
            }
            if view.loading {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if kit::button(ui, &theme, "Reload", Some("⌘R")).clicked() {
                    self.load_repo(repo);
                }
                if kit::button(ui, &theme, "GitHub ↗", None).clicked() {
                    self.queue.push(Command::new("url.open", Some(&r.url)));
                }
                let local = self.local_space(repo);
                if let Some(path) = &local {
                    let p = path.display().to_string();
                    if kit::button(ui, &theme, "New agent", None).clicked() {
                        self.queue.push(Command::new("spawn.open", Some(&p)));
                    }
                    if kit::button(ui, &theme, "Editor", None)
                        .on_hover_text(&p)
                        .clicked()
                    {
                        self.queue.push(Command::new("path.edit", Some(&p)));
                    }
                }
                let r2 = kit::button(ui, &theme, "Copy ▾", None);
                egui::Popup::menu(&r2).show(|ui| {
                    for (label, value) in [
                        ("Copy owner/repo", r.name.clone()),
                        ("Copy URL", r.url.clone()),
                        ("Copy gh repo clone", format!("gh repo clone {}", r.name)),
                    ] {
                        if ui.button(label).on_hover_text(&value).clicked() {
                            ui.ctx().copy_text(value);
                            ui.close();
                        }
                    }
                });
            });
        });
        if !r.description.is_empty() {
            ui.label(
                RichText::new(&r.description)
                    .size(13.0)
                    .color(style::c(theme.dim)),
            );
        }
        ui.add_space(kit::S2);
        // Where each environment stands.
        let envs: Vec<Deployment> = r.environments().into_iter().cloned().collect();
        if !envs.is_empty() {
            let gap = kit::S2;
            let n = envs.len() as f32;
            let total = ui.available_width();
            let w = ((total - gap * (n - 1.0)) / n).max(160.0);
            let h = 116.0;
            let (strip, _) = ui.allocate_exact_size(egui::vec2(total, h), egui::Sense::hover());
            for (k, d) in envs.iter().enumerate() {
                let color = deploy_color(&theme, d);
                let rect = egui::Rect::from_min_size(
                    egui::pos2(strip.min.x + k as f32 * (w + gap), strip.min.y),
                    egui::vec2(w, h),
                );
                let mut card = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                card.set_clip_rect(rect.intersect(ui.clip_rect()));
                egui::Frame::new()
                    .fill(tint(style::c(theme.raised), color, 0.06))
                    .stroke(Stroke::new(1.0, tint(style::c(theme.border), color, 0.4)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(12))
                    .show(&mut card, |ui| {
                        ui.set_width(w - 26.0);
                        ui.set_height(h - 28.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(d.environment.to_uppercase())
                                    .font(crate::fonts::semibold(11.0))
                                    .extra_letter_spacing(0.6)
                                    .color(style::c(theme.dim)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    kit::pill(ui, &theme, &deploy_label(d), color);
                                },
                            );
                        });
                        ui.label(RichText::new(&d.reference).monospace().size(18.0).strong());
                        kit::text(
                            ui,
                            &format!(
                                "{} · by {}",
                                github::age(&d.created, now),
                                d.creator.trim_end_matches("[bot]")
                            ),
                            11.5,
                            style::c(theme.dim),
                        );
                        ui.horizontal(|ui| {
                            if !d.env_url.is_empty()
                                && ui.link(RichText::new("console ↗").size(11.5)).clicked()
                            {
                                self.queue.push(Command::new("url.open", Some(&d.env_url)));
                            }
                            if !d.log_url.is_empty()
                                && ui.link(RichText::new("log ↗").size(11.5)).clicked()
                            {
                                self.queue.push(Command::new("url.open", Some(&d.log_url)));
                            }
                        });
                    });
            }
            let prod = envs
                .iter()
                .find(|d| d.environment.to_lowercase().contains("prod"));
            let ahead = envs
                .iter()
                .filter(|d| !d.environment.to_lowercase().contains("prod"))
                .map(|d| d.reference.clone())
                .max();
            if let (Some(p), Some(a)) = (prod, ahead)
                && a > p.reference
            {
                ui.add_space(kit::S1);
                ui.label(
                    RichText::new(format!(
                        "Production runs {} — {a} is out on other environments.",
                        p.reference
                    ))
                    .size(12.0)
                    .color(style::c(theme.blocked)),
                );
            }
            ui.add_space(kit::S2);
        }
        let subs = [RepoSub::Pulls, RepoSub::Deployments, RepoSub::Releases];
        let labels = vec![
            ("Pull requests".to_owned(), Some(r.open.len())),
            ("Deployments".to_owned(), Some(r.deployments.len())),
            ("Releases".to_owned(), Some(r.releases.len())),
        ];
        let sel = subs.iter().position(|s| *s == view.sub).unwrap_or(0);
        if let Some(i) = kit::tabs(ui, &theme, &labels, sel) {
            view.sub = subs[i.min(2)];
        }
        ui.add_space(kit::S2);
        egui::ScrollArea::vertical()
            .id_salt(("repo", repo))
            .auto_shrink([false, false])
            .show(ui, |ui| match view.sub {
                RepoSub::Pulls => {
                    let w = ui.available_width();
                    kit::search(
                        ui,
                        &theme,
                        &mut view.filter,
                        "ci:red review:changes @alice \"words\"",
                        w,
                    );
                    let f = github::Filter::parse(&view.filter);
                    let me = self.inbox.login.clone();
                    ui.add_space(kit::S1);
                    let open: Vec<&PrRow> = r
                        .open
                        .iter()
                        .filter(|p| f.matches(p, github::Bucket::Involved, &me))
                        .collect();
                    kit::section(ui, &theme, "Open", Some(open.len()), |_| {});
                    for p in open {
                        self.repo_pr_row(ui, p, false, now);
                    }
                    ui.add_space(kit::S3);
                    let merged: Vec<&PrRow> = r
                        .merged
                        .iter()
                        .filter(|p| f.matches(p, github::Bucket::Merged, &me))
                        .collect();
                    kit::section(ui, &theme, "Recently merged", Some(merged.len()), |_| {});
                    for p in merged {
                        self.repo_pr_row(ui, p, true, now);
                    }
                }
                RepoSub::Deployments => {
                    if r.deployments.is_empty() {
                        kit::empty(
                            ui,
                            &theme,
                            "No deployments",
                            "This repository records no GitHub deployments.",
                        );
                    }
                    for d in &r.deployments {
                        let color = deploy_color(&theme, d);
                        let resp = kit::row(
                            ui,
                            &theme,
                            ("deploy", &d.created, &d.environment),
                            kit::ROW,
                            false,
                            &[
                                Col::Px(12.0),
                                Col::Px(110.0),
                                Col::Px(110.0),
                                Col::Px(96.0),
                                Col::Flex,
                                Col::Px(150.0),
                                Col::Px(44.0),
                            ],
                            |c, ui| match c {
                                0 => kit::dot(ui, color, 4.0),
                                1 => {
                                    kit::text(ui, &d.environment, 12.5, style::c(theme.text));
                                }
                                2 => {
                                    kit::mono(ui, &d.reference, 12.0, style::c(theme.text));
                                }
                                3 => {
                                    kit::text(ui, &deploy_label(d), 11.5, color);
                                }
                                4 => {
                                    kit::text(
                                        ui,
                                        if d.headline.is_empty() {
                                            &d.description
                                        } else {
                                            &d.headline
                                        },
                                        12.0,
                                        style::c(theme.dim),
                                    );
                                }
                                5 => {
                                    kit::text(
                                        ui,
                                        d.creator.trim_end_matches("[bot]"),
                                        11.5,
                                        style::c(theme.dim),
                                    );
                                }
                                _ => kit::right(
                                    ui,
                                    &github::age(&d.created, now),
                                    11.0,
                                    style::c(theme.dim),
                                ),
                            },
                        );
                        let link = if d.log_url.is_empty() {
                            d.env_url.clone()
                        } else {
                            d.log_url.clone()
                        };
                        let resp = resp.on_hover_text(format!(
                            "{} {} — {}\n{}",
                            d.environment, d.reference, d.description, d.sha
                        ));
                        if resp.clicked() && !link.is_empty() {
                            self.queue.push(Command::new("url.open", Some(&link)));
                        }
                    }
                }
                RepoSub::Releases => {
                    if r.releases.is_empty() {
                        kit::empty(
                            ui,
                            &theme,
                            "No releases",
                            "Nothing published on GitHub Releases.",
                        );
                    }
                    for (i, rel) in r.releases.iter().enumerate() {
                        egui::Frame::new()
                            .fill(style::c(theme.raised))
                            .stroke(Stroke::new(1.0, style::c(theme.border)))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&rel.tag).monospace().size(14.0).strong(),
                                    );
                                    if rel.name != rel.tag && !rel.name.is_empty() {
                                        ui.label(RichText::new(&rel.name).size(13.0));
                                    }
                                    if rel.prerelease {
                                        kit::pill(
                                            ui,
                                            &theme,
                                            "pre-release",
                                            style::c(theme.blocked),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {}",
                                            github::age(&rel.published, now),
                                            rel.author
                                        ))
                                        .size(11.5)
                                        .color(style::c(theme.dim)),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if kit::icon_button(
                                                ui,
                                                &theme,
                                                kit::Icon::External,
                                                &rel.url,
                                            )
                                            .clicked()
                                            {
                                                self.queue
                                                    .push(Command::new("url.open", Some(&rel.url)));
                                            }
                                        },
                                    );
                                });
                                if !rel.body.trim().is_empty() {
                                    egui::CollapsingHeader::new(RichText::new("Notes").size(12.0))
                                        .id_salt(("release", &rel.tag))
                                        .default_open(i == 0)
                                        .show(ui, |ui| self.md(ui, &rel.body, 13.0));
                                }
                            });
                        ui.add_space(kit::S1);
                    }
                }
            });
        self.repos.insert(repo.to_owned(), view);
    }

    fn repo_pr_row(&mut self, ui: &mut egui::Ui, p: &PrRow, merged: bool, now: u64) {
        let theme = self.theme.clone();
        let Some(pr) = p.pr.clone() else { return };
        let (rl, rc) = match p.review.as_str() {
            "APPROVED" => ("approved", "done"),
            "CHANGES_REQUESTED" => ("changes requested", "danger"),
            "REVIEW_REQUIRED" => ("review required", "blocked"),
            _ => ("", "dim"),
        };
        let open_tab = self.tabs.contains(&Tab::Pr(pr.clone()));
        let resp = kit::row(
            ui,
            &theme,
            ("repo-pr", pr.number),
            kit::ROW,
            open_tab,
            &[
                Col::Px(12.0),
                Col::Px(64.0),
                Col::Flex,
                Col::Px(118.0),
                Col::Px(120.0),
                Col::Px(92.0),
                Col::Px(40.0),
            ],
            |c, ui| match c {
                0 => kit::dot(
                    ui,
                    if merged {
                        style::c(theme.accent)
                    } else {
                        crate::views::check_color(&theme, &p.checks)
                    },
                    4.0,
                ),
                1 => {
                    kit::mono(ui, &format!("#{}", pr.number), 11.5, style::c(theme.dim));
                }
                2 => {
                    if p.draft {
                        ui.label(
                            RichText::new("draft")
                                .size(11.0)
                                .color(style::c(theme.unknown)),
                        );
                    }
                    kit::text(ui, &p.title, 13.0, style::c(theme.text));
                }
                3 => {
                    if merged {
                        kit::text(ui, "merged", 11.5, style::c(theme.accent));
                    } else if !rl.is_empty() {
                        kit::text(ui, rl, 11.5, style::token(&theme, rc));
                    }
                }
                4 => {
                    kit::text(ui, &format!("@{}", p.author), 11.5, style::c(theme.dim));
                }
                5 => {
                    ui.label(
                        RichText::new(format!("+{}", p.additions))
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.done)),
                    );
                    ui.label(
                        RichText::new(format!("-{}", p.deletions))
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.danger)),
                    );
                }
                _ => kit::right(
                    ui,
                    &github::age(
                        if merged && !p.merged_at.is_empty() {
                            &p.merged_at
                        } else {
                            &p.updated
                        },
                        now,
                    ),
                    11.0,
                    style::c(theme.dim),
                ),
            },
        );
        if resp
            .on_hover_text(format!("{}\n{} → {}", p.title, p.head, p.base))
            .clicked()
        {
            self.open_tab(Tab::Pr(pr));
        }
    }
}

/// `owner/repo` from user input: a slug, a repo URL, or a PR reference.
pub fn parse_repo(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(pr) = PrRef::parse(s) {
        return Some(pr.repo);
    }
    if s.contains("github.com") {
        return github::slug_from_remote(s);
    }
    let mut parts = s.split('/');
    let (o, r) = (parts.next()?, parts.next()?);
    let ok = |x: &str| {
        !x.is_empty()
            && x.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (ok(o) && ok(r) && parts.next().is_none()).then(|| format!("{o}/{r}"))
}

#[cfg(test)]
mod tests {
    use super::parse_repo;

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
}
