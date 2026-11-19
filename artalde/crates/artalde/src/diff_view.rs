//! The Files tab of a PR: a file tree grouped by directory with status and
//! diffstat, and a diff with a shaded gutter, a sign column, light syntax
//! highlighting and the changed words of each modified line marked.

use std::ops::Range;

use artalde_core::github::{DiffFile, DiffLine, FileStatus, LineKind, PrRef, Thread};
use artalde_core::review::{Draft, DraftState, Origin, Side};
use artalde_core::theme::Theme;
use eframe::egui::{
    self, Color32, CornerRadius, FontId, RichText, Sense, Stroke, TextFormat, text::LayoutJob,
};

use crate::app::App;
use crate::gh::Req;
use crate::kit::{self, Col};
use crate::style::{self, tint};
use crate::views::PrView;

const ROW_H: f32 = 20.0;
const CODE_SIZE: f32 = 12.5;
const TREE_W: f32 = 300.0;
const TAB_WIDTH: usize = 4;
/// Lines longer than this are not tokenised (minified files).
const MAX_HIGHLIGHT: usize = 600;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Punct,
}

/// What a file's extension says about its syntax.
struct Lang {
    keywords: &'static [&'static str],
    line_comment: &'static [&'static str],
}

const C_LIKE: &[&str] = &[
    "fn",
    "let",
    "mut",
    "pub",
    "use",
    "mod",
    "impl",
    "struct",
    "enum",
    "trait",
    "match",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "return",
    "break",
    "continue",
    "const",
    "static",
    "async",
    "await",
    "move",
    "where",
    "type",
    "as",
    "in",
    "self",
    "Self",
    "crate",
    "super",
    "unsafe",
    "dyn",
    "ref",
    "true",
    "false",
    "function",
    "var",
    "class",
    "interface",
    "extends",
    "implements",
    "import",
    "export",
    "from",
    "default",
    "new",
    "this",
    "null",
    "undefined",
    "typeof",
    "instanceof",
    "void",
    "try",
    "catch",
    "finally",
    "throw",
    "switch",
    "case",
    "readonly",
    "private",
    "public",
    "protected",
    "abstract",
    "package",
    "func",
    "go",
    "defer",
    "chan",
    "map",
    "range",
    "struct",
    "interface",
    "nil",
    "def",
    "lambda",
    "None",
    "True",
    "False",
    "and",
    "or",
    "not",
    "is",
    "with",
    "yield",
    "elif",
    "pass",
    "raise",
    "except",
    "do",
    "then",
    "fi",
    "done",
    "esac",
    "echo",
    "local",
    "select",
    "where",
    "insert",
    "update",
    "delete",
    "values",
    "join",
    "on",
    "create",
    "table",
    "alter",
    "index",
];
const NO_KEYWORDS: &[&str] = &[];

fn lang(path: &str) -> Lang {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go" | "java" | "kt" | "swift"
        | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "scala" | "dart" | "proto" | "rhai" | "css"
        | "scss" => Lang {
            keywords: C_LIKE,
            line_comment: &["//"],
        },
        "py" | "rb" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" | "dockerfile" | "tf"
        | "mk" | "make" => Lang {
            keywords: C_LIKE,
            line_comment: &["#"],
        },
        "sql" => Lang {
            keywords: C_LIKE,
            line_comment: &["--"],
        },
        "json" | "md" | "txt" | "lock" | "csv" => Lang {
            keywords: NO_KEYWORDS,
            line_comment: &[],
        },
        _ => Lang {
            keywords: C_LIKE,
            line_comment: &["//", "#"],
        },
    }
}

