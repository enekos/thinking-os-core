//! Mods: Rhai scripts in `mods/<name>.rhai` that define `fn render(ctx)` and
//! return a UI tree built from the prelude (`text`, `row`, `card`, `button`,
//! …). The host compiles each mod once per edit, calls `render` only when
//! its inputs change, and turns the returned maps into [`UiNode`]s that the
//! app draws. A mod never touches the window or the disk: it describes, the
//! app draws, and every click comes back as an [`Action`].
//!
//! `ctx` holds `sessions`, `spaces`, `prs`, `inbox`, `counts`, `now`,
//! `selected`, `filter`, `theme`, `width` and the mod's own `state`, which
//! `set(key, value)` actions update.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use artalde_core::command::Command;
use artalde_core::config::{ModSource, write_error};
use artalde_core::model::{AgentState, Snapshot};
use artalde_core::text::{ago, clip};
use rhai::{AST, Dynamic, Engine, EvalAltResult, Map, Scope};

const PRELUDE: &str = include_str!("prelude.rhai");

/// Script budget per render; a runaway loop errors instead of freezing.
const MAX_OPERATIONS: u64 = 2_000_000;
const MAX_CALL_LEVELS: usize = 48;
const MAX_STRING: usize = 64 * 1024;
const MAX_COLLECTION: usize = 20_000;
/// Widget tree bounds.
const MAX_NODES: usize = 4_000;
const MAX_DEPTH: usize = 24;
const MAX_GRID_COLS: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    Heading,
    /// Markdown text.
    Markdown,
    Row,
    Col,
    Wrap,
    Grid,
    Card,
    Button,
    Badge,
    Dot,
    /// A Lucide icon named by `text`.
    Icon,
    Sep,
    Space,
    Bar,
}

/// What a click asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Run an app command.
    Command(Command),
    /// Merge into the mod's state and re-render.
    Set(Patch),
}

/// An opaque state update; hand it back to [`ModHost::apply`].
#[derive(Clone, Debug)]
pub struct Patch(Map);

impl PartialEq for Patch {
    fn eq(&self, other: &Patch) -> bool {
        format!("{:?}", self.0) == format!("{:?}", other.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiNode {
    pub kind: Kind,
    pub text: String,
    pub children: Vec<UiNode>,
    pub cols: usize,
    /// `space` size or `bar` fraction.
    pub value: f32,
    /// Theme token or `#rrggbb`.
    pub color: Option<String>,
    pub fill: Option<String>,
    pub size: Option<f32>,
    pub bold: bool,
    pub mono: bool,
    /// A button's leading icon, by Lucide name.
    pub icon: Option<String>,
    pub tip: Option<String>,
    pub action: Option<Action>,
}

/// One mod's render result.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub node: Result<UiNode, String>,
    pub micros: u128,
}

#[derive(Debug)]
struct Slot {
    source_hash: u64,
    ast: Result<AST, String>,
    state: Map,
    state_version: u64,
    cache: Option<((u64, u64), Rendered)>,
    path: std::path::PathBuf,
    last_error: Option<String>,
}

pub struct ModHost {
    engine: Engine,
    prelude: AST,
    slots: HashMap<String, Slot>,
}

impl std::fmt::Debug for ModHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModHost")
            .field("mods", &self.slots.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The part of `ctx` shared by every mod; build once per change with
/// [`context`].
#[derive(Clone, Debug)]
pub struct Context {
    /// Changes whenever any input changes.
    pub version: u64,
    map: Map,
}

/// What the app adds to the snapshot.
#[derive(Clone, Copy, Debug)]
pub struct ViewState<'a> {
    pub selected: Option<&'a str>,
    pub filter: Option<&'a str>,
    pub theme: &'a str,
}

/// Build the shared `ctx` from a snapshot.
pub fn context(snap: &Snapshot, view: ViewState<'_>, version: u64) -> Context {
    let mut map = Map::new();
    let sessions: rhai::Array = snap
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut d = rhai::serde::to_dynamic(s).unwrap_or(Dynamic::UNIT);
            if let Some(m) = d.write_lock::<Map>().as_deref_mut() {
                m.insert("name".into(), s.name().into());
                m.insert("index".into(), Dynamic::from_int(int(i + 1)));
                m.insert(
                    "age".into(),
                    Dynamic::from_int(int(snap.now.saturating_sub(s.rec.since))),
                );
                m.insert(
                    "selected".into(),
                    (view.selected == Some(s.rec.id.as_str())).into(),
                );
            }
            d
        })
        .collect();
    map.insert("sessions".into(), sessions.into());
    map.insert(
        "spaces".into(),
        rhai::serde::to_dynamic(&snap.spaces).unwrap_or(Dynamic::UNIT),
    );
    map.insert(
        "prs".into(),
        rhai::serde::to_dynamic(&snap.prs).unwrap_or(Dynamic::UNIT),
    );
    map.insert(
        "inbox".into(),
        rhai::serde::to_dynamic(&snap.inbox).unwrap_or(Dynamic::UNIT),
    );
    let mut counts = Map::new();
    for st in AgentState::ALL {
        counts.insert(st.as_str().into(), Dynamic::from_int(int(snap.count(st))));
    }
    map.insert("counts".into(), counts.into());
    map.insert("now".into(), Dynamic::from_int(int(snap.now)));
    map.insert(
        "selected".into(),
        view.selected.map_or(Dynamic::UNIT, |s| s.into()),
    );
    map.insert(
        "filter".into(),
        view.filter.map_or(Dynamic::UNIT, |s| s.into()),
    );
    map.insert("theme".into(), view.theme.into());
    Context { version, map }
}

