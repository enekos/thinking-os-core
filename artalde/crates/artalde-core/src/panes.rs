//! Terminal panes: a binary tree of splits whose leaves are terminal ids.
//!
//! Pure geometry and tree edits; the app owns the terminals themselves.

use serde::{Deserialize, Serialize};

/// The smallest share of a split either side may take.
const MIN_RATIO: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// Side by side.
    Right,
    /// One above the other.
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

impl Toward {
    pub fn parse(s: &str) -> Option<Toward> {
        match s {
            "left" => Some(Toward::Left),
            "right" => Some(Toward::Right),
            "up" => Some(Toward::Up),
            "down" => Some(Toward::Down),
            _ => None,
        }
    }
}

/// A rectangle in whatever unit the caller draws in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    fn center(self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pane {
    Leaf(u32),
    Split {
        dir: Dir,
        /// The first child's share, 0..1.
        ratio: f32,
        a: Box<Pane>,
        b: Box<Pane>,
    },
}

/// A divider between the two children of the split at `path`.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    /// `false` = first child, `true` = second, from the root down.
    pub path: Vec<bool>,
    pub dir: Dir,
    /// The gap between the children.
    pub area: Area,
    /// The split's whole extent along `dir`, for turning a drag into a ratio.
    pub extent: f32,
}

