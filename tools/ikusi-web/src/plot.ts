import type { Meta, Point, Scene } from './api';

interface View {
  x: number;
  y: number;
  k: number;
}
interface Dot {
  x: number;
  y: number;
  r: number;
  node: Point;
}
export function fit(nodes: Point[], w: number, h: number): View {
  if (!nodes.length) return { x: w / 2, y: h / 2, k: 1 };
  const range = (values: number[]) => {
    values.sort((a, b) => a - b);
    const trim = values.length > 150 ? Math.floor(values.length * 0.015) : 0;
    return [values[trim], values[values.length - 1 - trim]];
  };
  const [x0, x1] = range(nodes.map((n) => n.x)),
    [y0, y1] = range(nodes.map((n) => n.y));
  const k = Math.max(0.015, Math.min(4, (w - 88) / (x1 - x0 || 1), (h - 110) / (y1 - y0 || 1)));
  return { k, x: w / 2 - ((x1 + x0) * k) / 2, y: h / 2 - ((y1 + y0) * k) / 2 };
}
function shape(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, kind: number) {
  ctx.beginPath();
  if (kind === 0 || kind === 3) {
    ctx.arc(x, y, r, 0, 2 * Math.PI);
    if (kind === 3) {
      ctx.moveTo(x + r * 0.4, y);
      ctx.arc(x, y, r * 0.4, 0, 2 * Math.PI, true);
    }
  } else if (kind === 1) ctx.roundRect(x - r, y - r, 2 * r, 2 * r, 1);
  else {
    const sides = kind === 2 ? 4 : kind === 4 ? 3 : 6;
    for (let i = 0; i < sides; i++) {
      const a = (i * Math.PI * 2) / sides - Math.PI / 2;
      const px = x + r * Math.cos(a),
        py = y + r * Math.sin(a);
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
  }
  ctx.fill();
}
export function paint(
  canvas: HTMLCanvasElement,
  scene: Scene,
  meta: Meta,
  focus: string,
  hidden: Set<string>,
  view: View,
  width: number,
  height: number,
): Dot[] {
  const ctx = canvas.getContext('2d');
  if (!ctx || !width || !height) return [];
  const dpr = Math.min(devicePixelRatio, 2);
  if (canvas.width !== Math.round(width * dpr) || canvas.height !== Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  const css = getComputedStyle(canvas);
  const color = (name: string) => css.getPropertyValue(name).trim();
  const wiki = color('--graph-wiki'),
    code = color('--graph-code'),
    ink = color('--o-text');
  const edge = color('--graph-edge'),
    paper = color('--plot-bg');
  const types = new Map(meta.types.map((t) => [t.t, t]));
  const dots = scene.nodes.map((node) => ({
    node,
    x: node.x * view.k + view.x,
    y: node.y * view.k + view.y,
    r: Math.max(node.id === focus ? 7 : 2, Math.min(13, (3.5 + Math.sqrt(node.d) * 0.9) * Math.sqrt(view.k))),
  }));
  const visible = dots.filter(
    (d) => !hidden.has(d.node.t) && d.x >= -15 && d.x <= width + 15 && d.y >= -15 && d.y <= height + 15,
  );
  const labels: { x: number; y: number; text: string; major: boolean }[] = [];
  ctx.lineWidth = 1;
  for (const e of scene.edges) {
    const a = dots[e.s],
      b = dots[e.t];
    if (hidden.has(a.node.t) || hidden.has(b.node.t)) continue;
    const lit = !!focus && (a.node.id === focus || b.node.id === focus);
    ctx.strokeStyle = lit ? wiki : edge;
    ctx.globalAlpha = lit ? 0.72 : (scene.nodes.length > 500 ? 0.28 : 0.58) * e.c;
    ctx.setLineDash(e.r === 'MENTIONS' ? [3, 4] : []);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    if ((lit || scene.edges.length < 45) && Math.hypot(a.x - b.x, a.y - b.y) > 65) {
      labels.push({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, text: e.r, major: false });
    }
  }
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
  for (const d of visible) {
    const type = types.get(d.node.t);
    ctx.fillStyle = type?.w === 'wiki' ? wiki : code;
    shape(ctx, d.x, d.y, d.r, type?.s ?? 0);
    if (d.node.id === focus) {
      ctx.beginPath();
      ctx.arc(d.x, d.y, d.r + 4, 0, Math.PI * 2);
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
  }
  const sorted = [...visible].sort(
    (a, b) =>
      Number(b.node.id === focus) - Number(a.node.id === focus) ||
      Number(b.node.h === 1) - Number(a.node.h === 1) ||
      b.node.d - a.node.d,
  );
  const text = sorted
    .slice(0, 420)
    .map((d) => ({
      x: d.x,
      y: d.y - d.r - 8,
      text: d.node.n.length > 32 ? d.node.n.slice(0, 31) + '…' : d.node.n,
      major: true,
    }));
  const boxes: { x: number; y: number; w: number }[] = [];
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.lineJoin = 'round';
  for (const label of [...text, ...labels]) {
    const { x, y, major } = label;
    if (y < 8 || y > height - 8 || x < 0 || x > width) continue;
    ctx.font = major ? '600 12px "DM Sans", sans-serif' : '10px monospace';
    const w = ctx.measureText(label.text).width / 2 + 3;
    if (boxes.some((b) => Math.abs(y - b.y) < 15 && Math.abs(x - b.x) < w + b.w)) continue;
    boxes.push({ x, y, w });
    ctx.lineWidth = 4;
    ctx.strokeStyle = paper;
    ctx.strokeText(label.text, x, y);
    ctx.fillStyle = major ? ink : color('--o-text-2');
    ctx.fillText(label.text, x, y);
  }
  return visible;
}
export function hitTest(dots: Dot[], x: number, y: number) {
  let best: Dot | undefined;
  let distance = Infinity;
  for (const d of dots) {
    const delta = Math.hypot(x - d.x, y - d.y);
    if (delta <= d.r + 6 && delta < distance) {
      best = d;
      distance = delta;
    }
  }
  return best;
}
