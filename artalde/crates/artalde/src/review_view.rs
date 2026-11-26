//! The Review tab of a PR: run an agent, decide on what it proposes, add
//! your own comments, then finish the review — nothing is posted until
//! Submit, and Submit asks first.

use artalde_core::github::{Action, PrDetail, PrRef};
use artalde_core::review::{self, Draft, DraftState, Origin, PendingReview, Verdict};
use artalde_core::text::ago;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};

use crate::app::App;
use crate::kit;
use crate::style::{self, tint};
use crate::views::{PrSub, PrView};

fn severity_color(theme: &artalde_core::theme::Theme, sev: &str) -> Color32 {
    style::c(match sev {
        "blocker" => theme.danger,
        "major" => theme.blocked,
        "minor" => theme.working,
        _ => theme.dim,
    })
}

impl App {
    /// The pending review for `pr`, loaded from disk the first time.
    pub(crate) fn pending<'a>(&self, pr: &PrRef, view: &'a mut PrView) -> &'a mut PendingReview {
        view.review.get_or_insert_with(|| {
            review::load(&review::review_dir(&artalde_core::store::state_dir(), pr))
        })
    }

    pub(crate) fn save_review(&mut self, pr: &PrRef, view: &PrView) {
        if let Some(r) = &view.review
            && let Err(e) = review::save(
                &review::review_dir(&artalde_core::store::state_dir(), pr),
                r,
            )
        {
            self.push_log(format!("review: {e}"));
        }
    }

    /// The worktree checked out on the PR's head branch, if one exists here.
    fn checkout_for(&self, d: &PrDetail) -> Option<std::path::PathBuf> {
        self.snap
            .spaces
            .iter()
            .flat_map(|s| &s.worktrees)
            .find(|w| w.branch.as_deref() == Some(d.head.as_str()))
            .map(|w| w.path.clone())
    }

    pub(crate) fn start_agent_review(
        &mut self,
        ctx: &egui::Context,
        pr: &PrRef,
        agent_index: usize,
    ) {
        if self.dry_run {
            self.push_log(format!("dry run: review {pr} with agent {agent_index}"));
            return;
        }
        let Some(view) = self.prs.get(pr) else { return };
        let Some(detail) = view.detail.clone() else {
            self.push_log("review: the PR is still loading");
            return;
        };
        let agents = self.settings().review_agents;
        let Some(agent) = agents.get(agent_index).or_else(|| agents.first()).cloned() else {
            self.push_log("review: no review agents in settings.json");
            return;
        };
        let dir = review::review_dir(&artalde_core::store::state_dir(), pr);
        let checkout = self.checkout_for(&detail);
        self.reviewer.start(
            ctx,
            crate::reviewer::Job {
                pr: pr.clone(),
                agent: agent.name.clone(),
                command: agent.command,
                detail: *detail,
                dir,
                checkout,
            },
        );
        self.push_log(format!("{pr}: {} review started", agent.name));
    }

    /// Fold finished agent runs into their PR's pending review.
    pub(crate) fn drain_reviews(&mut self, ctx: &egui::Context) {
        self.reviewer.reap();
        while let Ok(done) = self.reviewer.rx.try_recv() {
            self.reviewer.runs.remove(&done.pr);
            let mut view = self.prs.remove(&done.pr).unwrap_or_default();
            let files = view
                .diff
                .as_ref()
                .and_then(|d| d.as_ref().ok())
                .cloned()
                .unwrap_or_default();
            let line = match done
                .result
                .and_then(|text| review::parse_agent_output(&text))
            {
                Ok(run) => {
                    if files.is_empty() {
                        // Anchoring needs the parsed diff; fetch it and keep the run.
                        view.pending_run = Some(run);
                        format!(
                            "{}: {} finished — loading the diff to place its comments",
                            done.pr, done.agent
                        )
                    } else {
                        let (a, l) = self.pending(&done.pr, &mut view).absorb(run, &files);
                        view.agent_status = Some(format!(
                            "{} proposed {a} comment{}{}",
                            done.agent,
                            if a == 1 { "" } else { "s" },
                            if l > 0 {
                                format!(", {l} general note{}", if l == 1 { "" } else { "s" })
                            } else {
                                String::new()
                            }
                        ));
                        view.sub = PrSub::Review;
                        self.save_review(&done.pr, &view);
                        format!("{}: {} proposed {a} comments", done.pr, done.agent)
                    }
                }
                Err(e) => {
                    view.agent_status = Some(format!("{} failed: {e}", done.agent));
                    format!("{}: {} review failed: {e}", done.pr, done.agent)
                }
            };
            self.push_log(line.clone());
            self.toast_line(ctx, line);
            self.prs.insert(done.pr, view);
        }
        // Runs that finished before their diff arrived.
        let waiting: Vec<PrRef> = self
            .prs
            .iter()
            .filter(|(_, v)| v.pending_run.is_some())
            .map(|(p, _)| p.clone())
            .collect();
        for pr in waiting {
            let Some(mut view) = self.prs.remove(&pr) else {
                continue;
            };
            match view.diff.clone() {
                Some(Ok(files)) => {
                    if let Some(run) = view.pending_run.take() {
                        let (a, l) = self.pending(&pr, &mut view).absorb(run, &files);
                        view.agent_status =
                            Some(format!("proposed {a} comments, {l} general notes"));
                        view.sub = PrSub::Review;
                        self.save_review(&pr, &view);
                    }
                }
                Some(Err(_)) => view.pending_run = None,
                None if !view.diff_loading
                    && self.gh.send(crate::gh::Req::Diff(pr.clone())).is_ok() =>
                {
                    view.diff_loading = true;
                }
                None => {}
            }
            self.prs.insert(pr, view);
        }
    }

    pub(crate) fn review_tab(
        &mut self,
        ui: &mut egui::Ui,
        pr: &PrRef,
        view: &mut PrView,
        d: &PrDetail,
    ) {
        let theme = self.theme.clone();
        let dim = style::c(theme.dim);
        egui::ScrollArea::vertical().id_salt(("review", pr.to_string())).auto_shrink([false, false]).show(ui, |ui| {
            // Agent.
            let agents = self.settings().review_agents;
            kit::section(ui, &theme, "Agent review", None, |_| {});
            egui::Frame::new()
                .fill(style::c(theme.raised))
                .stroke(Stroke::new(1.0, style::c(theme.border)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let running = self.reviewer.runs.get(pr).map(|r| (r.agent.clone(), r.started.elapsed().as_secs()));
                    match running {
                        Some((agent, secs)) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new(format!("{agent} is reviewing… {}", ago(secs))).font(crate::fonts::semibold(13.0)));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if kit::button(ui, &theme, "Cancel", None).clicked() {
                                        self.reviewer.cancel(pr);
                                    }
                                });
                            });
                            ui.label(RichText::new("Read-only: it reads pr.md and pr.diff (and your checkout of the head branch, if any) and proposes comments. Nothing is posted.").size(12.0).color(dim));
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                        }
                        None => {
                            ui.horizontal(|ui| {
                                let names: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();
                                let sel = view.agent.min(names.len().saturating_sub(1));
                                if !names.is_empty() {
                                    let current = names.get(sel).cloned().unwrap_or_default();
                                    kit::dropdown(ui, &theme, ("agent", pr.to_string()), &current, 140.0, |ui| {
                                        for (i, n) in names.iter().enumerate() {
                                            if kit::menu_item(ui, &theme, n, i == sel) {
                                                view.agent = i;
                                            }
                                        }
                                    });
                                }
                                if kit::primary(ui, &theme, "Review with agent", style::c(theme.accent)).on_hover_text("⇧⌘R").clicked() {
                                    let ctx = ui.ctx().clone();
                                    let pr2 = pr.clone();
                                    let agent = view.agent;
                                    self.queue_review = Some((pr2, agent));
                                    let _ = ctx;
                                }
                                if let Some(s) = &view.agent_status {
                                    ui.label(RichText::new(s).size(12.0).color(dim));
                                }
                            });
                            ui.label(RichText::new("The agent proposes; you decide on every comment below before anything reaches GitHub.").size(12.0).color(dim));
                        }
                    }
                    let dir = review::review_dir(&artalde_core::store::state_dir(), pr);
                    if dir.join("agent.log").exists() && kit::button(ui, &theme, "Open run folder", None).clicked() {
                        self.queue.push(artalde_core::command::Command::new("path.open", Some(&dir.display().to_string())));
                    }
                });
            ui.add_space(kit::S3);

            // Drafts.
            let pending = self.pending(pr, view).clone();
            let proposed = pending.proposed();
            let accepted = pending.accepted().count();
            kit::section(ui, &theme, "Comments", Some(pending.drafts.iter().filter(|d| d.state != DraftState::Dismissed).count()), |ui| {
                if proposed > 0 {
                    if kit::button(ui, &theme, &format!("Dismiss {proposed} proposed"), None).clicked() {
                        for d in &mut self.pending(pr, view).drafts {
                            if d.state == DraftState::Proposed {
                                d.state = DraftState::Dismissed;
                            }
                        }
                        self.save_review(pr, view);
                    }
                    if kit::button(ui, &theme, &format!("Accept {proposed} proposed"), None).clicked() {
                        for d in &mut self.pending(pr, view).drafts {
                            if d.state == DraftState::Proposed {
                                d.state = DraftState::Accepted;
                            }
                        }
                        self.save_review(pr, view);
                    }
                }
            });
            if pending.drafts.is_empty() {
                kit::empty(ui, &theme, "No comments yet", "Run an agent review, or click a line number in Files to comment.");
            }
            let mut files: Vec<String> = pending.drafts.iter().map(|d| d.path.clone()).collect();
            files.dedup();
            let mut seen = std::collections::HashSet::new();
            files.retain(|f| seen.insert(f.clone()));
            for f in files {
                ui.add_space(kit::S1);
                ui.label(RichText::new(&f).monospace().size(12.0).color(dim));
                for d in pending.drafts.iter().filter(|d| d.path == f) {
                    self.draft_card(ui, pr, view, d, true);
                }
            }
            if !pending.general_notes.is_empty() {
                ui.add_space(kit::S3);
                kit::section(ui, &theme, "General notes", Some(pending.general_notes.len()), |_| {});
                ui.label(RichText::new("Findings the agent could not place on a changed line. Add the useful ones to the summary.").size(12.0).color(dim));
                let mut add = None;
                for (i, n) in pending.general_notes.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        if kit::button_icon(ui, &theme, kit::Icon::Plus, "Summary", None).clicked() {
                            add = Some(i);
                        }
                        ui.vertical(|ui| self.md(ui, n, 13.0));
                    });
                }
                if let Some(i) = add {
                    let p = self.pending(pr, view);
                    let note = p.general_notes.remove(i);
                    if !p.summary.is_empty() {
                        p.summary.push_str("\n\n");
                    }
                    p.summary.push_str(&format!("- {note}"));
                    self.save_review(pr, view);
                }
            }

            // Finish.
            ui.add_space(kit::S3);
            kit::section(ui, &theme, "Finish review", None, |_| {});
            egui::Frame::new()
                .fill(style::c(theme.raised))
                .stroke(Stroke::new(1.0, style::c(theme.border)))
                .corner_radius(CornerRadius::same(kit::CARD_RADIUS))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut summary = self.pending(pr, view).summary.clone();
                    let r = ui.add(egui::TextEdit::multiline(&mut summary).hint_text("Summary (markdown). The agent's summary lands here; edit freely.").desired_rows(4).desired_width(f32::INFINITY));
                    if r.changed() {
                        self.pending(pr, view).summary = summary.clone();
                    }
                    if r.lost_focus() {
                        self.save_review(pr, view);
                    }
                    ui.add_space(kit::S1);
                    let agent_verdict = Verdict::parse(&pending.agent_verdict);
                    ui.horizontal(|ui| {
                        for v in [Verdict::Comment, Verdict::Approve, Verdict::RequestChanges] {
                            let allowed = !(d.viewer_is_author && v != Verdict::Comment);
                            let label = if agent_verdict == Some(v) { format!("{} · agent", v.label()) } else { v.label().to_owned() };
                            ui.add_enabled_ui(allowed, |ui| {
                                if kit::pill_tab(ui, &theme, &label, None, view.verdict == v, style::c(theme.accent)).clicked() {
                                    view.verdict = v;
                                }
                            });
                        }
                    });
                    if d.viewer_is_author {
                        ui.label(RichText::new("You authored this PR: GitHub only lets you comment.").size(11.5).color(dim));
                    }
                    ui.add_space(kit::S1);
                    ui.horizontal(|ui| {
                        let n = accepted;
                        let label = format!("Submit review · {} · {n} comment{}", view.verdict.label().to_lowercase(), if n == 1 { "" } else { "s" });
                        let ok = d.open() && !self.pending_actions.contains_key(pr) && (n > 0 || !summary.trim().is_empty());
                        ui.add_enabled_ui(ok, |ui| {
                            if kit::primary(ui, &theme, &label, style::c(theme.accent)).clicked() {
                                let review = Box::new(self.pending(pr, view).clone());
                                view.submitting = true;
                                self.act(pr, Action::SubmitReview { verdict: view.verdict, body: summary.trim().to_owned(), review });
                            }
                        });
                        if proposed > 0 {
                            ui.label(RichText::new(format!("{proposed} proposed comment{} still undecided — only accepted ones are sent", if proposed == 1 { "" } else { "s" })).size(11.5).color(style::c(theme.blocked)));
                        }
                    });
                });
            ui.add_space(kit::S3 * 2.0);
        });
    }

    /// One draft: origin, severity, place, body, decision buttons.
    pub(crate) fn draft_card(
        &mut self,
        ui: &mut egui::Ui,
        pr: &PrRef,
        view: &mut PrView,
        d: &Draft,
        show_place: bool,
    ) {
        let theme = self.theme.clone();
        let dim = style::c(theme.dim);
        let edge = match d.state {
            DraftState::Accepted => style::c(theme.done),
            DraftState::Proposed => style::c(theme.accent),
            DraftState::Dismissed => style::c(theme.unknown),
        };
        let fill = if d.state == DraftState::Dismissed {
            style::c(theme.surface)
        } else {
            tint(style::c(theme.raised), edge, 0.05)
        };
        let resp = egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, tint(style::c(theme.border), edge, 0.35)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin {
                left: 14,
                right: 10,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    kit::pill(
                        ui,
                        &theme,
                        if d.origin == Origin::Agent {
                            "agent"
                        } else {
                            "you"
                        },
                        if d.origin == Origin::Agent {
                            style::c(theme.accent)
                        } else {
                            style::c(theme.text)
                        },
                    );
                    if !d.severity.is_empty() {
                        kit::pill(ui, &theme, &d.severity, severity_color(&theme, &d.severity));
                    }
                    if show_place {
                        let place = format!(
                            "{}:{}{}",
                            d.path.rsplit('/').next().unwrap_or(&d.path),
                            d.line,
                            if d.side == review::Side::Left {
                                " (old)"
                            } else {
                                ""
                            }
                        );
                        if ui
                            .link(RichText::new(place).monospace().size(11.5))
                            .on_hover_text("Show in Files")
                            .clicked()
                        {
                            view.sub = PrSub::Files;
                            view.jump = Some((d.path.clone(), d.line));
                        }
                    }
                    ui.label(
                        RichText::new(match d.state {
                            DraftState::Accepted => "in review",
                            DraftState::Proposed => "proposed",
                            DraftState::Dismissed => "dismissed",
                        })
                        .size(11.0)
                        .color(edge),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let set = |s: DraftState, app: &mut App, view: &mut PrView| {
                            if let Some(x) = app.pending(pr, view).get_mut(d.id) {
                                x.state = s;
                            }
                            app.save_review(pr, view);
                        };
                        match d.state {
                            DraftState::Proposed => {
                                if kit::button(ui, &theme, "Dismiss", None).clicked() {
                                    set(DraftState::Dismissed, self, view);
                                }
                                if kit::button(ui, &theme, "Edit", None).clicked() {
                                    view.editing = Some((d.id, d.body.clone()));
                                }
                                if kit::primary(ui, &theme, "Accept", style::c(theme.done))
                                    .clicked()
                                {
                                    set(DraftState::Accepted, self, view);
                                }
                            }
                            DraftState::Accepted => {
                                if kit::button(ui, &theme, "Remove", None).clicked() {
                                    set(DraftState::Dismissed, self, view);
                                }
                                if kit::button(ui, &theme, "Edit", None).clicked() {
                                    view.editing = Some((d.id, d.body.clone()));
                                }
                            }
                            DraftState::Dismissed => {
                                if kit::button(ui, &theme, "Restore", None).clicked() {
                                    set(
                                        if d.origin == Origin::Agent {
                                            DraftState::Proposed
                                        } else {
                                            DraftState::Accepted
                                        },
                                        self,
                                        view,
                                    );
                                }
                            }
                        }
                    });
                });
                if !d.title.is_empty() {
                    ui.label(RichText::new(&d.title).font(crate::fonts::semibold(13.5)));
                }
                let editing = view
                    .editing
                    .as_ref()
                    .filter(|(id, _)| *id == d.id)
                    .map(|(_, t)| t.clone());
                match editing {
                    Some(mut text) => {
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        view.editing = Some((d.id, text.clone()));
                        ui.horizontal(|ui| {
                            if kit::primary(ui, &theme, "Save & accept", style::c(theme.done))
                                .clicked()
                            {
                                if let Some(x) = self.pending(pr, view).get_mut(d.id) {
                                    x.body = text;
                                    x.state = DraftState::Accepted;
                                }
                                view.editing = None;
                                self.save_review(pr, view);
                            }
                            if kit::button(ui, &theme, "Cancel", None).clicked() {
                                view.editing = None;
                            }
                        });
                    }
                    None if d.state != DraftState::Dismissed => {
                        let mut shown = d.clone();
                        shown.title.clear();
                        self.md(ui, &shown.rendered(), 13.0);
                    }
                    None => {
                        ui.label(
                            RichText::new(artalde_core::text::clip(&d.body, 120))
                                .size(12.0)
                                .color(dim),
                        );
                    }
                }
            })
            .response;
        let bar = egui::Rect::from_min_size(resp.rect.min, egui::vec2(3.0, resp.rect.height()));
        ui.painter().rect_filled(
            bar,
            CornerRadius {
                nw: 8,
                sw: 8,
                ne: 0,
                se: 0,
            },
            edge,
        );
        ui.add_space(kit::S1);
    }
}