/// Byte ranges of `line` with their token kind.
pub fn tokens(line: &str, path: &str) -> Vec<(Range<usize>, Tok)> {
    if line.len() > MAX_HIGHLIGHT {
        return vec![(0..line.len(), Tok::Plain)];
    }
    let l = lang(path);
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut plain_start = 0;
    let push = |out: &mut Vec<(Range<usize>, Tok)>, r: Range<usize>, t: Tok| {
        if !r.is_empty() {
            out.push((r, t));
        }
    };
    while i < bytes.len() {
        let rest = line.get(i..).unwrap_or_default();
        if let Some(c) = l.line_comment.iter().find(|c| rest.starts_with(**c)) {
            let _ = c;
            push(&mut out, plain_start..i, Tok::Plain);
            push(&mut out, i..line.len(), Tok::Comment);
            return out;
        }
        if rest.starts_with("/*")
            || rest.starts_with("*/")
            || (rest.starts_with("* ") && line.trim_start().starts_with('*'))
        {
            push(&mut out, plain_start..i, Tok::Plain);
            push(&mut out, i..line.len(), Tok::Comment);
            return out;
        }
        let c = bytes.get(i).copied().unwrap_or(b' ');
        if c == b'"' || c == b'\'' || c == b'`' {
            push(&mut out, plain_start..i, Tok::Plain);
            let start = i;
            i += 1;
            while i < bytes.len() {
                let b = bytes.get(i).copied().unwrap_or(c);
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                i += 1;
                if b == c {
                    break;
                }
            }
            i = i.min(line.len());
            push(&mut out, start..i, Tok::String);
            plain_start = i;
            continue;
        }
        if c.is_ascii_digit()
            && (i == 0
                || !bytes
                    .get(i - 1)
                    .is_some_and(|p| p.is_ascii_alphanumeric() || *p == b'_'))
        {
            push(&mut out, plain_start..i, Tok::Plain);
            let start = i;
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'.' || *b == b'_')
            {
                i += 1;
            }
            push(&mut out, start..i, Tok::Number);
            plain_start = i;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                i += 1;
            }
            let word = line.get(start..i).unwrap_or_default();
            let kind = if l.keywords.contains(&word) {
                Some(Tok::Keyword)
            } else if word.chars().next().is_some_and(char::is_uppercase)
                && word.chars().any(char::is_lowercase)
            {
                Some(Tok::Type)
            } else {
                None
            };
            if let Some(k) = kind {
                push(&mut out, plain_start..start, Tok::Plain);
                push(&mut out, start..i, k);
                plain_start = i;
            }
            continue;
        }
        if "{}()[];,.:=<>!&|+-*/%?".contains(c as char) {
            push(&mut out, plain_start..i, Tok::Plain);
            push(&mut out, i..i + 1, Tok::Punct);
            plain_start = i + 1;
        }
        i += line
            .get(i..)
            .and_then(|r| r.chars().next())
            .map_or(1, char::len_utf8);
    }
    push(&mut out, plain_start..line.len(), Tok::Plain);
    out
}

/// Like [`tokens`], for a line that may start inside a backtick string
/// left open by an earlier line.
pub fn tokens_from(line: &str, path: &str, in_template: bool) -> Vec<(Range<usize>, Tok)> {
    if !in_template {
        return tokens(line, path);
    }
    match unescaped_backtick(line) {
        None => vec![(0..line.len(), Tok::String)],
        Some(close) => {
            let mut out = vec![(0..close + 1, Tok::String)];
            let rest = line.get(close + 1..).unwrap_or_default();
            out.extend(
                tokens(rest, path)
                    .into_iter()
                    .map(|(r, t)| (r.start + close + 1..r.end + close + 1, t)),
            );
            out
        }
    }
}

fn unescaped_backtick(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (0..b.len()).find(|&i| b.get(i) == Some(&b'`') && (i == 0 || b.get(i - 1) != Some(&b'\\')))
}

/// Whether a backtick string is open after `line`, given it was `open`
/// before. Only for languages with backtick strings.
fn template_after(line: &str, open: bool) -> bool {
    let b = line.as_bytes();
    let count = (0..b.len())
        .filter(|&i| b.get(i) == Some(&b'`') && (i == 0 || b.get(i - 1) != Some(&b'\\')))
        .count();
    open ^ (count % 2 == 1)
}

/// For each diff line, whether it starts inside a backtick string: the old
/// side (context + removed) and the new side (context + added) are tracked
/// apart, and a hunk resets both.
fn template_state(lines: &[DiffLine], path: &str) -> Vec<bool> {
    let ext = path.rsplit('.').next().unwrap_or_default();
    if !matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "go") {
        return vec![false; lines.len()];
    }
    let (mut old, mut new) = (false, false);
    lines
        .iter()
        .map(|l| {
            let text = l.text.get(1..).unwrap_or_default();
            match l.kind {
                LineKind::Hunk | LineKind::Meta => {
                    // `@@ … @@ const X = \`` names the enclosing line: a
                    // backtick left open there is still open here.
                    let context = l.text.splitn(3, "@@").nth(2).unwrap_or_default();
                    old = template_after(context, false);
                    new = old;
                    false
                }
                LineKind::Remove => {
                    let start = old;
                    old = template_after(text, old);
                    start
                }
                LineKind::Add => {
                    let start = new;
                    new = template_after(text, new);
                    start
                }
                LineKind::Context => {
                    let start = new;
                    old = template_after(text, old);
                    new = template_after(text, new);
                    start
                }
            }
        })
        .collect()
}

/// The changed middle of a removed/added line pair: byte ranges in each,
/// after the common prefix and suffix. `None` when the lines share
/// too little to be worth marking.
pub fn intraline(old: &str, new: &str) -> Option<(Range<usize>, Range<usize>)> {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, a), b)| a == b)
        .last()
        .map_or(0, |((i, a), _)| i + a.len_utf8());
    let o_rest = old.get(prefix..)?;
    let n_rest = new.get(prefix..)?;
    let suffix = o_rest
        .chars()
        .rev()
        .zip(n_rest.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let o_end = old.len().saturating_sub(suffix).max(prefix);
    let n_end = new.len().saturating_sub(suffix).max(prefix);
    let shared = prefix + suffix;
    if shared * 3 < old.len().max(new.len()) || (o_end == prefix && n_end == prefix) {
        return None;
    }
    Some((prefix..o_end, prefix..n_end))
}

