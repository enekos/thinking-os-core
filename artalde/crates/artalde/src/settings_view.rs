//! The Settings tab: every option in `settings.json`, plus the read-only
//! state a developer wants at hand (keys, hook, mods, paths). Edits go to a
//! draft; Save writes `settings.json` and the config watcher applies it.

use artalde_core::command::{COMMANDS, Command};
use artalde_core::config::{self, Settings};
use artalde_core::theme::Theme;
use eframe::egui::{self, RichText};

use crate::app::App;
use crate::kit;
use crate::style;

const LABEL_W: f32 = 220.0;
const FIELD_W: f32 = 420.0;
const PAGE_W: f32 = 860.0;

/// `label` + help text on the left, the control on the right.
fn field(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    help: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(LABEL_W);
            ui.label(RichText::new(label).font(crate::fonts::semibold(13.0)));
            if !help.is_empty() {
                ui.label(RichText::new(help).size(11.5).color(style::c(theme.dim)));
            }
        });
        ui.vertical(|ui| {
            ui.set_width(FIELD_W);
            control(ui);
        });
    });
    ui.add_space(kit::S3);
}

fn swatches(ui: &mut egui::Ui, t: &Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for c in [
            t.background,
            t.surface,
            t.raised,
            t.text,
            t.accent,
            t.working,
            t.blocked,
            t.done,
            t.danger,
        ] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(r, egui::CornerRadius::same(3), style::c(c));
            ui.painter().rect_stroke(
                r,
                egui::CornerRadius::same(3),
                egui::Stroke::new(1.0, style::c(t.border)),
                egui::StrokeKind::Inside,
            );
        }
    });
}

