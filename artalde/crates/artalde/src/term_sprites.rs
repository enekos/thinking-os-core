//! Box-drawing and block characters drawn as shapes that fill their cell, so
//! borders join across cells whatever font the text falls back to.

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, epaint::PathShape, pos2};

const NONE: u8 = 0;
const LIGHT: u8 = 1;
const HEAVY: u8 = 2;
const DOUBLE: u8 = 3;

/// The arms of U+2500..=U+257F as `[left, right, up, down]` weights.
#[rustfmt::skip]
const ARMS: [[u8; 4]; 128] = [
    // U+2500
    [1,1,0,0],[2,2,0,0],[0,0,1,1],[0,0,2,2],[1,1,0,0],[2,2,0,0],[0,0,1,1],[0,0,2,2],
    [1,1,0,0],[2,2,0,0],[0,0,1,1],[0,0,2,2],[0,1,0,1],[0,2,0,1],[0,1,0,2],[0,2,0,2],
    // U+2510
    [1,0,0,1],[2,0,0,1],[1,0,0,2],[2,0,0,2],[0,1,1,0],[0,2,1,0],[0,1,2,0],[0,2,2,0],
    [1,0,1,0],[2,0,1,0],[1,0,2,0],[2,0,2,0],[0,1,1,1],[0,2,1,1],[0,1,2,1],[0,1,1,2],
    // U+2520
    [0,1,2,2],[0,2,2,1],[0,2,1,2],[0,2,2,2],[1,0,1,1],[2,0,1,1],[1,0,2,1],[1,0,1,2],
    [1,0,2,2],[2,0,2,1],[2,0,1,2],[2,0,2,2],[1,1,0,1],[2,1,0,1],[1,2,0,1],[2,2,0,1],
    // U+2530
    [1,1,0,2],[2,1,0,2],[1,2,0,2],[2,2,0,2],[1,1,1,0],[2,1,1,0],[1,2,1,0],[2,2,1,0],
    [1,1,2,0],[2,1,2,0],[1,2,2,0],[2,2,2,0],[1,1,1,1],[2,1,1,1],[1,2,1,1],[2,2,1,1],
    // U+2540
    [1,1,2,1],[1,1,1,2],[1,1,2,2],[2,1,2,1],[1,2,2,1],[2,1,1,2],[1,2,1,2],[2,2,2,1],
    [2,2,1,2],[2,1,2,2],[1,2,2,2],[2,2,2,2],[1,1,0,0],[2,2,0,0],[0,0,1,1],[0,0,2,2],
    // U+2550
    [3,3,0,0],[0,0,3,3],[0,3,0,1],[0,1,0,3],[0,3,0,3],[3,0,0,1],[1,0,0,3],[3,0,0,3],
    [0,3,1,0],[0,1,3,0],[0,3,3,0],[3,0,1,0],[1,0,3,0],[3,0,3,0],[0,3,1,1],[0,1,3,3],
    // U+2560
    [0,3,3,3],[3,0,1,1],[1,0,3,3],[3,0,3,3],[3,3,0,1],[1,1,0,3],[3,3,0,3],[3,3,1,0],
    [1,1,3,0],[3,3,3,0],[3,3,1,1],[1,1,3,3],[3,3,3,3],[0,1,0,1],[1,0,0,1],[1,0,1,0],
    // U+2570
    [0,1,1,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[1,0,0,0],[0,0,1,0],[0,1,0,0],[0,0,0,1],
    [2,0,0,0],[0,0,2,0],[0,2,0,0],[0,0,0,2],[1,2,0,0],[0,0,1,2],[2,1,0,0],[0,0,2,1],
];

/// Whether `ch` is drawn by [`paint`] rather than taken from a font.
pub fn covers(ch: char) -> bool {
    ('\u{2500}'..='\u{259F}').contains(&ch)
}

/// Draw `ch` in `cell`. `light` is the thickness of a light line; it and the
/// cell's edges are expected on whole pixels.
pub fn paint(p: &Painter, cell: Rect, ch: char, color: Color32, light: f32) {
    match ch {
        '\u{256D}'..='\u{2570}' => arc(p, cell, ch, color, light),
        '\u{2571}'..='\u{2573}' => diagonals(p, cell, ch, color, light),
        '\u{2500}'..='\u{257F}' => {
            let arms = ARMS[(ch as usize - 0x2500) & 0x7F];
            lines(p, cell, arms, color, light);
        }
        '\u{2580}'..='\u{259F}' => block(p, cell, ch, color),
        _ => {}
    }
}

fn thickness(weight: u8, light: f32) -> f32 {
    match weight {
        LIGHT => light,
        HEAVY => 2.0 * light,
        _ => 0.0,
    }
}

/// The point the lines of `cell` cross at, placed so a light line through it
/// has its edges where `cell`'s are.
fn centre(cell: Rect, light: f32) -> Pos2 {
    let on_grid = |lo: f32, mid: f32| lo + ((mid - light / 2.0 - lo) / light).round() * light;
    pos2(
        on_grid(cell.left(), cell.center().x) + light / 2.0,
        on_grid(cell.top(), cell.center().y) + light / 2.0,
    )
}

/// How far past the centre one line of a double arm runs: `near` is the
/// crossing arm on the line's side, `far` the one on the other side.
fn double_reach(near: u8, far: u8, light: f32) -> f32 {
    match (near, far) {
        (DOUBLE, _) => -light,
        (NONE, DOUBLE) => 1.5 * light,
        (NONE, far) => thickness(far, light) / 2.0,
        _ => 0.0,
    }
}

/// How far past the centre a single arm runs, given the two arms crossing it
/// and the arm opposite.
fn single_reach(cross: (u8, u8), opposite: u8, light: f32) -> f32 {
    match cross {
        (DOUBLE, DOUBLE) if opposite == NONE => -light,
        (DOUBLE, DOUBLE) => 0.0,
        (DOUBLE, _) | (_, DOUBLE) if opposite == NONE => 1.5 * light,
        (DOUBLE, _) | (_, DOUBLE) => 0.0,
        (a, b) => thickness(a.max(b), light) / 2.0,
    }
}

fn lines(p: &Painter, cell: Rect, [l, r, u, d]: [u8; 4], color: Color32, light: f32) {
    let c = centre(cell, light);
    let fill = |x0: f32, x1: f32, y0: f32, y1: f32| {
        p.rect_filled(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)), 0.0, color);
    };
    // One arm: `along` runs from the cell edge to past the centre, `across`
    // is the line's own axis. `flip` mirrors the arm onto the far side.
    let arm = |weight: u8, cross: (u8, u8), opposite: u8, horizontal: bool, flip: bool| {
        let (edge, mid, other) = match (horizontal, flip) {
            (true, false) => (cell.left(), c.x, c.y),
            (true, true) => (cell.right(), c.x, c.y),
            (false, false) => (cell.top(), c.y, c.x),
            (false, true) => (cell.bottom(), c.y, c.x),
        };
        let sign = if flip { -1.0 } else { 1.0 };
        let line = |offset: f32, reach: f32, t: f32| {
            let end = mid + sign * reach;
            let (a0, a1) = (edge.min(end), edge.max(end));
            let (b0, b1) = (other + offset - t / 2.0, other + offset + t / 2.0);
            if horizontal {
                fill(a0, a1, b0, b1);
            } else {
                fill(b0, b1, a0, a1);
            }
        };
        match weight {
            NONE => {}
            DOUBLE => {
                line(-light, double_reach(cross.0, cross.1, light), light);
                line(light, double_reach(cross.1, cross.0, light), light);
            }
            w => line(
                0.0,
                single_reach(cross, opposite, light),
                thickness(w, light),
            ),
        }
    };
    arm(l, (u, d), r, true, false);
    arm(r, (u, d), l, true, true);
    arm(u, (l, r), d, false, false);
    arm(d, (l, r), u, false, true);
}