fn tok_color(theme: &Theme, t: Tok) -> Color32 {
    let text = style::c(theme.text);
    match t {
        Tok::Plain => text,
        Tok::Keyword => tint(text, style::c(theme.accent), 0.85),
        Tok::String => tint(text, style::c(theme.done), 0.8),
        Tok::Comment => style::c(theme.dim),
        Tok::Number => tint(text, style::c(theme.blocked), 0.8),
        Tok::Type => tint(text, style::c(theme.working), 0.7),
        Tok::Punct => tint(text, style::c(theme.dim), 0.5),
    }
}

fn status_color(theme: &Theme, s: FileStatus) -> Color32 {
    style::c(match s {
        FileStatus::Added => theme.done,
        FileStatus::Deleted => theme.danger,
        FileStatus::Renamed => theme.accent,
        FileStatus::Modified => theme.blocked,
    })
}

/// A five-block diffstat bar, GitHub-style.
fn diffstat(ui: &mut egui::Ui, theme: &Theme, added: u32, removed: u32) {
    let total = (added + removed).max(1) as f32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0..=5 blocks"
    )]
    let green = ((added as f32 / total) * 5.0).round() as usize;
    let red = if removed > 0 {
        (5 - green).clamp(1, 5)
    } else {
        0
    };
    let green = green.min(5 - red);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(5.0 * 7.0, 10.0), Sense::hover());
    for i in 0..5 {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + i as f32 * 7.0, rect.min.y + 2.0),
            egui::vec2(6.0, 6.0),
        );
        let c = if i < green {
            style::c(theme.done)
        } else if i < green + red {
            style::c(theme.danger)
        } else {
            tint(style::c(theme.surface), style::c(theme.text), 0.12)
        };
        ui.painter().rect_filled(r, CornerRadius::same(1), c);
    }
}

/// `…/prompts/toolFree/` for long directories.
fn short_dir(dir: &str, max: usize) -> String {
    if dir.chars().count() <= max {
        return dir.to_owned();
    }
    let parts: Vec<&str> = dir.trim_end_matches('/').split('/').collect();
    let mut out = String::new();
    for p in parts.iter().rev() {
        let next = format!("{p}/{out}");
        if next.chars().count() + 2 > max {
            break;
        }
        out = next;
    }
    format!("…/{out}")
}

