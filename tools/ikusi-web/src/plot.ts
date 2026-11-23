import type { Meta, Point, Scene } from './api';

export interface View {
  x: number;
  y: number;
  k: number;
}
interface Mark {
  x: number;
  y: number;
  node: Point;
  count: number;
  wiki: boolean;
  shape: number;
}
interface Line {
  a: number;
  b: number;
  weight: number;
  legacy: boolean;
  rel: string;
  confidence: number;
}
export interface Dot {
  x: number;
  y: number;
  r: number;
  mark: Mark;
}

function fit(points: Mark[], width: number, height: number): View {
  if (!points.length) return { x: width / 2, y: height / 2, k: 1 };
  let left = Infinity,
    right = -Infinity,
    top = Infinity,
    bottom = -Infinity;
  for (const p of points) {
    left = Math.min(left, p.x);
    right = Math.max(right, p.x);
    top = Math.min(top, p.y);
    bottom = Math.max(bottom, p.y);
  }
  const k = Math.max(
    0.001,
    Math.min(4, (width - 80) / (right - left || 1), (height - 80) / (bottom - top || 1)),
  );
  return { k, x: width / 2 - ((left + right) * k) / 2, y: height / 2 - ((top + bottom) * k) / 2 };
}

// A level is immutable between zoom thresholds. Panning visits only occupied
// tiles in view and their incident connections.
class Level {
  private buckets = new Map<string, number[]>();
  readonly incident: number[][];
  constructor(
    readonly marks: Mark[],
    readonly lines: Line[],
    private tile: number,
  ) {
    this.incident = marks.map(() => []);
    for (const [i, p] of marks.entries()) {
      const key = Math.floor(p.x / tile) + ',' + Math.floor(p.y / tile);
      const bucket = this.buckets.get(key);
      if (bucket) bucket.push(i);
      else this.buckets.set(key, [i]);
    }
    for (const [i, e] of lines.entries()) {
      this.incident[e.a].push(i);
      if (e.a !== e.b) this.incident[e.b].push(i);
    }
  }
  visible(view: View, w: number, h: number) {
    const left = (-24 - view.x) / view.k,
      right = (w + 24 - view.x) / view.k;
    const top = (-24 - view.y) / view.k,
      bottom = (h + 24 - view.y) / view.k;
    const x0 = Math.floor(left / this.tile),
      x1 = Math.floor(right / this.tile);
    const y0 = Math.floor(top / this.tile),
      y1 = Math.floor(bottom / this.tile);
    const found: number[] = [];
    const visit = (i: number) => {
      const p = this.marks[i];
      if (p.x >= left && p.x <= right && p.y >= top && p.y <= bottom) found.push(i);
    };
    if ((x1 - x0 + 1) * (y1 - y0 + 1) > this.marks.length) {
      for (let i = 0; i < this.marks.length; i++) visit(i);
    } else {
      for (let y = y0; y <= y1; y++)
        for (let x = x0; x <= x1; x++) this.buckets.get(x + ',' + y)?.forEach(visit);
    }
    return found;
  }
}

// At the lower end of a zoom tier a hub disk can be smaller than its badge.
// Coalesce only nearby badges, keeping the strongest group's actual position as
// the zoom target. The screen-space gap is at least 50px throughout the tier.
function spaceBadges(marks: Mark[], size: number, focus: string) {
  const spacing = size * 0.7;
  const cells = new Map<string, number[]>();
  const spaced: Mark[] = [];
  const index = new Int32Array(marks.length);
  const ranked = marks
    .map((_, i) => i)
    .sort((a, b) => marks[b].count - marks[a].count || marks[b].node.d - marks[a].node.d);
  for (const i of ranked) {
    const mark = marks[i];
    const x = Math.floor(mark.x / spacing),
      y = Math.floor(mark.y / spacing);
    let nearest = -1,
      distance = spacing * spacing;
    if (mark.node.id !== focus) {
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          for (const candidate of cells.get(x + dx + ',' + (y + dy)) ?? []) {
            const other = spaced[candidate];
            const d = (mark.x - other.x) ** 2 + (mark.y - other.y) ** 2;
            if (d < distance) {
              nearest = candidate;
              distance = d;
            }
          }
        }
      }
    }
    if (nearest >= 0) {
      spaced[nearest].count += mark.count;
      index[i] = nearest;
    } else {
      index[i] = spaced.length;
      spaced.push(mark);
      if (mark.node.id === focus) continue;
      const key = x + ',' + y;
      const cell = cells.get(key);
      if (cell) cell.push(index[i]);
      else cells.set(key, [index[i]]);
    }
  }
  return { marks: spaced, index };
}