fn int(n: impl TryInto<rhai::INT>) -> rhai::INT {
    n.try_into().unwrap_or(rhai::INT::MAX)
}

impl Default for ModHost {
    fn default() -> ModHost {
        ModHost::new()
    }
}

impl ModHost {
    pub fn new() -> ModHost {
        let mut engine = Engine::new();
        engine.set_max_operations(MAX_OPERATIONS);
        engine.set_max_call_levels(MAX_CALL_LEVELS);
        engine.set_max_string_size(MAX_STRING);
        engine.set_max_array_size(MAX_COLLECTION);
        engine.set_max_map_size(MAX_COLLECTION);
        engine.set_max_expr_depths(64, 32);
        engine.on_print(|s| eprintln!("artalde mod: {s}"));
        engine.register_fn("ago", |secs: rhai::INT| {
            ago(u64::try_from(secs).unwrap_or(0))
        });
        engine.register_fn("clip", |s: &str, n: rhai::INT| {
            clip(s, usize::try_from(n).unwrap_or(0))
        });
        let prelude = match engine.compile(PRELUDE) {
            Ok(ast) => ast,
            Err(e) => {
                eprintln!("artalde: prelude does not compile: {e}");
                AST::empty()
            }
        };
        ModHost {
            engine,
            prelude,
            slots: HashMap::new(),
        }
    }

    /// Track `sources`: compile new or edited mods, forget removed ones.
    /// Mod state survives edits.
    pub fn sync(&mut self, sources: &[ModSource]) {
        self.slots
            .retain(|name, _| sources.iter().any(|s| &s.name == name));
        for src in sources {
            let hash = hash(&src.source);
            if self
                .slots
                .get(&src.name)
                .is_some_and(|s| s.source_hash == hash)
            {
                continue;
            }
            let ast = self
                .engine
                .compile(&src.source)
                .map(|user| self.prelude.merge(&user))
                .map_err(|e| format!("compile: {e}"));
            let state = self
                .slots
                .remove(&src.name)
                .map(|s| s.state)
                .unwrap_or_default();
            self.slots.insert(
                src.name.clone(),
                Slot {
                    source_hash: hash,
                    ast,
                    state,
                    state_version: 0,
                    cache: None,
                    path: src.path.clone(),
                    last_error: None,
                },
            );
        }
    }

    pub fn names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.slots.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// Render `name` against `ctx`, reusing the last result when neither
    /// `ctx` nor the mod's state changed.
    pub fn render(&mut self, name: &str, ctx: &Context) -> Rendered {
        let Some(slot) = self.slots.get_mut(name) else {
            return Rendered {
                node: Err(format!("no mod named {name:?} in mods/")),
                micros: 0,
            };
        };
        let key = (ctx.version, slot.state_version);
        if let Some((k, r)) = &slot.cache
            && *k == key
        {
            return r.clone();
        }
        let started = std::time::Instant::now();
        let node = match &slot.ast {
            Err(e) => Err(e.clone()),
            Ok(ast) => {
                let mut map = ctx.map.clone();
                map.insert("state".into(), slot.state.clone().into());
                let out: Result<Dynamic, Box<EvalAltResult>> = self.engine.call_fn(
                    &mut Scope::new(),
                    ast,
                    "render",
                    (Dynamic::from_map(map),),
                );
                out.map_err(|e| format!("render: {e}")).and_then(|d| {
                    let mut budget = MAX_NODES;
                    to_node(&d, 0, &mut budget)
                })
            }
        };
        let err = node.as_ref().err().cloned();
        if err != slot.last_error {
            write_error(&slot.path, err.as_deref());
            slot.last_error = err;
        }
        let rendered = Rendered {
            node,
            micros: started.elapsed().as_micros(),
        };
        slot.cache = Some((key, rendered.clone()));
        rendered
    }

