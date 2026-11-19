//! Layout files: a window tree of rows, columns, tabbed stacks and blocks.
//!
//! ```json
//! { "root": { "row": [
//!     { "block": "spaces", "size": { "px": 260 }, "resizable": true },
//!     { "column": [ { "block": "agents" }, { "block": "mod:radar", "size": { "px": 180 } } ] }
//! ] } }
//! ```
//!
//! A block shows a built-in panel ([`BUILTIN_PANELS`]) or a mod (`mod:<name>`).

use serde::Deserialize;

pub const BUILTIN_PANELS: &[&str] = &[
    "agents", "spaces", "inbox", "prs", "detail", "status", "log",
];

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Size {
    /// Fixed logical pixels (resizable blocks start here).
    Px(f32),
    /// A share of what fixed siblings leave.
    Weight(f32),
}

/// The file as written.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    #[serde(default)]
    title: Option<String>,
    root: RawNode,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawNode {
    row: Option<Vec<RawNode>>,
    column: Option<Vec<RawNode>>,
    stack: Option<Vec<RawNode>>,
    block: Option<String>,
    size: Option<Size>,
    min: Option<f32>,
    max: Option<f32>,
    resizable: bool,
    hidden: bool,
    title: Option<String>,
    gap: Option<f32>,
    padding: Option<f32>,
    background: Option<String>,
    border: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    Builtin(String),
    Mod(String),
}

impl Panel {
    pub fn label(&self) -> &str {
        match self {
            Panel::Builtin(n) | Panel::Mod(n) => n,
        }
    }
}

/// How a node sits in its parent.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub size: Size,
    pub min: f32,
    pub max: f32,
    pub resizable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub title: Option<String>,
    pub padding: f32,
    pub background: Option<String>,
    pub border: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Split {
        axis: Axis,
        gap: f32,
        children: Vec<(Slot, Node)>,
    },
    Stack {
        style: Style,
        children: Vec<Node>,
    },
    Block {
        panel: Panel,
        style: Style,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub name: String,
    pub title: String,
    pub root: Node,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{path}: {msg}")]
    Shape { path: String, msg: String },
}

const DEFAULT_GAP: f32 = 6.0;
const DEFAULT_PADDING: f32 = 10.0;
const DEFAULT_MIN: f32 = 60.0;

pub fn parse(name: &str, text: &str) -> Result<Layout, LayoutError> {
    let raw: RawLayout = serde_json::from_str(text)?;
    Ok(Layout {
        name: name.to_owned(),
        title: raw.title.unwrap_or_else(|| name.to_owned()),
        root: node(&raw.root, "root")?,
    })
}

fn shape(path: &str, msg: impl Into<String>) -> LayoutError {
    LayoutError::Shape {
        path: path.to_owned(),
        msg: msg.into(),
    }
}

