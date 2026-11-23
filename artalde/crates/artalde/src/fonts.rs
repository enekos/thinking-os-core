//! Typefaces: azala's (Instrument Sans, Bricolage Grotesque, JetBrains Mono —
//! SIL OFL 1.1, see `fonts/CREDITS.md`), plus the system's symbol fonts as
//! fallbacks so `⌘ ⏎ ● ⇄` and friends render instead of boxes.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const BODY: &[u8] = include_bytes!("../fonts/InstrumentSans-Regular.ttf");
const BODY_SEMIBOLD: &[u8] = include_bytes!("../fonts/InstrumentSans-SemiBold.ttf");
const DISPLAY: &[u8] = include_bytes!("../fonts/Bricolage-Bold.ttf");
const MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");
/// Lucide (ISC), subset by `tools/icons.py` to the glyphs in `icons.rs`.
const ICONS: &[u8] = include_bytes!("../fonts/lucide.ttf");

/// Read at runtime, never shipped: macOS's own symbol coverage.
const SYSTEM_FALLBACKS: [&str; 2] = [
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];

/// Terminal programs draw with glyphs JetBrains Mono lacks (prompt arrows,
/// box drawing, powerline). Monospace only, tried in order after it.
const TERMINAL_FALLBACKS: [&str; 3] = [
    "/System/Library/Fonts/Menlo.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

/// Where a user keeps their own fonts, relative to the home directory.
const USER_FONT_DIRS: [&str; 3] = ["Library/Fonts", ".local/share/fonts", ".fonts"];

/// Larger than any symbol font we would load.
const MAX_FALLBACK_BYTES: u64 = 40 * 1024 * 1024;

pub const SEMIBOLD: &str = "semibold";
pub const DISPLAY_FAMILY: &str = "display";
pub const ICON_FAMILY: &str = "icons";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY_FAMILY.into()))
}

pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICON_FAMILY.into()))
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let builtin: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("instrument", BODY);
    add("instrument-semibold", BODY_SEMIBOLD);
    add("bricolage", DISPLAY);
    add("jetbrains", MONO);
    add("lucide", ICONS);

    let mut fallbacks = Vec::new();
    for (i, path) in SYSTEM_FALLBACKS.iter().enumerate() {
        let ok = std::fs::metadata(path).is_ok_and(|m| m.len() <= MAX_FALLBACK_BYTES);
        if let Some(bytes) = ok.then(|| std::fs::read(path).ok()).flatten() {
            let name = format!("system-fallback-{i}");
            defs.font_data
                .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            fallbacks.push(name);
        }
    }
    // egui's own fonts last: they carry its icon glyphs.
    fallbacks.extend(builtin);

    let mut mono = vec!["jetbrains".to_owned()];
    let terminal = TERMINAL_FALLBACKS
        .iter()
        .map(std::path::PathBuf::from)
        .chain(nerd_font());
    for (i, path) in terminal.enumerate() {
        let ok = std::fs::metadata(&path).is_ok_and(|m| m.len() <= MAX_FALLBACK_BYTES);
        if let Some(bytes) = ok.then(|| std::fs::read(&path).ok()).flatten() {
            let name = format!("terminal-fallback-{i}");
            defs.font_data
                .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            mono.push(name);
        }
    }
    mono.extend(fallbacks.iter().cloned());

    let family = |primary: &str| -> Vec<String> {
        std::iter::once(primary.to_owned())
            .chain(fallbacks.iter().cloned())
            .collect()
    };
    defs.families
        .insert(FontFamily::Proportional, family("instrument"));
    defs.families.insert(FontFamily::Monospace, mono);
    defs.families.insert(
        FontFamily::Name(SEMIBOLD.into()),
        family("instrument-semibold"),
    );
    defs.families
        .insert(FontFamily::Name(DISPLAY_FAMILY.into()), family("bricolage"));
    defs.families
        .insert(FontFamily::Name(ICON_FAMILY.into()), vec!["lucide".into()]);
    ctx.set_fonts(defs);
}

/// One of the user's Nerd Fonts, for the private-use icons prompts and TUIs
/// draw. The single-width `Mono` cut, regular weight; the first by name.
fn nerd_font() -> Option<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    let mut found: Vec<std::path::PathBuf> = USER_FONT_DIRS
        .iter()
        .filter_map(|dir| std::fs::read_dir(home.join(dir)).ok())
        .flat_map(|entries| entries.filter_map(Result::ok).map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("NerdFontMono-Regular."))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}