impl App {
    pub(crate) fn pr_files_view(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView) {
        let theme = self.theme.clone();
        if view.diff.is_none() && !view.diff_loading {
            match self.gh.send(Req::Diff(pr.clone())) {
                Ok(()) => view.diff_loading = true,
                Err(e) => view.diff = Some(Err(e)),
            }
        }
        let files = match &view.diff {
            None => {
                kit::empty(ui, &theme, "Loading the diff…", "gh pr diff");
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(style::c(theme.danger)));
                if kit::button(ui, &theme, "Retry", None).clicked() {
                    view.diff = None;
                }
                return;
            }
            Some(Ok(f)) => f.clone(),
        };
        if files.is_empty() {
            kit::empty(ui, &theme, "No changes", "This PR's diff is empty.");
            return;
        }
        // [ and ] step through files.
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if !typing {
            let (prev, next) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket),
                )
            });
            if prev {
                view.file = view.file.saturating_sub(1);
            }
            if next {
                view.file = (view.file + 1).min(files.len() - 1);
            }
        }
        if let Some((path, line)) = view.jump.take()
            && let Some(i) = files.iter().position(|f| f.path == path)
        {
            view.file = i;
            view.jump_line = Some(line);
        }
        view.file = view.file.min(files.len() - 1);
        let rect = ui.available_rect_before_wrap();
        let tree_w = TREE_W.min(rect.width() * 0.32);
        let tree = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + tree_w, rect.max.y));
        let body = egui::Rect::from_min_max(egui::pos2(tree.max.x + 14.0, rect.min.y), rect.max);
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(tree));
        self.file_tree(&mut left, pr, view, &files);
        ui.painter().vline(
            tree.max.x + 7.0,
            rect.y_range(),
            Stroke::new(1.0, style::c(theme.border)),
        );
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(body));
        if let Some(f) = files.get(view.file) {
            self.file_diff(&mut right, pr, view, f, files.len());
        }
    }

    fn file_tree(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView, files: &[DiffFile]) {
        let theme = self.theme.clone();
        let added: u32 = files.iter().map(|f| f.added).sum();
        let removed: u32 = files.iter().map(|f| f.removed).sum();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} files", files.len())).font(crate::fonts::semibold(13.0)),
            );
            ui.label(
                RichText::new(format!("+{added}"))
                    .monospace()
                    .size(11.5)
                    .color(style::c(theme.done)),
            );
            ui.label(
                RichText::new(format!("−{removed}"))
                    .monospace()
                    .size(11.5)
                    .color(style::c(theme.danger)),
            );
        });
        ui.add_space(kit::S1);
        ui.add(
            egui::TextEdit::singleline(&mut view.file_filter)
                .hint_text("Filter files…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(kit::S1);
        let needle = view.file_filter.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("tree", pr.to_string()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut last_dir: Option<String> = None;
                for (i, f) in files.iter().enumerate() {
                    if !needle.is_empty() && !f.path.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let (dir, name) = match f.path.rsplit_once('/') {
                        Some((d, n)) => (format!("{d}/"), n.to_owned()),
                        None => (String::new(), f.path.clone()),
                    };
                    if last_dir.as_deref() != Some(dir.as_str()) {
                        if !dir.is_empty() {
                            ui.add_space(kit::S1);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(short_dir(&dir, 40))
                                        .monospace()
                                        .size(10.5)
                                        .color(style::c(theme.dim)),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&dir);
                        }
                        last_dir = Some(dir.clone());
                    }
                    let on = view.file == i;
                    let resp = kit::row(
                        ui,
                        &theme,
                        ("file", i),
                        26.0,
                        on,
                        &[Col::Px(14.0), Col::Flex, Col::Px(36.0), Col::Px(64.0)],
                        |c, ui| match c {
                            0 => {
                                ui.label(
                                    RichText::new(f.status.letter())
                                        .monospace()
                                        .size(11.0)
                                        .color(status_color(&theme, f.status)),
                                );
                            }
                            1 => {
                                let color = if f.status == FileStatus::Deleted {
                                    style::c(theme.dim)
                                } else {
                                    style::c(theme.text)
                                };
                                kit::text(ui, &name, 12.5, color);
                            }
                            2 => diffstat(ui, &theme, f.added, f.removed),
                            _ => {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        if f.removed > 0 {
                                            ui.label(
                                                RichText::new(format!("−{}", f.removed))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(style::c(theme.danger)),
                                            );
                                        }
                                        if f.added > 0 {
                                            ui.label(
                                                RichText::new(format!("+{}", f.added))
                                                    .monospace()
                                                    .size(10.5)
                                                    .color(style::c(theme.done)),
                                            );
                                        }
                                    },
                                );
                            }
                        },
                    );
                    if resp.on_hover_text(&f.path).clicked() {
                        view.file = i;
                    }
                }
            });
    }

    fn file_diff(
        &mut self,
        ui: &mut egui::Ui,
        pr: &PrRef,
        view: &mut PrView,
        f: &DiffFile,
        total: usize,
    ) {
        let theme = self.theme.clone();
        // Header.
        let header = egui::Frame::new()
            .fill(style::c(theme.raised))
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            })
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    kit::pill(
                        ui,
                        &theme,
                        match f.status {
                            FileStatus::Added => "added",
                            FileStatus::Deleted => "deleted",
                            FileStatus::Renamed => "renamed",
                            FileStatus::Modified => "modified",
                        },
                        status_color(&theme, f.status),
                    );
                    let (dir, name) = f
                        .path
                        .rsplit_once('/')
                        .map_or((String::new(), f.path.clone()), |(d, n)| {
                            (format!("{d}/"), n.to_owned())
                        });
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(
                        RichText::new(dir)
                            .monospace()
                            .size(12.0)
                            .color(style::c(theme.dim)),
                    );
                    ui.label(RichText::new(name).monospace().size(12.5).strong());
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("+{}", f.added))
                            .monospace()
                            .size(11.5)
                            .color(style::c(theme.done)),
                    );
                    ui.label(
                        RichText::new(format!("−{}", f.removed))
                            .monospace()
                            .size(11.5)
                            .color(style::c(theme.danger)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::button(ui, &theme, "›", Some("]"))
                            .on_hover_text("Next file")
                            .clicked()
                        {
                            view.file = (view.file + 1).min(total - 1);
                        }
                        if kit::button(ui, &theme, "‹", Some("["))
                            .on_hover_text("Previous file")
                            .clicked()
                        {
                            view.file = view.file.saturating_sub(1);
                        }
                        ui.label(
                            RichText::new(format!("{} / {total}", view.file + 1))
                                .size(11.5)
                                .color(style::c(theme.dim)),
                        );
                        ui.checkbox(&mut view.wrap, RichText::new("wrap").size(12.0));
                        if kit::button(ui, &theme, "Copy path", None).clicked() {
                            ui.ctx().copy_text(f.path.clone());
                        }
                        if kit::button(ui, &theme, "GitHub ↗", None).clicked() {
                            self.queue.push(artalde_core::command::Command::new(
                                "url.open",
                                Some(&format!("{}/files", pr.url())),
                            ));
                        }
                    });
                });
                if let Some(old) = &f.old_path {
                    ui.label(
                        RichText::new(format!("renamed from {old}"))
                            .monospace()
                            .size(11.0)
                            .color(style::c(theme.dim)),
                    );
                }
            });
        let _ = header;
        // Body.
        let code_bg = style::c(theme.background);
        let body_rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(
            body_rect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 8,
                se: 8,
            },
            code_bg,
        );
        ui.painter().rect_stroke(
            body_rect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 8,
                se: 8,
            },
            Stroke::new(1.0, style::c(theme.border)),
            egui::StrokeKind::Inside,
        );
        if f.lines.is_empty() {
            kit::empty(
                ui,
                &theme,
                "No text changes",
                "Binary file, mode change or pure rename.",
            );
            return;
        }
        let font = FontId::monospace(CODE_SIZE);
        let char_w = ui.fonts_mut(|fo| fo.glyph_width(&font, 'm')).max(1.0);
        let digits = f
            .lines
            .iter()
            .filter_map(|l| l.new.max(l.old))
            .max()
            .unwrap_or(1)
            .to_string()
            .len()
            .max(3);
        let num_w = digits as f32 * char_w + 12.0;
        let gutter_w = num_w * 2.0;
        let sign_w = char_w + 10.0;
        let max_chars = f
            .lines
            .iter()
            .map(|l| l.text.chars().count() + TAB_WIDTH)
            .max()
            .unwrap_or(80);
        let pairs = pair_changes(&f.lines);
        let templates = template_state(&f.lines, &f.path);
        let wrap = view.wrap;
        let mut area = egui::ScrollArea::both()
            .id_salt(("diff", pr.to_string(), view.file))
            .auto_shrink([false, false]);
        if wrap {
            area = egui::ScrollArea::vertical()
                .id_salt(("diff-wrap", pr.to_string(), view.file))
                .auto_shrink([false, false]);
        }
        let row = |ui: &mut egui::Ui, idx: usize, line: &DiffLine| -> Option<(u32, Side, String)> {
            let (bg, gutter_bg, sign, sign_color) = match line.kind {
                LineKind::Add => (
                    tint(code_bg, style::c(theme.done), 0.10),
                    tint(code_bg, style::c(theme.done), 0.20),
                    "+",
                    style::c(theme.done),
                ),
                LineKind::Remove => (
                    tint(code_bg, style::c(theme.danger), 0.10),
                    tint(code_bg, style::c(theme.danger), 0.20),
                    "−",
                    style::c(theme.danger),
                ),
                LineKind::Hunk => (
                    tint(code_bg, style::c(theme.accent), 0.06),
                    tint(code_bg, style::c(theme.accent), 0.10),
                    "",
                    style::c(theme.dim),
                ),
                _ => (
                    code_bg,
                    tint(code_bg, style::c(theme.text), 0.03),
                    "",
                    style::c(theme.dim),
                ),
            };
            let content = if matches!(line.kind, LineKind::Hunk | LineKind::Meta) {
                line.text.clone()
            } else {
                line.text
                    .get(1..)
                    .unwrap_or_default()
                    .replace('\t', &" ".repeat(TAB_WIDTH))
            };
            let text_w = ui.available_width() - gutter_w - sign_w - 8.0;
            let mut job = LayoutJob::default();
            if wrap {
                job.wrap.max_width = text_w.max(80.0);
            }
            if line.kind == LineKind::Hunk {
                // `@@ -17,48 +23,222 @@ const NAMES` → the context, quietly.
                let ctx = content.splitn(3, "@@").nth(2).unwrap_or_default().trim();
                let ranges = content.split("@@").nth(1).unwrap_or_default().trim();
                job.append(
                    ranges,
                    0.0,
                    TextFormat::simple(FontId::monospace(CODE_SIZE - 1.0), style::c(theme.accent)),
                );
                if !ctx.is_empty() {
                    job.append(
                        &format!("   {ctx}"),
                        0.0,
                        TextFormat::simple(FontId::monospace(CODE_SIZE - 1.0), style::c(theme.dim)),
                    );
                }
            } else {
                let marked = pairs.get(&idx).cloned();
                for (r, t) in tokens_from(
                    &content,
                    &f.path,
                    templates.get(idx).copied().unwrap_or(false),
                ) {
                    let piece = content.get(r.clone()).unwrap_or_default();
                    // Split each token at the changed-range edges.
                    let mut cuts = vec![r.start, r.end];
                    if let Some(m) = &marked {
                        for e in [m.start, m.end] {
                            if e > r.start && e < r.end {
                                cuts.push(e);
                            }
                        }
                    }
                    cuts.sort_unstable();
                    cuts.dedup();
                    let _ = piece;
                    for w in cuts.windows(2) {
                        let (a, b) = (w[0], w[1]);
                        let mut fmt = TextFormat::simple(font.clone(), tok_color(&theme, t));
                        if marked
                            .as_ref()
                            .is_some_and(|m| a >= m.start && b <= m.end && m.start < m.end)
                        {
                            fmt.background = match line.kind {
                                LineKind::Add => tint(code_bg, style::c(theme.done), 0.32),
                                _ => tint(code_bg, style::c(theme.danger), 0.32),
                            };
                        }
                        job.append(content.get(a..b).unwrap_or_default(), 0.0, fmt);
                    }
                }
            }
            let galley = ui.fonts_mut(|fo| fo.layout_job(job));
            let h = galley.size().y.max(ROW_H - 4.0) + 4.0;
            let w = if wrap {
                ui.available_width()
            } else {
                (gutter_w + sign_w + 8.0 + max_chars as f32 * char_w).max(ui.available_width())
            };
            let raw = line.text.get(1..).unwrap_or_default().to_owned();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
            let target = match line.kind {
                LineKind::Add | LineKind::Context => line.new.map(|n| (n, Side::Right)),
                LineKind::Remove => line.old.map(|n| (n, Side::Left)),
                _ => None,
            };
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::ZERO, bg);
            let g = egui::Rect::from_min_size(rect.min, egui::vec2(gutter_w, h));
            p.rect_filled(g, CornerRadius::ZERO, gutter_bg);
            let num_color = tint(style::c(theme.dim), bg, 0.3);
            let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
            p.text(
                egui::pos2(rect.min.x + num_w - 6.0, rect.min.y + 2.0),
                egui::Align2::RIGHT_TOP,
                num(line.old),
                FontId::monospace(CODE_SIZE - 1.5),
                num_color,
            );
            p.text(
                egui::pos2(rect.min.x + gutter_w - 6.0, rect.min.y + 2.0),
                egui::Align2::RIGHT_TOP,
                num(line.new),
                FontId::monospace(CODE_SIZE - 1.5),
                num_color,
            );
            p.text(
                egui::pos2(rect.min.x + gutter_w + 6.0, rect.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                sign,
                font.clone(),
                sign_color,
            );
            p.galley(
                egui::pos2(rect.min.x + gutter_w + sign_w, rect.min.y + 2.0),
                galley,
                style::c(theme.text),
            );
            if line.kind == LineKind::Hunk {
                p.hline(
                    rect.x_range(),
                    rect.min.y,
                    Stroke::new(1.0, tint(code_bg, style::c(theme.border), 0.8)),
                );
            }
            if target.is_some() && resp.hovered() {
                // GitHub's blue "+" on the hovered line.
                let c = egui::pos2(rect.min.x + gutter_w + sign_w * 0.5, rect.min.y + h / 2.0);
                p.rect_filled(
                    egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0)),
                    CornerRadius::same(4),
                    style::c(theme.accent),
                );
                p.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "+",
                    FontId::proportional(13.0),
                    style::c(theme.background),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                return target.map(|(n, s)| (n, s, raw));
            }
            None
        };
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body_rect.shrink(1.0)));
        // Drafts, review threads and an open composer render under their
        // lines, so the diff lays out every line when any are present.
        let drafts: Vec<Draft> = view
            .review
            .as_ref()
            .map(|r| {
                r.drafts
                    .iter()
                    .filter(|d| d.path == f.path && d.state != DraftState::Dismissed)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let threads: Vec<Thread> = view
            .detail
            .as_ref()
            .map(|d| {
                d.threads
                    .iter()
                    .filter(|t| t.path == f.path && !t.outdated)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let composing = view
            .inline_at
            .as_ref()
            .is_some_and(|(p, _, _)| *p == f.path);
        let rich = wrap || composing || !drafts.is_empty() || !threads.is_empty();
        if let Some(line) = view.jump_line.take()
            && let Some(idx) = f
                .lines
                .iter()
                .position(|l| l.new == Some(line) || l.old == Some(line))
        {
            area = area.vertical_scroll_offset((idx as f32 * ROW_H - 120.0).max(0.0));
        }
        let mut clicked = None;
        if rich {
            area.show(&mut inner, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, l) in f.lines.iter().enumerate() {
                    if let Some(t) = row(ui, i, l) {
                        clicked = Some(t);
                    }
                    let at = match l.kind {
                        LineKind::Add | LineKind::Context => l.new.map(|n| (n, Side::Right)),
                        LineKind::Remove => l.old.map(|n| (n, Side::Left)),
                        _ => None,
                    };
                    let Some((n, side)) = at else { continue };
                    let here_threads: Vec<&Thread> = threads
                        .iter()
                        .filter(|t| side == Side::Right && t.line == Some(u64::from(n)))
                        .collect();
                    let here_drafts: Vec<&Draft> = drafts
                        .iter()
                        .filter(|d| d.line == n && d.side == side)
                        .collect();
                    let compose_here = view
                        .inline_at
                        .as_ref()
                        .is_some_and(|(p, ln, sd)| *p == f.path && *ln == n && *sd == side);
                    if here_threads.is_empty() && here_drafts.is_empty() && !compose_here {
                        continue;
                    }
                    egui::Frame::new()
                        .fill(style::c(theme.surface))
                        .inner_margin(egui::Margin {
                            left: 16,
                            right: 16,
                            top: 6,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            ui.set_max_width((ui.available_width()).min(900.0));
                            ui.spacing_mut().item_spacing.y = 4.0;
                            for t in here_threads {
                                self.inline_thread(ui, t);
                            }
                            for d in here_drafts {
                                self.draft_card(ui, pr, view, d, false);
                            }
                            if compose_here {
                                self.inline_composer(ui, pr, view);
                            }
                        });
                }
            });
        } else {
            area.show_rows(&mut inner, ROW_H, f.lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in range {
                    if let Some(l) = f.lines.get(i)
                        && let Some(t) = row(ui, i, l)
                    {
                        clicked = Some(t);
                    }
                }
            });
        }
        if let Some((n, side, text)) = clicked {
            view.inline_at = Some((f.path.clone(), n, side));
            view.inline_line = text;
            view.inline_text.clear();
        }
    }

    /// An existing GitHub review thread, compact, under its line.
    fn inline_thread(&mut self, ui: &mut egui::Ui, t: &Thread) {
        let theme = self.theme.clone();
        let now = artalde_core::now();
        let edge = if t.resolved {
            style::c(theme.dim)
        } else {
            style::c(theme.blocked)
        };
        let resp = egui::Frame::new()
            .fill(style::c(theme.raised))
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin {
                left: 14,
                right: 10,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for c in &t.comments {
                    ui.horizontal(|ui| {
                        let who = if c.bot {
                            artalde_core::github::bot_name(&c.author)
                        } else {
                            format!("@{}", c.author)
                        };
                        ui.label(RichText::new(who).font(crate::fonts::semibold(12.5)));
                        ui.label(
                            RichText::new(artalde_core::github::age(&c.created, now))
                                .size(11.0)
                                .color(style::c(theme.dim)),
                        );
                        if t.resolved {
                            ui.label(
                                RichText::new("resolved")
                                    .size(11.0)
                                    .color(style::c(theme.done)),
                            );
                        }
                    });
                    self.md(ui, &c.body, 13.0);
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
    }

    /// GitHub's line composer: write, suggest, add to the review.
    fn inline_composer(&mut self, ui: &mut egui::Ui, pr: &PrRef, view: &mut PrView) {
        let theme = self.theme.clone();
        let Some((path, line, side)) = view.inline_at.clone() else {
            return;
        };
        egui::Frame::new()
            .fill(style::c(theme.raised))
            .stroke(Stroke::new(1.0, style::c(theme.accent)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "Comment on line {line}{}",
                        if side == Side::Left { " (old)" } else { "" }
                    ))
                    .size(11.5)
                    .color(style::c(theme.dim)),
                );
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut view.inline_text)
                        .hint_text("Markdown. ⌘↩ adds to the review.")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                edit.request_focus();
                let submit = edit.has_focus()
                    && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let ok = !view.inline_text.trim().is_empty();
                    let add = ui
                        .add_enabled_ui(ok, |ui| {
                            kit::primary(ui, &theme, "Add to review", style::c(theme.accent))
                                .clicked()
                        })
                        .inner;
                    if (add || submit) && ok {
                        let body = std::mem::take(&mut view.inline_text);
                        let (suggestion, body) = split_suggestion(&body);
                        self.pending(pr, view).add(Draft {
                            path: path.clone(),
                            line,
                            side,
                            body,
                            suggestion,
                            origin: Origin::Me,
                            state: DraftState::Accepted,
                            ..Draft::default()
                        });
                        view.inline_at = None;
                        self.save_review(pr, view);
                    }
                    if side == Side::Right
                        && kit::button(ui, &theme, "± Suggest change", None).clicked()
                    {
                        view.inline_text = format!(
                            "{}\n\n```suggestion\n{}\n```",
                            view.inline_text.trim_end(),
                            view.inline_line
                        );
                    }
                    if kit::button(ui, &theme, "Cancel", Some("esc")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        view.inline_at = None;
                    }
                });
            });
    }
}