export class Plot {
  private readonly base: Level;
  private readonly visibleIndex = new Map<string, number>();
  private readonly hubs = new Map<number, { members: number; anchor: Point }>();
  private recent?: { size: number; level: Level };
  private previous?: { size: number; level: Level };
  constructor(
    scene: Scene,
    meta: Meta,
    hidden: Set<string>,
    readonly focus: string,
  ) {
    const types = new Map(meta.types.map((t) => [t.t, t]));
    const indexes = new Int32Array(scene.nodes.length).fill(-1);
    const marks: Mark[] = [];
    scene.nodes.forEach((node, i) => {
      if (hidden.has(node.t)) return;
      const type = types.get(node.t);
      if (node.g !== undefined) {
        const anchor = scene.nodes[node.g];
        const group = this.hubs.get(node.g);
        if (group) group.members++;
        else this.hubs.set(node.g, { anchor, members: 1 });
      }
      indexes[i] = marks.length;
      this.visibleIndex.set(node.id, marks.length);
      marks.push({ x: node.x, y: node.y, node, count: 1, wiki: type?.w === 'wiki', shape: type?.s ?? 0 });
    });
    const lines: Line[] = [];
    for (const e of scene.edges) {
      const a = indexes[e.s],
        b = indexes[e.t];
      if (a >= 0 && b >= 0)
        lines.push({ a, b, weight: 1, rel: e.r, legacy: e.r === 'MENTIONS', confidence: e.c });
    }
    this.base = new Level(marks, lines, 96);
  }
  fit(w: number, h: number) {
    return fit(this.base.marks, w, h);
  }
  level(k: number) {
    // Keep hub neighborhoods together until there is space to open them. World-aligned cells remain
    // stable while dragging. Keep focused nodes independently selectable.
    const size = 2 ** Math.floor(Math.log2(144 / k));
    if (this.base.marks.length <= 48 || size < 16 || k >= 3) return this.base;
    if (this.recent?.size === size) return this.recent.level;
    if (this.previous?.size === size) {
      [this.recent, this.previous] = [this.previous, this.recent];
      return this.recent.level;
    }
    const bins = new Map<string, number>();
    const marks: Mark[] = [];
    const indexes = this.base.marks.map((p) => {
      const group = p.node.g === undefined ? undefined : this.hubs.get(p.node.g);
      const collapsed = group && group.members >= 4 && 24 * Math.sqrt(group.members) <= 3 * size;
      const key =
        p.node.id === this.focus
          ? 'focus'
          : collapsed
            ? 'hub' + p.node.g
            : Math.floor(p.x / size) + ',' + Math.floor(p.y / size);
      const i = bins.get(key);
      if (i === undefined) {
        const first = marks.length;
        bins.set(key, first);
        marks.push({ ...p });
        return first;
      }
      const m = marks[i];
      m.count++;
      m.x += p.x;
      m.y += p.y;
      if (p.node.d > m.node.d) {
        m.node = p.node;
        m.shape = p.shape;
        m.wiki = p.wiki;
      }
      return i;
    });
    for (const [key, index] of bins) {
      const m = marks[index];
      m.x /= m.count;
      m.y /= m.count;
      if (key.startsWith('hub')) {
        const g = this.hubs.get(Number(key.slice(3)));
        const i = g && g.anchor.id !== this.focus ? this.visibleIndex.get(g.anchor.id) : undefined;
        if (i !== undefined) {
          const center = this.base.marks[i];
          m.node = center.node;
          m.shape = center.shape;
          m.wiki = center.wiki;
        }
      }
    }
    const spaced = spaceBadges(marks, size, this.focus);
    for (let i = 0; i < indexes.length; i++) indexes[i] = spaced.index[indexes[i]];
    const pairs = new Map<number, Map<number, Line>>();
    const lines: Line[] = [];
    for (const e of this.base.lines) {
      const a = Math.min(indexes[e.a], indexes[e.b]),
        b = Math.max(indexes[e.a], indexes[e.b]);
      if (a === b) continue;
      let row = pairs.get(a);
      if (!row) {
        row = new Map();
        pairs.set(a, row);
      }
      const existing = row.get(b);
      if (existing) {
        existing.weight++;
        existing.legacy &&= e.legacy;
        existing.confidence = Math.max(existing.confidence, e.confidence);
      } else {
        const line = { ...e, a, b };
        row.set(b, line);
        lines.push(line);
      }
    }
    lines.sort((a, b) => b.weight - a.weight);
    const level = new Level(spaced.marks, lines, size);
    this.previous = this.recent;
    this.recent = { size, level };
    return level;
  }
}