    /// Apply a `set(...)` action to the mod's state.
    pub fn apply(&mut self, name: &str, patch: &Patch) {
        if let Some(slot) = self.slots.get_mut(name) {
            for (k, v) in &patch.0 {
                slot.state.insert(k.clone(), v.clone());
            }
            slot.state_version = slot.state_version.wrapping_add(1);
        }
    }
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn to_node(d: &Dynamic, depth: usize, budget: &mut usize) -> Result<UiNode, String> {
    if depth > MAX_DEPTH {
        return Err(format!("tree deeper than {MAX_DEPTH}"));
    }
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| format!("more than {MAX_NODES} nodes"))?;
    if d.is_unit() {
        return Ok(UiNode {
            kind: Kind::Space,
            ..Default::default()
        });
    }
    if d.is_string() || d.is_int() || d.is_float() || d.is_bool() {
        return Ok(UiNode {
            kind: Kind::Text,
            text: d.to_string(),
            ..Default::default()
        });
    }
    if let Some(arr) = d.read_lock::<rhai::Array>() {
        return Ok(UiNode {
            kind: Kind::Col,
            children: children(&arr, depth, budget)?,
            ..Default::default()
        });
    }
    let Some(map) = d.read_lock::<Map>() else {
        return Err(format!("expected a node, got {}", d.type_name()));
    };
    let s = |k: &str| map.get(k).filter(|v| !v.is_unit()).map(|v| v.to_string());
    let kind = match s("t").as_deref() {
        Some("text") => Kind::Text,
        Some("heading") => Kind::Heading,
        Some("md") => Kind::Markdown,
        Some("row") => Kind::Row,
        Some("col") => Kind::Col,
        Some("wrap") => Kind::Wrap,
        Some("grid") => Kind::Grid,
        Some("card") => Kind::Card,
        Some("button") => Kind::Button,
        Some("badge") => Kind::Badge,
        Some("dot") => Kind::Dot,
        Some("icon") => Kind::Icon,
        Some("sep") => Kind::Sep,
        Some("space") => Kind::Space,
        Some("bar") => Kind::Bar,
        other => return Err(format!("unknown node type {other:?}")),
    };
    let kids = match map
        .get("children")
        .and_then(|c| c.read_lock::<rhai::Array>().map(|a| a.clone()))
    {
        Some(arr) => children(&arr, depth, budget)?,
        None => Vec::new(),
    };
    let num = |k: &str| {
        map.get(k).and_then(|v| {
            v.as_float()
                .ok()
                .or_else(|| v.as_int().ok().map(|i| i as f64))
        })
    };
    #[expect(clippy::cast_possible_truncation, reason = "UI sizes")]
    let value = num("value").unwrap_or(0.0) as f32;
    #[expect(clippy::cast_possible_truncation, reason = "UI sizes")]
    let size = num("size").map(|v| (v as f32).clamp(6.0, 64.0));
    let cols = map
        .get("cols")
        .and_then(|v| v.as_int().ok())
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(1)
        .clamp(1, MAX_GRID_COLS);
    let action = map
        .get("action")
        .filter(|a| !a.is_unit())
        .map(to_action)
        .transpose()?;
    Ok(UiNode {
        kind,
        text: s("text").unwrap_or_default(),
        children: kids,
        cols,
        value,
        color: s("color"),
        fill: s("fill"),
        size,
        bold: map
            .get("bold")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false),
        mono: map
            .get("mono")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false),
        icon: s("icon"),
        tip: s("tip"),
        action,
    })
}

fn children(arr: &[Dynamic], depth: usize, budget: &mut usize) -> Result<Vec<UiNode>, String> {
    arr.iter().map(|c| to_node(c, depth + 1, budget)).collect()
}