impl Pane {
    pub fn leaves(&self) -> Vec<u32> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<u32>) {
        match self {
            Pane::Leaf(id) => out.push(*id),
            Pane::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn contains(&self, id: u32) -> bool {
        match self {
            Pane::Leaf(l) => *l == id,
            Pane::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    /// Put `new` beside `target`, sharing its space evenly. Returns whether
    /// `target` was found.
    pub fn split(&mut self, target: u32, new: u32, dir: Dir) -> bool {
        match self {
            Pane::Leaf(id) if *id == target => {
                *self = Pane::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(Pane::Leaf(target)),
                    b: Box::new(Pane::Leaf(new)),
                };
                true
            }
            Pane::Leaf(_) => false,
            Pane::Split { a, b, .. } => a.split(target, new, dir) || b.split(target, new, dir),
        }
    }

    /// The tree without `id`; its sibling takes the space. `None` when the
    /// tree was only that leaf.
    #[must_use]
    pub fn without(self, id: u32) -> Option<Pane> {
        match self {
            Pane::Leaf(l) if l == id => None,
            leaf @ Pane::Leaf(_) => Some(leaf),
            Pane::Split { dir, ratio, a, b } => match (a.without(id), b.without(id)) {
                (Some(a), Some(b)) => Some(Pane::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    /// Keep only leaves `keep` accepts.
    #[must_use]
    pub fn retain(self, keep: &impl Fn(u32) -> bool) -> Option<Pane> {
        let mut tree = Some(self);
        loop {
            let gone = tree
                .as_ref()
                .and_then(|t| t.leaves().into_iter().find(|id| !keep(*id)));
            match (gone, tree.take()) {
                (Some(id), Some(t)) => tree = t.without(id),
                (_, t) => return t,
            }
        }
    }

    /// Where every leaf goes inside `area`, with `gap` between siblings.
    pub fn place(&self, area: Area, gap: f32) -> Vec<(u32, Area)> {
        let mut leaves = Vec::new();
        let mut dividers = Vec::new();
        self.walk(area, gap, &mut Vec::new(), &mut leaves, &mut dividers);
        leaves
    }

    pub fn dividers(&self, area: Area, gap: f32) -> Vec<Divider> {
        let mut leaves = Vec::new();
        let mut dividers = Vec::new();
        self.walk(area, gap, &mut Vec::new(), &mut leaves, &mut dividers);
        dividers
    }

    fn walk(
        &self,
        area: Area,
        gap: f32,
        path: &mut Vec<bool>,
        leaves: &mut Vec<(u32, Area)>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Pane::Leaf(id) => leaves.push((*id, area)),
            Pane::Split { dir, ratio, a, b } => {
                let ratio = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                let (first, mid, second, extent) = match dir {
                    Dir::Right => {
                        let extent = (area.w - gap).max(0.0);
                        let w = (extent * ratio).round();
                        (
                            Area { w, ..area },
                            Area {
                                x: area.x + w,
                                w: gap,
                                ..area
                            },
                            Area {
                                x: area.x + w + gap,
                                w: extent - w,
                                ..area
                            },
                            extent,
                        )
                    }
                    Dir::Down => {
                        let extent = (area.h - gap).max(0.0);
                        let h = (extent * ratio).round();
                        (
                            Area { h, ..area },
                            Area {
                                y: area.y + h,
                                h: gap,
                                ..area
                            },
                            Area {
                                y: area.y + h + gap,
                                h: extent - h,
                                ..area
                            },
                            extent,
                        )
                    }
                };
                dividers.push(Divider {
                    path: path.clone(),
                    dir: *dir,
                    area: mid,
                    extent,
                });
                path.push(false);
                a.walk(first, gap, path, leaves, dividers);
                path.pop();
                path.push(true);
                b.walk(second, gap, path, leaves, dividers);
                path.pop();
            }
        }
    }

    /// Move the divider of the split at `path` by `delta` (a share of the
    /// split's extent). `reset` puts it back in the middle.
    pub fn nudge(&mut self, path: &[bool], delta: f32, reset: bool) {
        match (self, path.split_first()) {
            (Pane::Split { ratio, .. }, None) => {
                *ratio = if reset {
                    0.5
                } else {
                    (*ratio + delta).clamp(MIN_RATIO, 1.0 - MIN_RATIO)
                };
            }
            (Pane::Split { a, b, .. }, Some((second, rest))) => {
                if *second {
                    b.nudge(rest, delta, reset);
                } else {
                    a.nudge(rest, delta, reset);
                }
            }
            (Pane::Leaf(_), _) => {}
        }
    }

    /// The pane `toward` from `from`: the nearest one whose centre lies in
    /// that direction and which overlaps `from` on the other axis.
    pub fn neighbor(&self, from: u32, toward: Toward) -> Option<u32> {
        // Large enough that rounding to whole units does not matter.
        let canvas = Area {
            x: 0.0,
            y: 0.0,
            w: 4096.0,
            h: 4096.0,
        };
        let placed = self.place(canvas, 0.0);
        let here = placed.iter().find(|(id, _)| *id == from)?.1;
        let (hx, hy) = here.center();
        placed
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, a)| {
                let (cx, cy) = a.center();
                let overlap_y = a.y < here.y + here.h && here.y < a.y + a.h;
                let overlap_x = a.x < here.x + here.w && here.x < a.x + a.w;
                let dist = match toward {
                    Toward::Left if cx < hx && overlap_y => hx - cx,
                    Toward::Right if cx > hx && overlap_y => cx - hx,
                    Toward::Up if cy < hy && overlap_x => hy - cy,
                    Toward::Down if cy > hy && overlap_x => cy - hy,
                    _ => return None,
                };
                // Among equally near panes, prefer the one level with us.
                let off = match toward {
                    Toward::Left | Toward::Right => (cy - hy).abs(),
                    Toward::Up | Toward::Down => (cx - hx).abs(),
                };
                Some((*id, dist, off))
            })
            .min_by(|a, b| {
                (a.1, a.2)
                    .partial_cmp(&(b.1, b.2))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(id, _, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Area = Area {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 60.0,
    };

    fn grid() -> Pane {
        // 1 | 2
        //   | 3
        let mut p = Pane::Leaf(1);
        assert!(p.split(1, 2, Dir::Right));
        assert!(p.split(2, 3, Dir::Down));
        p
    }

    #[test]
    fn split_places_panes_with_a_gap() {
        let placed = grid().place(FULL, 4.0);
        assert_eq!(placed.len(), 3);
        assert_eq!(
            placed[0],
            (
                1,
                Area {
                    x: 0.0,
                    y: 0.0,
                    w: 48.0,
                    h: 60.0
                }
            )
        );
        assert_eq!(placed[1].1.x, 52.0);
        assert_eq!(placed[1].1.h, 28.0);
        assert_eq!(placed[2].1.y, 32.0);
        assert!(!grid().clone().split(9, 4, Dir::Right));
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_sibling() {
        let p = grid().without(2).expect("two left");
        assert_eq!(p.leaves(), vec![1, 3]);
        let p = p.without(1).expect("one left");
        assert_eq!(p, Pane::Leaf(3));
        assert_eq!(p.without(3), None);
    }

    #[test]
    fn retain_drops_dead_terminals() {
        assert_eq!(grid().retain(&|id| id == 3), Some(Pane::Leaf(3)));
        assert_eq!(grid().retain(&|_| false), None);
        assert_eq!(grid().retain(&|_| true), Some(grid()));
    }

    #[test]
    fn focus_moves_by_direction() {
        let p = grid();
        assert_eq!(p.neighbor(1, Toward::Right), Some(2));
        assert_eq!(p.neighbor(2, Toward::Down), Some(3));
        assert_eq!(p.neighbor(3, Toward::Up), Some(2));
        assert_eq!(p.neighbor(3, Toward::Left), Some(1));
        assert_eq!(p.neighbor(1, Toward::Left), None);
        assert_eq!(p.neighbor(7, Toward::Left), None);
    }

    #[test]
    fn dividers_resize_within_bounds() {
        let mut p = grid();
        let d = p.dividers(FULL, 4.0);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].path, Vec::<bool>::new());
        assert_eq!(d[1].path, vec![true]);
        assert_eq!(d[1].dir, Dir::Down);
        p.nudge(&[], 0.9, false);
        assert_eq!(p.place(FULL, 0.0)[0].1.w, 90.0);
        p.nudge(&[], 0.0, true);
        assert_eq!(p.place(FULL, 0.0)[0].1.w, 50.0);
    }

    #[test]
    fn round_trips_as_json() {
        let json = serde_json::to_string(&grid()).expect("serialises");
        assert_eq!(serde_json::from_str::<Pane>(&json).expect("parses"), grid());
    }
}