export function colors(canvas: HTMLCanvasElement) {
  const css = getComputedStyle(canvas);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    wiki: token('--graph-wiki'),
    code: token('--graph-code'),
    paper: token('--plot-bg'),
    edge: token('--graph-edge'),
    ink: token('--o-text'),
    muted: token('--o-text-2'),
    wikiSoft: token('--mint-bg'),
    codeSoft: token('--violet-bg'),
  };
}
function shape(ctx: CanvasRenderingContext2D, d: Dot) {
  const { x, y, r, mark } = d;
  if (mark.shape === 0 || mark.shape === 3) {
    ctx.moveTo(x + r, y);
    ctx.arc(x, y, r, 0, Math.PI * 2);
    if (mark.shape === 3) {
      ctx.moveTo(x + r * 0.4, y);
      ctx.arc(x, y, r * 0.4, 0, Math.PI * 2, true);
    }
  } else if (mark.shape === 1) ctx.rect(x - r, y - r, 2 * r, 2 * r);
  else {
    const sides = mark.shape === 2 ? 4 : mark.shape === 4 ? 3 : 6;
    for (let i = 0; i < sides; i++) {
      const angle = (i * 2 * Math.PI) / sides - Math.PI / 2;
      const px = x + r * Math.cos(angle),
        py = y + r * Math.sin(angle);
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
  }
}

export function paint(
  canvas: HTMLCanvasElement,
  plot: Plot,
  palette: ReturnType<typeof colors>,
  view: View,
  w: number,
  h: number,
): Dot[] {
  const ctx = canvas.getContext('2d');
  if (!ctx || !w || !h) return [];
  const dpr = Math.min(devicePixelRatio, 2);
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const level = plot.level(view.k),
    visible = level.visible(view, w, h);
  const dots = visible.map((i) => {
    const mark = level.marks[i];
    const r =
      mark.count > 1
        ? Math.min(23, 9 + Math.log2(mark.count) * 1.7)
        : Math.max(
            mark.node.id === plot.focus ? 7 : 2.4,
            Math.min(12, (3 + Math.sqrt(mark.node.d) * 0.7) * Math.sqrt(view.k)),
          );
    return { mark, x: mark.x * view.k + view.x, y: mark.y * view.k + view.y, r };
  });
  const dense = level.marks.length > 75 || level.marks.some((m) => m.count > 1);
  const linkLabels: { text: string; x: number; y: number }[] = [];
  // Budget the busiest hubs and batch strokes. Thousands of incident paths
  // contribute little at a distance; the focused neighborhood exposes them.
  const paths = Array.from({ length: 5 }, () => new Path2D());
  const focusPath = 4;
  const seen = new Set<number>();
  for (const i of visible) {
    const incident = level.incident[i];
    const limit = dense ? Math.min(incident.length, 4) : incident.length;
    for (let j = 0; j < limit; j++) {
      const index = incident[j];
      if (seen.has(index)) continue;
      seen.add(index);
      const e = level.lines[index],
        a = level.marks[e.a],
        b = level.marks[e.b];
      const lit = a.node.id === plot.focus || b.node.id === plot.focus;
      const path = paths[lit ? focusPath : Number(e.legacy) + (e.confidence < 0.75 ? 2 : 0)];
      const x = a.x * view.k + view.x,
        y = a.y * view.k + view.y,
        bx = b.x * view.k + view.x,
        by = b.y * view.k + view.y;
      path.moveTo(x, y);
      path.lineTo(bx, by);
      if (!dense && (lit || level.lines.length < 45) && Math.hypot(x - bx, y - by) > 95)
        linkLabels.push({ text: e.rel, x: (x + bx) / 2, y: (y + by) / 2 });
    }
  }
  for (const [i, path] of paths.entries()) {
    ctx.strokeStyle = i === focusPath ? palette.wiki : palette.edge;
    ctx.lineWidth = i === focusPath ? 1.4 : 1;
    ctx.globalAlpha = i === focusPath ? 0.8 : (dense ? 0.27 : 0.55) * (i >= 2 ? 0.5 : 1);
    ctx.setLineDash(i % 2 === 1 ? [3, 4] : []);
    ctx.stroke(path);
  }
  ctx.globalAlpha = 1;
  ctx.setLineDash([]);
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  for (const d of dots) {
    const color = d.mark.wiki ? palette.wiki : palette.code;
    ctx.beginPath();
    if (d.mark.count > 1) {
      ctx.arc(d.x, d.y, d.r, 0, 2 * Math.PI);
      ctx.fillStyle = d.mark.wiki ? palette.wikiSoft : palette.codeSoft;
      ctx.fill();
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = color;
      ctx.stroke();
      ctx.font = '600 12px "DM Sans", sans-serif';
      ctx.fillStyle = palette.ink;
      ctx.fillText(String(d.mark.count), d.x, d.y, 2 * d.r - 4);
    } else {
      shape(ctx, d);
      ctx.fillStyle = color;
      ctx.fill();
      if (d.mark.node.id === plot.focus) {
        ctx.beginPath();
        ctx.arc(d.x, d.y, d.r + 4, 0, Math.PI * 2);
        ctx.lineWidth = 2;
        ctx.strokeStyle = color;
        ctx.stroke();
      }
    }
  }
  const ranked = [...dots].sort(
    (a, b) =>
      Number(b.mark.node.id === plot.focus) - Number(a.mark.node.id === plot.focus) ||
      b.mark.count - a.mark.count ||
      Number(b.mark.node.h === 1) - Number(a.mark.node.h === 1) ||
      b.mark.node.d - a.mark.node.d,
  );
  const budget = Math.max(4, Math.min(dense ? 18 : 60, Math.floor((w * h) / (dense ? 26000 : 10000))));
  const boxes: { x: number; y: number; half: number }[] = [];
  function label(text: string, x: number, y: number, minor: boolean) {
    if (!ctx || boxes.length >= budget || y < 9 || y > h - 9) return;
    ctx.font = minor ? '10px monospace' : '600 12px "DM Sans", sans-serif';
    const half = ctx.measureText(text).width / 2 + 4;
    if (
      x - half < 1 ||
      x + half > w - 1 ||
      boxes.some((b) => Math.abs(b.y - y) < 17 && Math.abs(b.x - x) < b.half + half)
    )
      return;
    if (dots.some((d) => Math.abs(d.y - y) < d.r + 6 && Math.abs(d.x - x) < half + d.r)) return;
    boxes.push({ x, y, half });
    ctx.lineJoin = 'round';
    ctx.lineWidth = 4;
    ctx.strokeStyle = palette.paper;
    ctx.strokeText(text, x, y);
    ctx.fillStyle = minor ? palette.muted : palette.ink;
    ctx.fillText(text, x, y);
  }
  for (const d of ranked.slice(0, budget * 4)) {
    if (boxes.length >= budget) break;
    const name = d.mark.node.n;
    label(name.length > 29 ? name.slice(0, 28) + '…' : name, d.x, d.y - d.r - 10, false);
  }
  for (const l of linkLabels) label(l.text, l.x, l.y, true);
  return dots;
}
export function hitTest(dots: Dot[], x: number, y: number) {
  let best: Dot | undefined,
    distance = Infinity;
  for (const d of dots) {
    const delta = Math.hypot(x - d.x, y - d.y);
    if (delta <= d.r + 5 && delta < distance) {
      best = d;
      distance = delta;
    }
  }
  return best;
}