fn to_action(d: &Dynamic) -> Result<Action, String> {
    if d.is_string() {
        return Ok(Action::Command(Command::parse(&d.to_string())));
    }
    let map = d
        .read_lock::<Map>()
        .ok_or_else(|| format!("action must be cmd(..) or set(..), got {}", d.type_name()))?;
    if let Some(name) = map.get("cmd") {
        let arg = map
            .get("arg")
            .filter(|a| !a.is_unit())
            .map(ToString::to_string);
        return Ok(Action::Command(Command {
            name: name.to_string(),
            arg,
        }));
    }
    if let Some(patch) = map
        .get("set")
        .and_then(|p| p.read_lock::<Map>().map(|m| m.clone()))
    {
        return Ok(Action::Set(Patch(patch)));
    }
    Err("action map needs `cmd` or `set`".into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use artalde_core::model::{Session, SessionRecord};

    use super::*;

    fn src(name: &str, source: &str) -> ModSource {
        ModSource {
            name: name.into(),
            path: std::env::temp_dir().join(format!(
                "artalde-mod-test-{}-{name}.rhai",
                std::process::id()
            )),
            source: source.into(),
        }
    }

    fn snap() -> Snapshot {
        let rec = |id: &str, state| SessionRecord {
            id: id.into(),
            agent: "claude".into(),
            state,
            since: 90,
            ..Default::default()
        };
        Snapshot {
            version: 1,
            now: 100,
            sessions: vec![
                Session {
                    rec: rec("a", AgentState::Blocked),
                    repo: Some("data".into()),
                    ..Default::default()
                },
                Session {
                    rec: rec("b", AgentState::Working),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn ctx(version: u64) -> Context {
        context(
            &snap(),
            ViewState {
                selected: Some("a"),
                filter: None,
                theme: "graphite",
            },
            version,
        )
    }

    #[test]
    fn renders_a_tree_with_actions() {
        let mut host = ModHost::new();
        host.sync(&[src(
            "t",
            r#"fn render(ctx) {
                let b = ctx.sessions.filter(|s| s.state == "blocked");
                col([
                    heading(`${ctx.counts.blocked} blocked`),
                    card([text(b[0].name).bold(), text(ago(b[0].age))]).fill("blocked").on_click(cmd("agent.select", b[0].id)),
                    button("compact", set("compact", true)),
                ])
            }"#,
        )]);
        let node = host.render("t", &ctx(1)).node.unwrap();
        assert_eq!(node.kind, Kind::Col);
        assert_eq!(node.children[0].text, "1 blocked");
        let card = &node.children[1];
        assert_eq!(card.fill.as_deref(), Some("blocked"));
        assert_eq!(card.children[0].text, "claude · data");
        assert!(card.children[0].bold);
        assert_eq!(card.children[1].text, "10s");
        assert_eq!(
            card.action,
            Some(Action::Command(Command::new("agent.select", Some("a"))))
        );
        assert!(matches!(node.children[2].action, Some(Action::Set(_))));
    }

    #[test]
    fn set_updates_state_and_rerenders() {
        let mut host = ModHost::new();
        host.sync(&[src("s", r#"fn render(ctx) { button(if ctx.state.on ?? false { "on" } else { "off" }, set("on", true)) }"#)]);
        let first = host.render("s", &ctx(1)).node.unwrap();
        assert_eq!(first.text, "off");
        let Some(Action::Set(patch)) = first.action else {
            panic!("no set action")
        };
        host.apply("s", &patch);
        assert_eq!(host.render("s", &ctx(1)).node.unwrap().text, "on");
        host.sync(&[src("s", r#"fn render(ctx) { text(ctx.state.on) }"#)]);
        assert_eq!(
            host.render("s", &ctx(1)).node.unwrap().text,
            "true",
            "state survives an edit"
        );
    }

    #[test]
    fn errors_are_contained() {
        let mut host = ModHost::new();
        host.sync(&[
            src("loop", "fn render(ctx) { loop {} }"),
            src("syntax", "fn render(ctx) { col([ }"),
            src("shape", r#"fn render(ctx) { #{ t: "blink" } }"#),
        ]);
        let mut err = |n| host.render(n, &ctx(1)).node.unwrap_err();
        assert!(
            err("loop").contains("render"),
            "runaway loops hit the operation budget"
        );
        assert!(err("syntax").starts_with("compile"));
        assert!(err("shape").contains("blink"));
        let path = PathBuf::from(format!("{}.error", src("shape", "").path.display()));
        assert!(path.exists(), "errors are written next to the mod");
        let _cleanup = std::fs::remove_file(path);
    }

    #[test]
    fn huge_trees_are_refused() {
        let mut host = ModHost::new();
        host.sync(&[src(
            "big",
            "fn render(ctx) { let a = []; for i in 0..5000 { a.push(text(i)); } col(a) }",
        )]);
        assert!(
            host.render("big", &ctx(1))
                .node
                .unwrap_err()
                .contains("nodes")
        );
    }
}