/// A body's trailing ```suggestion block, split out.
fn split_suggestion(body: &str) -> (Option<String>, String) {
    match body.split_once("```suggestion") {
        Some((before, rest)) => {
            let code = rest
                .trim_start_matches('\n')
                .split("```")
                .next()
                .unwrap_or_default()
                .trim_end_matches('\n')
                .to_owned();
            (Some(code), before.trim().to_owned())
        }
        None => (None, body.trim().to_owned()),
    }
}

/// For each removed/added line that pairs with one on the other side of
/// the same change block, the byte range (in the line without its sign)
/// that changed.
fn pair_changes(lines: &[DiffLine]) -> std::collections::HashMap<usize, Range<usize>> {
    let mut out = std::collections::HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        if lines.get(i).is_some_and(|l| l.kind == LineKind::Remove) {
            let rem_start = i;
            while lines.get(i).is_some_and(|l| l.kind == LineKind::Remove) {
                i += 1;
            }
            let add_start = i;
            while lines.get(i).is_some_and(|l| l.kind == LineKind::Add) {
                i += 1;
            }
            let removed = add_start - rem_start;
            let added = i - add_start;
            for k in 0..removed.min(added) {
                let (Some(o), Some(n)) = (lines.get(rem_start + k), lines.get(add_start + k))
                else {
                    continue;
                };
                let ot = o
                    .text
                    .get(1..)
                    .unwrap_or_default()
                    .replace('\t', &" ".repeat(TAB_WIDTH));
                let nt = n
                    .text
                    .get(1..)
                    .unwrap_or_default()
                    .replace('\t', &" ".repeat(TAB_WIDTH));
                if let Some((a, b)) = intraline(&ot, &nt) {
                    out.insert(rem_start + k, a);
                    out.insert(add_start + k, b);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_cover_the_line() {
        let line = r#"let x: Vec<u8> = parse("a\"b", 42); // done"#;
        let toks = tokens(line, "a.rs");
        let joined: String = toks.iter().map(|(r, _)| &line[r.clone()]).collect();
        assert_eq!(joined, line);
        let kind = |s: &str| {
            toks.iter()
                .find(|(r, _)| &line[r.clone()] == s)
                .map(|(_, t)| *t)
        };
        assert_eq!(kind("let"), Some(Tok::Keyword));
        assert_eq!(kind("Vec"), Some(Tok::Type));
        assert_eq!(kind(r#""a\"b""#), Some(Tok::String));
        assert_eq!(kind("42"), Some(Tok::Number));
        assert_eq!(kind("// done"), Some(Tok::Comment));
        assert_eq!(tokens("# heading", "x.yaml")[0].1, Tok::Comment);
    }

    #[test]
    fn intraline_marks_the_changed_middle() {
        let (a, b) = intraline(".replace(X, render(t))", ".replace(X, () => render(t))").unwrap();
        assert_eq!((a.start, a.end), (12, 12));
        assert_eq!(&".replace(X, () => render(t))"[b], "() => ");
        assert!(intraline("completely", "different").is_none());
    }

    #[test]
    fn template_strings_span_lines() {
        let lines: Vec<DiffLine> = [" const A = `", "+  and in prose", " ` + x"]
            .iter()
            .map(|t| DiffLine {
                kind: if t.starts_with('+') {
                    LineKind::Add
                } else {
                    LineKind::Context
                },
                text: (*t).to_owned(),
                old: None,
                new: None,
            })
            .collect();
        assert_eq!(template_state(&lines, "a.ts"), [false, true, true]);
        let hunk = DiffLine {
            kind: LineKind::Hunk,
            text: "@@ -17,4 +23,5 @@ const NAMES = `".into(),
            old: None,
            new: None,
        };
        let ctx = DiffLine {
            kind: LineKind::Context,
            text: "  then prose".into(),
            old: None,
            new: None,
        };
        assert_eq!(
            template_state(&[hunk, ctx], "a.ts"),
            [false, true],
            "a hunk inside a template starts inside it"
        );
        let t = tokens_from("` + x", "a.ts", true);
        assert_eq!(t[0], (0..1, Tok::String));
        assert_eq!(
            tokens_from("and in prose", "a.ts", true),
            [(0..12, Tok::String)]
        );
    }

    #[test]
    fn suggestions_split_out_of_bodies() {
        assert_eq!(
            split_suggestion("use a fn\n\n```suggestion\nx();\n```"),
            (Some("x();".into()), "use a fn".into())
        );
        assert_eq!(split_suggestion("plain"), (None, "plain".into()));
    }

    #[test]
    fn dirs_shorten_from_the_left() {
        assert_eq!(
            short_dir("server/modules/cv-parser/application/prompts/toolFree/", 24),
            "…/prompts/toolFree/"
        );
        assert_eq!(short_dir("src/", 24), "src/");
    }
}
