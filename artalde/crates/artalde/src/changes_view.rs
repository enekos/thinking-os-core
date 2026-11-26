//! A worktree's uncommitted changes, in the pull-request diff viewer.
//!
//! git runs on a short-lived thread per load, so a huge worktree never
//! stalls a frame; a visible view reloads itself every few seconds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use artalde_core::github::{DiffFile, PrRef};
use eframe::egui;

use crate::app::App;
use crate::kit::{self, Icon};
use crate::style;
use crate::views::PrView;

/// How stale a visible view may get before it reloads.
const RELOAD: Duration = Duration::from_secs(4);

type Loaded = (PathBuf, Result<Vec<DiffFile>, String>);

pub struct Changes {
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    views: HashMap<PathBuf, PrView>,
}

impl Default for Changes {
    fn default() -> Self {
        let (tx, rx) = channel();
        Changes {
            tx,
            rx,
            views: HashMap::new(),
        }
    }
}

impl Changes {
    pub(crate) fn view_mut(&mut self, dir: &Path) -> Option<&mut PrView> {
        self.views.get_mut(dir)
    }

    fn load(&self, ctx: &egui::Context, dir: &Path) -> std::io::Result<()> {
        let (tx, ctx, dir) = (self.tx.clone(), ctx.clone(), dir.to_path_buf());
        std::thread::Builder::new()
            .name("artalde-changes".into())
            .spawn(move || {
                let out = artalde_core::changes::local(&dir);
                // The window may be gone by the time git answers.
                let _closed = tx.send((dir, out));
                ctx.request_repaint();
            })
            .map(drop)
    }

    fn drain(&mut self) {
        for (dir, out) in self.rx.try_iter() {
            if let Some(view) = self.views.get_mut(&dir) {
                // Keep the selected file across reloads, by path.
                let selected = match (&view.diff, &out) {
                    (Some(Ok(old)), Ok(new)) => old
                        .get(view.file)
                        .and_then(|f| new.iter().position(|n| n.path == f.path)),
                    _ => None,
                };
                view.file = selected.unwrap_or(view.file);
                view.diff = Some(out);
                view.diff_loading = false;
                view.loaded_at = Some(Instant::now());
            }
        }
    }
}

/// `(files, added, removed)` of a loaded diff.
fn totals(files: &[DiffFile]) -> (usize, u32, u32) {
    files.iter().fold((0, 0, 0), |(n, a, r), f| {
        (
            n + 1,
            a.saturating_add(f.added),
            r.saturating_add(f.removed),
        )
    })
}

impl App {
    /// The number of changed files in `dir`, once loaded.
    pub(crate) fn changes_count(&self, dir: &Path) -> Option<usize> {
        match self.changes.views.get(dir)?.diff.as_ref()? {
            Ok(files) => Some(files.len()),
            Err(_) => None,
        }
    }

    pub(crate) fn changes_view(&mut self, ui: &mut egui::Ui, dir: &Path) {
        let theme = self.theme.clone();
        self.changes.drain();
        let mut view = self.changes.views.remove(dir).unwrap_or_default();
        let stale = view.loaded_at.is_none_or(|at| at.elapsed() >= RELOAD);
        if stale && !view.diff_loading {
            match self.changes.load(ui.ctx(), dir) {
                Ok(()) => view.diff_loading = true,
                Err(e) => view.diff = Some(Err(e.to_string())),
            }
        }
        ui.ctx().request_repaint_after(RELOAD);
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = kit::S2;
            kit::icon(ui, Icon::FolderOpen, 14.0, style::c(theme.dim));
            kit::mono(ui, &dir.display().to_string(), 12.0, style::c(theme.dim));
            if let Some(Ok(files)) = &view.diff {
                let (n, added, removed) = totals(files);
                if n > 0 {
                    kit::pill(ui, &theme, &format!("{n} changed"), style::c(theme.working));
                    kit::mono(ui, &format!("+{added}"), 11.5, style::c(theme.done));
                    kit::mono(ui, &format!("−{removed}"), 11.5, style::c(theme.danger));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reload = kit::icon_button(ui, &theme, Icon::Refresh, "Reload").clicked();
                if kit::icon_button(ui, &theme, Icon::Editor, "Open the worktree in the editor")
                    .clicked()
                {
                    self.queue.push(artalde_core::command::Command::new(
                        "path.edit",
                        Some(&dir.display().to_string()),
                    ));
                }
            });
        });
        if reload {
            view.loaded_at = None;
        }
        ui.add_space(kit::S2);
        match view.diff.clone() {
            None => kit::empty_with(ui, &theme, Icon::Loader, "Reading the worktree…", ""),
            Some(Err(e)) => {
                kit::empty_with(ui, &theme, Icon::Warning, "No changes to show", &e);
            }
            Some(Ok(files)) if files.is_empty() => kit::empty_with(
                ui,
                &theme,
                Icon::CircleCheck,
                "Clean worktree",
                "Nothing differs from HEAD.",
            ),
            Some(Ok(files)) => {
                let local = PrRef {
                    repo: dir.display().to_string(),
                    number: 0,
                };
                self.files_split(ui, &local, &mut view, &files);
            }
        }
        self.changes.views.insert(dir.to_path_buf(), view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_add_up() {
        let files = [
            DiffFile {
                added: 2,
                removed: 1,
                ..DiffFile::default()
            },
            DiffFile {
                added: 5,
                ..DiffFile::default()
            },
        ];
        assert_eq!(totals(&files), (2, 7, 1));
    }
}