fn arc(p: &Painter, cell: Rect, ch: char, color: Color32, light: f32) {
    let c = centre(cell, light);
    let radius = cell.width().min(cell.height()) / 2.0;
    // Which side the horizontal and the vertical arm leave on.
    let (right, down) = match ch {
        '\u{256D}' => (true, true),
        '\u{256E}' => (false, true),
        '\u{256F}' => (false, false),
        _ => (true, false),
    };
    let (sx, sy) = (
        if right { 1.0 } else { -1.0 },
        if down { 1.0 } else { -1.0 },
    );
    let pivot = pos2(c.x + sx * radius, c.y + sy * radius);
    let mut points = vec![pos2(c.x, if down { cell.bottom() } else { cell.top() })];
    const STEPS: u8 = 8;
    for i in 0..=STEPS {
        let a = f32::from(i) / f32::from(STEPS) * std::f32::consts::FRAC_PI_2;
        points.push(pos2(
            pivot.x - sx * radius * a.cos(),
            pivot.y - sy * radius * a.sin(),
        ));
    }
    points.push(pos2(if right { cell.right() } else { cell.left() }, c.y));
    p.add(PathShape::line(points, Stroke::new(light, color)));
}

fn diagonals(p: &Painter, cell: Rect, ch: char, color: Color32, light: f32) {
    let stroke = Stroke::new(light, color);
    if ch != '\u{2572}' {
        p.line_segment([cell.left_bottom(), cell.right_top()], stroke);
    }
    if ch != '\u{2571}' {
        p.line_segment([cell.left_top(), cell.right_bottom()], stroke);
    }
}