fn node(raw: &RawNode, path: &str) -> Result<Node, LayoutError> {
    let kinds = [
        raw.row.is_some(),
        raw.column.is_some(),
        raw.stack.is_some(),
        raw.block.is_some(),
    ];
    if kinds.iter().filter(|k| **k).count() != 1 {
        return Err(shape(
            path,
            "needs exactly one of row, column, stack, block",
        ));
    }
    let style = Style {
        title: raw.title.clone(),
        padding: raw.padding.unwrap_or(DEFAULT_PADDING),
        background: raw.background.clone(),
        border: raw.border.unwrap_or(true),
    };
    if let Some(block) = &raw.block {
        let panel = match block.strip_prefix("mod:") {
            Some(m) if !m.is_empty() => Panel::Mod(m.to_owned()),
            Some(_) => return Err(shape(path, "mod: needs a name")),
            None if BUILTIN_PANELS.contains(&block.as_str()) => Panel::Builtin(block.clone()),
            None => {
                return Err(shape(
                    path,
                    format!(
                        "unknown panel {block:?}; built-ins: {}, or mod:<name>",
                        BUILTIN_PANELS.join(", ")
                    ),
                ));
            }
        };
        return Ok(Node::Block { panel, style });
    }
    if let Some(items) = &raw.stack {
        let children = visible(items)
            .map(|(i, c)| node(c, &format!("{path}.stack[{i}]")))
            .collect::<Result<Vec<_>, _>>()?;
        if children.is_empty() {
            return Err(shape(path, "stack is empty"));
        }
        return Ok(Node::Stack { style, children });
    }
    let (axis, items, key) = match (&raw.row, &raw.column) {
        (Some(items), _) => (Axis::Row, items, "row"),
        (_, Some(items)) => (Axis::Column, items, "column"),
        _ => return Err(shape(path, "unreachable")),
    };
    let children = visible(items)
        .map(|(i, c)| {
            let p = format!("{path}.{key}[{i}]");
            let size = match c.size {
                Some(Size::Px(px)) if px.is_finite() && px > 0.0 => Size::Px(px),
                Some(Size::Weight(w)) if w.is_finite() && w > 0.0 => Size::Weight(w),
                Some(_) => return Err(shape(&p, "size must be positive")),
                None => Size::Weight(1.0),
            };
            let slot = Slot {
                size,
                min: c.min.unwrap_or(DEFAULT_MIN).max(0.0),
                max: c.max.unwrap_or(f32::INFINITY),
                resizable: c.resizable,
            };
            Ok((slot, node(c, &p)?))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if children.is_empty() {
        return Err(shape(path, format!("{key} is empty")));
    }
    Ok(Node::Split {
        axis,
        gap: raw.gap.unwrap_or(DEFAULT_GAP),
        children,
    })
}

fn visible(items: &[RawNode]) -> impl Iterator<Item = (usize, &RawNode)> {
    items.iter().enumerate().filter(|(_, c)| !c.hidden)
}

impl Node {
    /// Every mod this tree shows.
    pub fn mods(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |p| {
            if let Panel::Mod(m) = p {
                out.push(m.clone());
            }
        });
        out
    }

    fn walk(&self, f: &mut impl FnMut(&Panel)) {
        match self {
            Node::Split { children, .. } => children.iter().for_each(|(_, c)| c.walk(f)),
            Node::Stack { children, .. } => children.iter().for_each(|c| c.walk(f)),
            Node::Block { panel, .. } => f(panel),
        }
    }
}

/// Split `total` logical pixels among `slots`: fixed sizes first (clamped),
/// weights share the rest. `overrides` replaces a slot's px (drag-resized).
pub fn distribute(total: f32, gap: f32, slots: &[&Slot], overrides: &[Option<f32>]) -> Vec<f32> {
    let gaps = gap * slots.len().saturating_sub(1) as f32;
    let avail = (total - gaps).max(0.0);
    let fixed: Vec<Option<f32>> = slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let px = overrides.get(i).copied().flatten().or(match s.size {
                Size::Px(px) => Some(px),
                Size::Weight(_) => None,
            });
            px.map(|p| p.clamp(s.min, s.max.max(s.min)))
        })
        .collect();
    let used: f32 = fixed.iter().flatten().sum();
    let weights: f32 = slots
        .iter()
        .zip(&fixed)
        .filter(|(_, f)| f.is_none())
        .map(|(s, _)| match s.size {
            Size::Weight(w) => w,
            Size::Px(_) => 0.0,
        })
        .sum();
    let rest = (avail - used).max(0.0);
    slots
        .iter()
        .zip(&fixed)
        .map(|(s, f)| match (f, s.size) {
            (Some(px), _) => *px,
            (None, Size::Weight(w)) if weights > 0.0 => rest * w / weights,
            _ => 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_tree() {
        let l = parse(
            "main",
            r#"{ "root": { "row": [
                { "block": "spaces", "size": { "px": 260 }, "resizable": true },
                { "block": "log", "hidden": true },
                { "stack": [ { "block": "agents" }, { "block": "mod:radar" } ] }
            ] } }"#,
        )
        .unwrap();
        let Node::Split { axis, children, .. } = &l.root else {
            panic!("not a split")
        };
        assert_eq!(*axis, Axis::Row);
        assert_eq!(children.len(), 2, "hidden children drop out");
        assert!(children[0].0.resizable);
        assert_eq!(l.root.mods(), ["radar"]);
    }

    #[test]
    fn errors_name_the_node() {
        let err = parse("x", r#"{ "root": { "row": [ { "block": "nope" } ] } }"#).unwrap_err();
        assert!(
            err.to_string().starts_with("root.row[0]: unknown panel"),
            "{err}"
        );
        assert!(parse("x", r#"{ "root": { "block": "agents", "row": [] } }"#).is_err());
        assert!(
            parse("x", r#"{ "root": { "blok": "agents" } }"#).is_err(),
            "unknown fields are refused"
        );
    }

    #[test]
    fn distribute_fixed_then_weights() {
        let px = Slot {
            size: Size::Px(100.0),
            min: 50.0,
            max: 400.0,
            resizable: true,
        };
        let w1 = Slot {
            size: Size::Weight(1.0),
            min: 0.0,
            max: f32::INFINITY,
            resizable: false,
        };
        let w3 = Slot {
            size: Size::Weight(3.0),
            ..w1.clone()
        };
        assert_eq!(
            distribute(510.0, 5.0, &[&px, &w1, &w3], &[None, None, None]),
            [100.0, 100.0, 300.0]
        );
        assert_eq!(
            distribute(510.0, 5.0, &[&px, &w1, &w3], &[Some(10.0), None, None]),
            [50.0, 112.5, 337.5]
        );
    }
}
