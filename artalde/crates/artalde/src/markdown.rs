//! Markdown → egui, themed: agent replies, PR bodies, comments, mods.
//!
//! Paragraphs lay out as one text job (fast, wraps properly); a paragraph
//! with links lays out word by word so each link is its own clickable
//! widget. Raw HTML is dropped except its text; `<!-- comments -->` vanish.

use artalde_core::theme::Theme;
use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Sense, Stroke, TextFormat,
    text::{LayoutJob, TextWrapping},
};
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::fonts;
use crate::style::{self, tint};

/// Longest text rendered; beyond it the rest is cut with a note.
const MAX_CHARS: usize = 120_000;

#[derive(Clone, Copy, Default)]
struct Inline {
    strong: u8,
    emph: u8,
    strike: u8,
    code: bool,
    heading: Option<HeadingLevel>,
}

/// A run of inline content: text with a style, maybe a link target.
struct Run {
    text: String,
    style: Inline,
    link: Option<String>,
}

#[derive(Default)]
struct Block {
    runs: Vec<Run>,
}

impl Block {
    fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.trim().is_empty())
    }
}

struct ListState {
    next: Option<u64>,
}

/// What the renderer asks the caller to do: links to open.
#[derive(Default)]
pub struct Output {
    pub clicked_links: Vec<String>,
}

/// Width of a list item's marker gutter.
const MARKER_W: f32 = 18.0;

/// Render `md` (GitHub-flavoured, HTML included) into `ui`.
pub fn show(ui: &mut egui::Ui, theme: &Theme, md: &str, size: f32) -> Output {
    let mut out = Output::default();
    let segments = crate::gfm::parse(md);
    ui.vertical(|ui| segments_ui(ui, theme, &segments, size, &mut out, 0));
    out
}

fn segments_ui(
    ui: &mut egui::Ui,
    theme: &Theme,
    segs: &[crate::gfm::Segment],
    size: f32,
    out: &mut Output,
    salt: u64,
) {
    use crate::gfm::Segment;
    for (i, seg) in segs.iter().enumerate() {
        let id = salt.wrapping_mul(31).wrapping_add(i as u64 + 1);
        match seg {
            Segment::Markdown(text) => {
                let o = show_plain(ui, theme, text, size);
                out.clicked_links.extend(o.clicked_links);
            }
            Segment::Details {
                summary,
                open,
                body,
            } => {
                let header = egui::RichText::new(summary)
                    .font(fonts::semibold(size * 0.95))
                    .color(style::c(theme.text));
                egui::CollapsingHeader::new(header)
                    .id_salt(("details", id, summary))
                    .default_open(*open)
                    .show(ui, |ui| segments_ui(ui, theme, body, size, out, id));
            }
            Segment::Callout { kind, body } => {
                let color = alert_color(theme, kind);
                let resp = egui::Frame::new()
                    .fill(tint(style::c(theme.surface), color, 0.07))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 10,
                        top: 8,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        if !kind.is_empty() {
                            ui.label(
                                egui::RichText::new(kind.to_lowercase())
                                    .font(fonts::semibold(size * 0.85))
                                    .color(color),
                            );
                        }
                        segments_ui(ui, theme, body, size, out, id);
                    })
                    .response;
                let bar =
                    egui::Rect::from_min_size(resp.rect.min, egui::vec2(3.0, resp.rect.height()));
                ui.painter().rect_filled(
                    bar,
                    CornerRadius {
                        nw: 6,
                        sw: 6,
                        ne: 0,
                        se: 0,
                    },
                    color,
                );
            }
        }
    }
}

fn alert_color(theme: &Theme, kind: &str) -> Color32 {
    style::c(match kind {
        "WARNING" | "CAUTION" => theme.blocked,
        "IMPORTANT" => theme.danger,
        "TIP" => theme.done,
        "NOTE" => theme.accent,
        _ => theme.border,
    })
}

/// Plain markdown (no `<details>` left) into `ui`.
fn show_plain(ui: &mut egui::Ui, theme: &Theme, md: &str, size: f32) -> Output {
    let text = if md.len() > MAX_CHARS {
        let cut = md
            .char_indices()
            .nth(MAX_CHARS)
            .map_or(md.len(), |(i, _)| i);
        format!(
            "{}\n\n*… cut at {MAX_CHARS} characters*",
            md.get(..cut).unwrap_or(md)
        )
    } else {
        md.to_owned()
    };
    let mut r = Renderer {
        theme,
        size,
        out: Output::default(),
        block: Block::default(),
        inline: Inline::default(),
        links: Vec::new(),
        lists: Vec::new(),
        quote: 0,
        item_marker: None,
        table: None,
        code: None,
        alert: None,
        image: None,
    };
    ui.spacing_mut().item_spacing.y = (size * 0.45).max(4.0);
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    ui.vertical(|ui| {
        for ev in Parser::new_ext(&text, opts) {
            r.event(ui, ev);
        }
        r.flush(ui);
    });
    r.out
}