fn block(p: &Painter, cell: Rect, ch: char, color: Color32) {
    let (w, h) = (cell.width(), cell.height());
    // A part of the cell, in eighths from its top-left corner.
    let part = |x0: f32, y0: f32, x1: f32, y1: f32, color: Color32| {
        p.rect_filled(
            Rect::from_min_max(
                pos2(cell.left() + w * x0 / 8.0, cell.top() + h * y0 / 8.0),
                pos2(cell.left() + w * x1 / 8.0, cell.top() + h * y1 / 8.0),
            ),
            0.0,
            color,
        );
    };
    let n = (ch as u32 - 0x2580) as u8;
    match n {
        0x00 => part(0.0, 0.0, 8.0, 4.0, color),
        0x01..=0x08 => part(0.0, 8.0 - f32::from(n), 8.0, 8.0, color),
        0x09..=0x0F => part(0.0, 0.0, f32::from(0x10 - n), 8.0, color),
        0x10 => part(4.0, 0.0, 8.0, 8.0, color),
        0x11..=0x13 => part(
            0.0,
            0.0,
            8.0,
            8.0,
            color.gamma_multiply(f32::from(n - 0x10) / 4.0),
        ),
        0x14 => part(0.0, 0.0, 8.0, 1.0, color),
        0x15 => part(7.0, 0.0, 8.0, 8.0, color),
        _ => {
            // Quadrants as bits: upper left, upper right, lower left, lower right.
            let quadrants: u8 = match n {
                0x16 => 0b0010,
                0x17 => 0b0001,
                0x18 => 0b1000,
                0x19 => 0b1011,
                0x1A => 0b1001,
                0x1B => 0b1110,
                0x1C => 0b1101,
                0x1D => 0b0100,
                0x1E => 0b0110,
                _ => 0b0111,
            };
            for (bit, x, y) in [
                (0b1000, 0.0, 0.0),
                (0b0100, 4.0, 0.0),
                (0b0010, 0.0, 4.0),
                (0b0001, 4.0, 4.0),
            ] {
                if quadrants & bit != 0 {
                    part(x, y, x + 4.0, y + 4.0, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_box_drawing_and_blocks_only() {
        assert!(covers('─') && covers('╬') && covers('█') && covers('▟'));
        assert!(!covers('a') && !covers('❯') && !covers('\u{25A0}'));
    }

    #[test]
    fn arms_match_a_few_known_glyphs() {
        let arms = |ch: char| ARMS[ch as usize - 0x2500];
        assert_eq!(arms('─'), [LIGHT, LIGHT, NONE, NONE]);
        assert_eq!(arms('┏'), [NONE, HEAVY, NONE, HEAVY]);
        assert_eq!(arms('┤'), [LIGHT, NONE, LIGHT, LIGHT]);
        assert_eq!(arms('╋'), [HEAVY; 4]);
        assert_eq!(arms('╔'), [NONE, DOUBLE, NONE, DOUBLE]);
        assert_eq!(arms('╢'), [LIGHT, NONE, DOUBLE, DOUBLE]);
        assert_eq!(arms('╿'), [NONE, NONE, HEAVY, LIGHT]);
    }

    #[test]
    fn double_corners_close_on_the_outside_and_stop_on_the_inside() {
        // ╔: the outer lines run past the centre, the inner ones stop short.
        assert_eq!(double_reach(NONE, DOUBLE, 1.0), 1.5);
        assert_eq!(double_reach(DOUBLE, NONE, 1.0), -1.0);
        // ═ meets its other half in the middle.
        assert_eq!(double_reach(NONE, NONE, 1.0), 0.0);
        // ╢: the single arm stops at the near line.
        assert_eq!(single_reach((DOUBLE, DOUBLE), NONE, 1.0), -1.0);
        // ┌ with a heavy upright covers the corner.
        assert_eq!(single_reach((NONE, HEAVY), NONE, 1.0), 1.0);
    }
}