impl App {
    /// Whether `~/.claude/settings.json` already calls `artalde hook`.
    fn hook_installed() -> bool {
        artalde_core::home()
            .map(|h| h.join(".claude/settings.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .is_some_and(|s| s.contains("artalde hook"))
    }

    pub(crate) fn settings_view(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme.clone();
        let Some(cfg) = self.config.clone() else {
            ui.spinner();
            return;
        };
        let mut draft = self
            .settings_draft
            .take()
            .unwrap_or_else(|| cfg.settings.clone());
        let dirty = draft != cfg.settings;
        // Header with Save / Revert.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").font(crate::fonts::display(22.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(dirty, |ui| {
                    if kit::primary(ui, &theme, "Save", style::c(theme.accent)).clicked() {
                        match config::save_settings(&cfg.dir, &draft) {
                            Ok(()) => {
                                self.push_log("settings saved");
                                let ctx = ui.ctx().clone();
                                self.toast_line(&ctx, "Settings saved".into());
                            }
                            Err(e) => self.push_log(format!("settings: {e}")),
                        }
                    }
                    if kit::button(ui, &theme, "Revert", None).clicked() {
                        draft = cfg.settings.clone();
                    }
                });
                if dirty {
                    ui.label(
                        RichText::new("unsaved changes")
                            .size(12.0)
                            .color(style::c(theme.blocked)),
                    );
                }
            });
        });
        ui.label(
            RichText::new(format!(
                "Stored in {} — edits to the file apply live too.",
                cfg.dir.join("settings.json").display()
            ))
            .size(12.0)
            .color(style::c(theme.dim)),
        );
        ui.add_space(kit::S2);
        egui::ScrollArea::vertical().id_salt("settings-page").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(PAGE_W);
            // Appearance.
            kit::section(ui, &theme, "Appearance", None, |_| {});
            field(ui, &theme, "Mode", "System follows macOS. ⇧⌘D toggles light and dark.", |ui| {
                ui.horizontal(|ui| {
                    for m in ["system", "light", "dark"] {
                        let label = match m {
                            "system" => "System",
                            "light" => "☀ Light",
                            _ => "☾ Dark",
                        };
                        ui.selectable_value(&mut draft.appearance, m.to_owned(), label);
                    }
                });
            });
            let names: Vec<String> = cfg.themes.iter().map(|t| t.name.clone()).collect();
            for (label, help, slot) in [
                ("Dark theme", "Used in dark mode.", &mut draft.theme),
                ("Light theme", "Used in light mode.", &mut draft.light_theme),
            ] {
                field(ui, &theme, label, help, |ui| {
                    egui::ComboBox::from_id_salt(label).selected_text(slot.as_str()).width(200.0).show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(slot, n.clone(), n);
                        }
                    });
                    swatches(ui, &cfg.theme(slot));
                });
            }
            field(ui, &theme, "Font size", "Body text; headings and code scale with it.", |ui| {
                ui.add(egui::Slider::new(&mut draft.font_size, 11.0..=18.0).step_by(0.5).suffix(" pt"));
            });
            // Workspace.
            kit::section(ui, &theme, "Workspace", None, |_| {});
            field(ui, &theme, "Dashboard layout", "The layout of the first tab. Layouts are files in layouts/.", |ui| {
                egui::ComboBox::from_id_salt("layout").selected_text(draft.layout.as_str()).width(200.0).show_ui(ui, |ui| {
                    for l in &cfg.layouts {
                        ui.selectable_value(&mut draft.layout, l.name.clone(), format!("{} ({})", l.title, l.name));
                    }
                });
            });
            field(ui, &theme, "Editor", "App that \"Open in editor\" uses (open -a <app>).", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.editor).desired_width(200.0));
            });
            field(ui, &theme, "zaindu inbox", "Days of proposal files shown.", |ui| {
                ui.add(egui::Slider::new(&mut draft.inbox_days, 1..=14).suffix(" days"));
            });
            // Agents.
            kit::section(ui, &theme, "Agents", None, |_| {});
            field(ui, &theme, "Agent commands", "Offered when starting a new agent, comma-separated.", |ui| {
                let mut list = draft.agents.join(", ");
                if ui.add(egui::TextEdit::singleline(&mut list).desired_width(FIELD_W)).changed() {
                    draft.agents = list.split(',').map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect();
                }
            });
            field(ui, &theme, "New worktree", "{repo} {branch} {agent} {prompt} are filled in, shell-quoted.", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.spawn_worktree).code_editor().desired_width(FIELD_W));
                ui.label(RichText::new(format!("$ {}", draft.spawn_worktree.replace("{repo}", "data").replace("{branch}", "THU-123-x").replace("{agent}", "claude").replace("{prompt}", "")))
                    .monospace()
                    .size(11.5)
                    .color(style::c(theme.dim)));
            });
            field(ui, &theme, "Existing folder", "{dir} {agent} {prompt} are filled in.", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.spawn_here).code_editor().desired_width(FIELD_W));
            });
            field(ui, &theme, "Notifications", "macOS notification when an agent blocks or finishes while the window is in the background.", |ui| {
                ui.checkbox(&mut draft.notify, "Notify");
            });
            // GitHub.
            kit::section(ui, &theme, "GitHub", None, |_| {});
            field(ui, &theme, "Pull requests", "Poll gh for your open PRs (pulse and mods). The PR inbox always loads on demand.", |ui| {
                ui.checkbox(&mut draft.gh, "Poll gh every 2 minutes");
                if !self.inbox.login.is_empty() {
                    ui.label(RichText::new(format!("signed in as @{}", self.inbox.login)).size(11.5).color(style::c(theme.dim)));
                }
            });
            ui.add_space(kit::S2);
            // Claude Code hook.
            kit::section(ui, &theme, "Claude Code hook", None, |_| {});
            let installed = App::hook_installed();
            field(
                ui,
                &theme,
                "Status",
                "Session state works without it (from Claude's own files); the hook adds the tool in flight and permission prompts.",
                |ui| {
                    if installed {
                        kit::pill(ui, &theme, "installed", style::c(theme.done));
                    } else {
                        kit::pill(ui, &theme, "not installed", style::c(theme.blocked));
                        if kit::button(ui, &theme, "Install into ~/.claude/settings.json…", None).clicked() {
                            self.hook_confirm = true;
                        }
                    }
                },
            );
            // Keyboard.
            kit::section(ui, &theme, "Keyboard", Some(cfg.keys.bindings.len()), |ui| {
                if ui.small_button("Edit keys.json").clicked() {
                    self.queue.push(Command::new("path.edit", Some(&cfg.dir.display().to_string())));
                }
            });
            egui::Grid::new("keys-grid").num_columns(3).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                for (chord, cmd) in &cfg.keys.bindings {
                    kit::key(ui, &theme, &chord.to_string());
                    ui.label(RichText::new(cmd.to_string()).monospace().size(12.0));
                    let desc = COMMANDS.iter().find(|(n, _)| *n == cmd.name).map_or("", |(_, d)| d);
                    ui.label(RichText::new(desc).size(12.0).color(style::c(theme.dim)));
                    ui.end_row();
                }
            });
            ui.label(RichText::new("In lists: j/k move, Enter opens, f focuses the terminal. Right-click rows for copy actions.").size(11.5).color(style::c(theme.dim)));
            ui.add_space(kit::S3);
            // Mods.
            kit::section(ui, &theme, "Mods", Some(cfg.mods.len()), |ui| {
                if ui.small_button("Open mods folder").clicked() {
                    self.queue.push(Command::new("path.open", Some(&cfg.dir.join("mods").display().to_string())));
                }
            });
            for m in &cfg.mods {
                let err = std::fs::read_to_string(format!("{}.error", m.path.display())).ok();
                ui.horizontal(|ui| {
                    kit::dot(ui, if err.is_some() { style::c(theme.danger) } else { style::c(theme.done) }, 3.5);
                    ui.label(RichText::new(&m.name).monospace());
                    ui.label(RichText::new(m.path.display().to_string()).size(11.0).color(style::c(theme.dim)));
                    if ui.small_button("edit").clicked() {
                        self.queue.push(Command::new("path.edit", Some(&cfg.dir.display().to_string())));
                    }
                });
                if let Some(e) = err {
                    ui.label(RichText::new(e).monospace().size(11.0).color(style::c(theme.danger)));
                }
            }
            if !cfg.errors.is_empty() {
                ui.add_space(kit::S2);
                for e in &cfg.errors {
                    ui.label(RichText::new(e).monospace().size(11.5).color(style::c(theme.danger)));
                }
            }
            ui.add_space(kit::S3);
            // Paths & about.
            kit::section(ui, &theme, "Paths", None, |_| {});
            let mut copied = false;
            copied |= kit::kv(ui, &theme, "config", &cfg.dir.display().to_string(), 70.0);
            copied |= kit::kv(ui, &theme, "state", &artalde_core::store::state_dir().display().to_string(), 70.0);
            copied |= kit::kv(ui, &theme, "wiki", &config::wiki_root().display().to_string(), 70.0);
            copied |= kit::kv(ui, &theme, "version", env!("CARGO_PKG_VERSION"), 70.0);
            if copied {
                let ctx = ui.ctx().clone();
                self.toast_line(&ctx, "copied".into());
            }
            ui.add_space(kit::S3 * 2.0);
        });
        self.settings_draft = Some(draft);
        let _ = Settings::default;
    }

    pub(crate) fn hook_confirm_ui(&mut self, ctx: &egui::Context) {
        if !self.hook_confirm {
            return;
        }
        let theme = self.theme.clone();
        let mut done = None;
        let modal = egui::Modal::new(egui::Id::unique("hook-confirm")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new("Install the Claude Code hook?").font(crate::fonts::display(18.0)));
            ui.add_space(kit::S1);
            ui.label("Adds `artalde hook` to SessionStart, UserPromptSubmit, Pre/PostToolUse, PermissionRequest, Notification, Stop and SessionEnd in ~/.claude/settings.json. Your existing hooks stay. The file is backed up to settings.json.artalde-bak first.");
            ui.add_space(kit::S2);
            ui.horizontal(|ui| {
                if kit::primary(ui, &theme, "Install", style::c(theme.accent)).clicked() {
                    done = Some(true);
                }
                if kit::button(ui, &theme, "Cancel", None).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() {
            done = Some(false);
        }
        if let Some(go) = done {
            self.hook_confirm = false;
            if go {
                match std::env::current_exe() {
                    Ok(exe) => self.submit_job(crate::actions::Job::Run {
                        program: exe.display().to_string(),
                        args: vec!["hooks".into(), "--install".into()],
                    }),
                    Err(e) => self.push_log(format!("hook: {e}")),
                }
            }
        }
    }
}