struct Table {
    align: Vec<Alignment>,
    rows: Vec<Vec<Block>>,
    cell: Block,
    head_rows: usize,
}

struct Renderer<'a> {
    theme: &'a Theme,
    size: f32,
    out: Output,
    block: Block,
    inline: Inline,
    links: Vec<String>,
    lists: Vec<ListState>,
    quote: usize,
    /// Marker waiting for the first line of a list item.
    item_marker: Option<String>,
    table: Option<Table>,
    code: Option<(String, String)>,
    alert: Option<BlockQuoteKind>,
    /// Inside `![alt](url)`: its url and alt text so far.
    image: Option<(String, String)>,
}

impl Renderer<'_> {
    fn event(&mut self, ui: &mut egui::Ui, ev: Event<'_>) {
        if let Some((_, buf)) = &mut self.code {
            match ev {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((lang, code)) = self.code.take() {
                        self.code_block(ui, &lang, &code);
                    }
                }
                _ => {}
            }
            return;
        }
        match ev {
            Event::Start(tag) => self.start(ui, tag),
            Event::End(tag) => self.end(ui, tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let prev = self.inline;
                self.inline.code = true;
                self.text(&t);
                self.inline = prev;
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.text("\n"),
            Event::Rule => {
                self.flush(ui);
                ui.separator();
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
            Event::Html(h) | Event::InlineHtml(h) => {
                let t = strip_html(&h);
                if !t.trim().is_empty() {
                    self.text(&t);
                }
            }
            Event::FootnoteReference(f) => self.text(&format!("[{f}]")),
            _ => {}
        }
    }

    fn start(&mut self, ui: &mut egui::Ui, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush(ui);
                self.inline.heading = Some(level);
            }
            Tag::BlockQuote(kind) => {
                self.flush(ui);
                self.quote += 1;
                self.alert = kind.or(self.alert);
            }
            Tag::CodeBlock(kind) => {
                self.flush(ui);
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush(ui);
                self.lists.push(ListState { next: start });
            }
            Tag::Item => {
                self.flush(ui);
                let marker = match self.lists.last_mut() {
                    Some(ListState { next: Some(n) }) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => ["•", "◦", "▪"][self.lists.len().saturating_sub(1) % 3].to_owned(),
                };
                self.item_marker = Some(marker);
            }
            Tag::Emphasis => self.inline.emph += 1,
            Tag::Strong => self.inline.strong += 1,
            Tag::Strikethrough => self.inline.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Tag::Table(align) => {
                self.flush(ui);
                self.table = Some(Table {
                    align,
                    rows: Vec::new(),
                    cell: Block::default(),
                    head_rows: 0,
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                    t.head_rows = 1;
                }
                self.inline.strong += 1;
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {}
            _ => {}
        }
    }

    fn end(&mut self, ui: &mut egui::Ui, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(ui),
            TagEnd::Heading(_) => {
                self.flush(ui);
                self.inline.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush(ui);
                self.quote = self.quote.saturating_sub(1);
                if self.quote == 0 {
                    self.alert = None;
                }
            }
            TagEnd::List(_) => {
                self.flush(ui);
                self.lists.pop();
            }
            TagEnd::Item => self.flush(ui),
            TagEnd::Emphasis => self.inline.emph = self.inline.emph.saturating_sub(1),
            TagEnd::Strong => self.inline.strong = self.inline.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.inline.strike = self.inline.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                // Badges inside links vanish (the link text says it);
                // pass/fail badges become marks; others link to the image.
                if let Some((url, alt)) = self.image.take()
                    && self.links.is_empty()
                {
                    let a = alt.trim().to_lowercase();
                    if a.contains("passed") || a == "success" {
                        self.text("✅ ");
                    } else if a.contains("failed") || a.contains("error") {
                        self.text("❌ ");
                    } else if !a.is_empty() {
                        self.links.push(url);
                        self.text(&format!("🖼 {} ", alt.trim()));
                        self.links.pop();
                    }
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = &mut self.table {
                    let cell = std::mem::take(&mut t.cell);
                    if let Some(row) = t.rows.last_mut() {
                        row.push(cell);
                    }
                }
            }
            TagEnd::TableHead => self.inline.strong = self.inline.strong.saturating_sub(1),
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table_ui(ui, t);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, t: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
            return;
        }
        let run = Run {
            text: t.to_owned(),
            style: self.inline,
            link: self.links.last().cloned(),
        };
        match &mut self.table {
            Some(table) => table.cell.runs.push(run),
            None => self.block.runs.push(run),
        }
    }

    fn format(&self, s: Inline, link: bool) -> TextFormat {
        let t = self.theme;
        let (font, color) = match s.heading {
            Some(level) => {
                let scale = match level {
                    HeadingLevel::H1 => 1.55,
                    HeadingLevel::H2 => 1.32,
                    HeadingLevel::H3 => 1.15,
                    _ => 1.0,
                };
                let id = if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    fonts::display(self.size * scale)
                } else {
                    fonts::semibold(self.size * scale)
                };
                (id, style::c(t.text))
            }
            None if s.code => (
                FontId::new(self.size * 0.92, FontFamily::Monospace),
                style::c(t.text),
            ),
            None if s.strong > 0 => (fonts::semibold(self.size), style::c(t.text)),
            None => (FontId::proportional(self.size), style::c(t.text)),
        };
        let mut f = TextFormat::simple(font, if link { style::c(t.accent) } else { color });
        f.italics = s.emph > 0;
        if s.strike > 0 {
            f.strikethrough = Stroke::new(1.0, style::c(t.dim));
        }
        if link {
            f.underline = Stroke::new(1.0, tint(style::c(t.surface), style::c(t.accent), 0.5));
        }
        if s.code {
            f.background = tint(style::c(t.surface), style::c(t.text), 0.08);
        }
        f
    }

    fn job(&self, runs: &[Run], width: f32) -> LayoutJob {
        let mut job = LayoutJob {
            wrap: TextWrapping {
                max_width: width,
                ..TextWrapping::default()
            },
            ..LayoutJob::default()
        };
        for r in runs {
            job.append(&r.text, 0.0, self.format(r.style, r.link.is_some()));
        }
        job
    }

    /// Lay out the pending inline content as one block.
    fn flush(&mut self, ui: &mut egui::Ui) {
        let block = std::mem::take(&mut self.block);
        let marker = self.item_marker.take();
        if block.is_empty() && marker.is_none() {
            return;
        }
        let indent = 18.0 * self.lists.len().saturating_sub(1) as f32;
        let quote = self.quote;
        let alert = self.alert;
        let theme = self.theme;
        let draw = |r: &mut Self, ui: &mut egui::Ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                if let Some(m) = &marker {
                    // A fixed gutter so wrapped lines and nested lists line up;
                    // bullets are painted as dots centred on the first line.
                    let line_h = ui
                        .text_style_height(&egui::TextStyle::Body)
                        .max(r.size * 1.3);
                    let (cell, _) =
                        ui.allocate_exact_size(egui::vec2(MARKER_W, line_h), egui::Sense::hover());
                    let color = style::c(theme.dim);
                    match m.as_str() {
                        "•" => {
                            ui.painter().circle_filled(
                                egui::pos2(cell.min.x + 6.0, cell.center().y),
                                2.2,
                                color,
                            );
                        }
                        "◦" => {
                            ui.painter().circle_stroke(
                                egui::pos2(cell.min.x + 6.0, cell.center().y),
                                2.2,
                                egui::Stroke::new(1.0, color),
                            );
                        }
                        "▪" => {
                            ui.painter().rect_filled(
                                egui::Rect::from_center_size(
                                    egui::pos2(cell.min.x + 6.0, cell.center().y),
                                    egui::vec2(4.0, 4.0),
                                ),
                                0.0,
                                color,
                            );
                        }
                        _ => {
                            ui.painter().text(
                                egui::pos2(cell.max.x - 4.0, cell.center().y),
                                egui::Align2::RIGHT_CENTER,
                                m,
                                egui::FontId::proportional(r.size),
                                color,
                            );
                        }
                    };
                }
                ui.vertical(|ui| r.inline_block(ui, &block));
            });
        };
        if quote > 0 {
            let color = match alert {
                Some(BlockQuoteKind::Warning | BlockQuoteKind::Caution) => style::c(theme.blocked),
                Some(BlockQuoteKind::Important) => style::c(theme.danger),
                Some(BlockQuoteKind::Tip) => style::c(theme.done),
                Some(BlockQuoteKind::Note) => style::c(theme.accent),
                None => style::c(theme.border),
            };
            let resp = egui::Frame::new()
                .fill(if alert.is_some() {
                    tint(style::c(theme.surface), color, 0.08)
                } else {
                    Color32::TRANSPARENT
                })
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 6,
                    top: 2,
                    bottom: 2,
                })
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    draw(self, ui);
                })
                .response;
            let bar = egui::Rect::from_min_size(resp.rect.min, egui::vec2(3.0, resp.rect.height()));
            ui.painter().rect_filled(bar, CornerRadius::same(1), color);
        } else {
            draw(self, ui);
        }
    }

    fn inline_block(&mut self, ui: &mut egui::Ui, block: &Block) {
        let width = ui.available_width().max(40.0);
        if block.runs.iter().all(|r| r.link.is_none()) {
            let job = self.job(&block.runs, width);
            ui.add(egui::Label::new(job).wrap());
            return;
        }
        // Word by word, so each link is clickable.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for run in &block.runs {
                for word in run.text.split_inclusive(' ') {
                    let piece = Run {
                        text: word.to_owned(),
                        style: run.style,
                        link: run.link.clone(),
                    };
                    let job = self.job(std::slice::from_ref(&piece), f32::INFINITY);
                    match &run.link {
                        Some(url) => {
                            let r = ui
                                .add(egui::Label::new(job).sense(Sense::click()))
                                .on_hover_text(url);
                            if r.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if r.clicked() {
                                self.out.clicked_links.push(url.clone());
                            }
                        }
                        None => {
                            ui.label(job);
                        }
                    }
                }
            }
        });
    }

    fn code_block(&mut self, ui: &mut egui::Ui, lang: &str, code: &str) {
        let t = self.theme;
        egui::Frame::new()
            .fill(tint(style::c(t.surface), style::c(t.text), 0.05))
            .stroke(Stroke::new(1.0, style::c(t.border)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let suggestion = lang == "suggestion";
                if suggestion {
                    ui.label(
                        egui::RichText::new("Suggested change")
                            .font(fonts::semibold(self.size * 0.85))
                            .color(style::c(t.done)),
                    );
                } else if !lang.is_empty() {
                    ui.label(egui::RichText::new(lang).small().color(style::c(t.dim)));
                }
                egui::ScrollArea::horizontal()
                    .id_salt(("code", code.len(), lang))
                    .show(ui, |ui| {
                        let mut job = LayoutJob::default();
                        for line in code.trim_end_matches('\n').split_inclusive('\n') {
                            let color = match line.chars().next() {
                                Some('+') if lang == "diff" => style::c(t.done),
                                Some('-') if lang == "diff" => style::c(t.danger),
                                _ if suggestion => style::c(t.done),
                                _ => style::c(t.text),
                            };
                            job.append(
                                line,
                                0.0,
                                TextFormat::simple(FontId::monospace(self.size * 0.9), color),
                            );
                        }
                        ui.add(egui::Label::new(job).extend());
                    });
            });
    }

    fn table_ui(&mut self, ui: &mut egui::Ui, t: Table) {
        let theme = self.theme;
        egui::Frame::new()
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt(("table", t.rows.len()))
                    .show(ui, |ui| {
                        egui::Grid::new(ui.next_auto_id())
                            .striped(true)
                            .spacing([14.0, 6.0])
                            .show(ui, |ui| {
                                for (i, row) in t.rows.iter().enumerate() {
                                    for (c, cell) in row.iter().enumerate() {
                                        let job = self.job(&cell.runs, 320.0);
                                        let layout = match t.align.get(c) {
                                            Some(Alignment::Right) => {
                                                egui::Layout::right_to_left(egui::Align::Center)
                                            }
                                            _ => egui::Layout::left_to_right(egui::Align::Center),
                                        };
                                        ui.with_layout(layout, |ui| {
                                            let natural = ui.fonts_mut(|f| {
                                                f.layout_job(LayoutJob {
                                                    wrap: TextWrapping::no_max_width(),
                                                    ..job.clone()
                                                })
                                                .size()
                                                .x
                                            });
                                            let mut label = egui::Label::new(job).wrap();
                                            ui.set_min_width(natural.min(320.0));
                                            if i < t.head_rows {
                                                label = label.selectable(false);
                                            }
                                            ui.add(label);
                                        });
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    }
}

/// Text inside HTML, without tags or comments.
fn strip_html(h: &str) -> String {
    let mut out = String::new();
    let mut rest = h;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, r)| r);
        } else if rest.starts_with('<') {
            let (tag, r) = rest.split_once('>').unwrap_or((rest, ""));
            let name = tag
                .trim_start_matches(['<', '/'])
                .split([' ', '/'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(name.as_str(), "br" | "p" | "div" | "summary" | "li") {
                out.push(' ');
            }
            rest = r;
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push_str(rest.get(..end).unwrap_or_default());
            rest = rest.get(end..).unwrap_or_default();
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::strip_html;

    #[test]
    fn html_is_reduced_to_text() {
        assert_eq!(strip_html("<!-- hidden -->"), "");
        assert_eq!(strip_html("<details><summary>More</summary>"), " More ");
        assert_eq!(strip_html("a &amp; b<br/>c"), "a & b c");
    }
}
