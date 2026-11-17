"use strict";

const NS = "http://www.w3.org/2000/svg";
const $ = (id) => document.getElementById(id);

const SHAPES = [
  (s) => `M 0 ${-s} A ${s} ${s} 0 1 0 0 ${s} A ${s} ${s} 0 1 0 0 ${-s} Z`,
  (s) => `M ${-s} ${-s} H ${s} V ${s} H ${-s} Z`,
  (s) => `M 0 ${-s * 1.3} L ${s * 1.3} 0 L 0 ${s * 1.3} L ${-s * 1.3} 0 Z`,
  (s) => `M 0 ${-s} A ${s} ${s} 0 1 0 0 ${s} A ${s} ${s} 0 1 0 0 ${-s} Z M 0 ${-s / 2.2} A ${s / 2.2} ${s / 2.2} 0 1 1 0 ${s / 2.2} A ${s / 2.2} ${s / 2.2} 0 1 1 0 ${-s / 2.2} Z`,
  (s) => `M 0 ${-s * 1.25} L ${s * 1.1} ${s * 0.8} L ${-s * 1.1} ${s * 0.8} Z`,
  (s) => { let p = ""; for (let i = 0; i < 6; i++) { const a = Math.PI / 6 + i * Math.PI / 3; p += (i ? "L" : "M") + (Math.cos(a) * s * 1.15).toFixed(2) + " " + (Math.sin(a) * s * 1.15).toFixed(2) + " "; } return p + "Z"; },
];

const state = {
  meta: null,
  ids: new Set(),
  shapeOf: {},
  worldOf: {},
  mode: "focus",
  depth: 2,
  focus: null,
  detail: null,
  scene: null,
  hidden: new Set(),
  trail: [],
  view: { x: 0, y: 0, k: 1 },
  els: { nodes: [], edges: [], labels: [], edgeLabels: [] },
  seq: 0,
};

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) =>
  ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

async function api(path, params) {
  const url = new URL(path, location.origin);
  for (const [k, v] of Object.entries(params || {})) {
    if (v !== undefined && v !== null && v !== "") url.searchParams.set(k, v);
  }
  const res = await fetch(url, { headers: { accept: "application/json" } });
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || res.statusText);
  return res.json();
}

const glyph = (type, size = 11) => {
  const s = state.shapeOf[type] ?? 0;
  const w = state.worldOf[type] || "code";
  return `<svg class="glyph" width="${size}" height="${size}" viewBox="-8 -8 16 16" aria-hidden="true"><path d="${SHAPES[s](5)}" fill="var(--${w})"/></svg>`;
};

/* ------------------------------------------------------------------ canvas */

const canvas = $("canvas");
const gScene = $("scene");
const gEdges = $("g-edges");
const gEdgeLabels = $("g-edgelabels");
const gNodes = $("g-nodes");
const gLabels = $("g-labels");
const tip = $("tip");

const radius = (d) => Math.min(15, 3.6 + Math.sqrt(d) * 1.35);

function applyView() {
  const { x, y, k } = state.view;
  gScene.setAttribute("transform", `translate(${x} ${y}) scale(${k})`);
  // Inline, not an SVG attribute: a `font-size` in the stylesheet silently wins
  // over a presentation attribute, and the labels then scale with the zoom.
  const fs = (v) => `${(v / k).toFixed(3)}px`;
  state.els.labels.forEach((el) => { el.style.fontSize = fs(el.dataset.big ? 12 : 10.5); });
  state.els.edgeLabels.forEach((el) => { el.style.fontSize = fs(9); });
  state.els.labels.forEach((el) => el.setAttribute("stroke-width", (2.6 / k).toFixed(3)));
  state.els.edgeLabels.forEach((el) => el.setAttribute("stroke-width", (3 / k).toFixed(3)));
  scheduleLabels();
}

let labelFrame = 0;
function scheduleLabels() {
  cancelAnimationFrame(labelFrame);
  labelFrame = requestAnimationFrame(layoutLabels);
}

/// Labels are drawn at a fixed pixel size, so whether two of them collide
/// depends on the zoom — a hub whose twelve DECIDES edges leave from one point
/// is a smear at arm's length and perfectly readable once you lean in. Deciding
/// this at render time got the answer wrong at every zoom but one; deciding it
/// per frame, in screen space, means zooming in uncovers more.
function layoutLabels() {
  const k = state.view.k;
  const boxes = [];
  const place = (el, halfW, halfH) => {
    if (el.dataset.off === "1") { el.style.display = "none"; return; }
    const x = Number(el.getAttribute("x")) * k;
    const y = Number(el.getAttribute("y")) * k;
    if (boxes.some((o) => Math.abs(o.x - x) < o.w + halfW && Math.abs(o.y - y) < o.h + halfH)) {
      el.style.display = "none";
      return;
    }
    boxes.push({ x, y, w: halfW, h: halfH });
    el.style.display = "";
  };
  for (const el of state.els.labels) place(el, el.textContent.length * 3.0 + 4, 8);
  for (const el of state.els.edgeLabels) place(el, el.textContent.length * 3.2 + 4, 7);
}

function fit(subset) {
  const use = (subset && subset.length ? subset : state.scene?.nodes) || [];
  if (!use.length) return;
  const r = canvas.getBoundingClientRect();
  const band = (vals) => {
    const s = vals.slice().sort((a, b) => a - b);
    const at = (q) => s[Math.min(s.length - 1, Math.max(0, Math.round((s.length - 1) * q)))];
    return [at(0.02), at(0.98)];
  };
  const [x0, x1] = band(use.map((n) => n.x));
  const [y0, y1] = band(use.map((n) => n.y));
  const pad = 56;
  let k = Math.min((r.width - pad * 2) / (x1 - x0 || 1), (r.height - pad * 2) / (y1 - y0 || 1));
  k = Math.min(Math.max(k, 0.05), 5);
  state.view.k = k;
  state.view.x = (r.width - (x1 - x0) * k) / 2 - x0 * k;
  state.view.y = (r.height - (y1 - y0) * k) / 2 - y0 * k;
  applyView();
}

function fitVisible() {
  const scene = state.scene;
  if (!scene) return;
  const live = new Set();
  scene.edges.forEach((e) => {
    if (state.hidden.has(scene.nodes[e.s].t) || state.hidden.has(scene.nodes[e.t].t)) return;
    live.add(e.s); live.add(e.t);
  });
  const connected = scene.nodes.filter((_, i) => live.has(i));
  const shown = scene.nodes.filter((n) => !state.hidden.has(n.t));
  fit(connected.length > 3 ? connected : shown);
}

function renderScene(scene, focusId) {
  state.scene = scene;
  gEdges.replaceChildren();
  gEdgeLabels.replaceChildren();
  gNodes.replaceChildren();
  gLabels.replaceChildren();
  state.els = { nodes: [], edges: [], labels: [], edgeLabels: [] };

  const byIndex = scene.nodes;
  const focusIdx = focusId ? byIndex.findIndex((n) => n.id === focusId) : -1;

  const incident = new Set();
  scene.edges.forEach((e, i) => { if (e.s === focusIdx || e.t === focusIdx) incident.add(i); });

  scene.edges.forEach((e, i) => {
    const a = byIndex[e.s], b = byIndex[e.t];
    const el = document.createElementNS(NS, "line");
    el.setAttribute("x1", a.x); el.setAttribute("y1", a.y);
    el.setAttribute("x2", b.x); el.setAttribute("y2", b.y);
    el.setAttribute("class", "edge" + (e.r === "MENTIONS" ? " legacy" : ""));
    el.setAttribute("opacity", (0.28 + e.c * 0.42).toFixed(2));
    gEdges.appendChild(el);
    state.els.edges.push(el);
  });

  // Verbs are the point of this graph, so label every edge when the picture is
  // small enough to read and only the focused node's edges when it is not.
  // Every edge of a hub starts at the same pixel, so a label sits two thirds of
  // the way OUT along its edge — at the midpoint they stack into a smear — and
  // one that would still land on an earlier label is dropped.
  const labelAll = scene.edges.length <= 400;
  const order = scene.edges.map((_, i) => i)
    .filter((i) => labelAll || incident.has(i))
    .sort((a, b) => (incident.has(b) ? 1 : 0) - (incident.has(a) ? 1 : 0));
  for (const i of order) {
    const e = scene.edges[i];
    const a = byIndex[e.s], b = byIndex[e.t];
    // Every edge of a hub starts at the same pixel, so the label of one that
    // touches the focus sits two thirds of the way OUT along it.
    const near = e.s === focusIdx ? a : e.t === focusIdx ? b : null;
    const far = near === a ? b : near === b ? a : null;
    if (Math.hypot(a.x - b.x, a.y - b.y) < 18) continue;
    const at = near ? 0.68 : 0.5;
    const x = near ? near.x + (far.x - near.x) * at : (a.x + b.x) / 2;
    const y = near ? near.y + (far.y - near.y) * at : (a.y + b.y) / 2;
    const el = document.createElementNS(NS, "text");
    el.setAttribute("class", "edge-label");
    el.setAttribute("x", x.toFixed(1));
    el.setAttribute("y", (y - 2).toFixed(1));
    el.textContent = e.r;
    el.dataset.i = i;
    gEdgeLabels.appendChild(el);
    state.els.edgeLabels.push(el);
  }

  byIndex.forEach((n, i) => {
    const el = document.createElementNS(NS, "path");
    el.setAttribute("d", SHAPES[state.shapeOf[n.t] ?? 0](i === focusIdx ? Math.max(radius(n.d), 10) : radius(n.d)));
    el.setAttribute("transform", `translate(${n.x} ${n.y})`);
    el.setAttribute("class", "node w-" + (state.worldOf[n.t] || "code"));
    el.dataset.i = i;
    gNodes.appendChild(el);
    state.els.nodes.push(el);
  });

  if (focusIdx >= 0) {
    const n = byIndex[focusIdx];
    const ring = document.createElementNS(NS, "circle");
    ring.setAttribute("class", "focus-ring");
    ring.setAttribute("cx", n.x); ring.setAttribute("cy", n.y);
    ring.setAttribute("r", Math.max(radius(n.d), 10) + 8);
    gNodes.insertBefore(ring, gNodes.firstChild);
  }

  // Ranked, not filtered: the focus first, then its immediate neighbours, then
  // the hubs. layoutLabels() decides per frame how many of them fit.
  const ranked = byIndex.map((n, i) => [i, (i === focusIdx ? 1e6 : 0) + (n.h === 1 ? 1e3 : 0) + n.d])
    .sort((a, b) => b[1] - a[1]);
  const cap = 420;
  for (const [i] of ranked) {
    if (state.els.labels.length >= cap) break;
    const n = byIndex[i];
    const el = document.createElementNS(NS, "text");
    el.setAttribute("class", "label" + (i === focusIdx ? " focus" : ""));
    el.setAttribute("x", n.x);
    el.setAttribute("y", n.y - radius(n.d) - 5);
    el.setAttribute("text-anchor", "middle");
    const text = n.n && n.n !== n.id ? n.n : n.id;
    el.textContent = text.length > 34 ? text.slice(0, 33) + "…" : text;
    el.dataset.i = i;
    if (i === focusIdx) el.dataset.big = "1";
    gLabels.appendChild(el);
    state.els.labels.push(el);
  }

  highlight(focusIdx);
  refreshVisibility();
}

function highlight(focusIdx) {
  const scene = state.scene;
  if (!scene) return;
  const lit = new Set();
  if (focusIdx >= 0) {
    lit.add(focusIdx);
    scene.edges.forEach((e) => {
      if (e.s === focusIdx) lit.add(e.t);
      if (e.t === focusIdx) lit.add(e.s);
    });
  }
  const dimming = focusIdx >= 0 && scene.nodes.length > 14;
  state.els.nodes.forEach((el, i) => {
    el.classList.toggle("focus", i === focusIdx);
    el.classList.toggle("hot", lit.has(i) && i !== focusIdx);
    el.classList.toggle("dim", dimming && !lit.has(i));
  });
  state.els.edges.forEach((el, i) => {
    const e = scene.edges[i];
    const on = e.s === focusIdx || e.t === focusIdx;
    el.classList.toggle("lit", on);
    el.classList.toggle("dim", dimming && !on);
  });
  state.els.labels.forEach((el) => {
    const i = Number(el.dataset.i);
    el.classList.toggle("dim", dimming && !lit.has(i));
  });
}

function refreshVisibility() {
  const scene = state.scene;
  if (!scene) return;
  const off = (i) => state.hidden.has(scene.nodes[i].t);
  state.els.nodes.forEach((el, i) => el.style.display = off(i) ? "none" : "");
  state.els.edges.forEach((el, i) => {
    const e = scene.edges[i];
    el.style.display = off(e.s) || off(e.t) ? "none" : "";
  });
  state.els.edgeLabels.forEach((el) => {
    const e = scene.edges[Number(el.dataset.i)];
    el.dataset.off = off(e.s) || off(e.t) ? "1" : "0";
  });
  state.els.labels.forEach((el) => { el.dataset.off = off(Number(el.dataset.i)) ? "1" : "0"; });
  scheduleLabels();
  const shown = scene.nodes.filter((_, i) => !off(i)).length;
  const shownEdges = scene.edges.filter((e) => !off(e.s) && !off(e.t)).length;
  const held = scene.held ? ` · ${scene.held} hub${scene.held > 1 ? "s" : ""} left unexpanded` : "";
  $("status").textContent = state.mode === "all"
    ? `whole graph · ${shown} nodes · ${shownEdges} edges`
    : `${shown} nodes within ${state.depth} hop${state.depth > 1 ? "s" : ""} · ${shownEdges} edges${held}`;
}

canvas.addEventListener("wheel", (ev) => {
  ev.preventDefault();
  const r = canvas.getBoundingClientRect();
  const px = ev.clientX - r.left, py = ev.clientY - r.top;
  const k = Math.min(14, Math.max(0.04, state.view.k * Math.exp(-ev.deltaY * 0.0015)));
  state.view.x = px - (px - state.view.x) * (k / state.view.k);
  state.view.y = py - (py - state.view.y) * (k / state.view.k);
  state.view.k = k;
  applyView();
}, { passive: false });

let drag = null;
canvas.addEventListener("pointerdown", (ev) => {
  drag = { x: ev.clientX - state.view.x, y: ev.clientY - state.view.y, moved: false };
  canvas.classList.add("dragging");
  canvas.setPointerCapture(ev.pointerId);
});
canvas.addEventListener("pointermove", (ev) => {
  if (!drag) return;
  drag.moved = true;
  state.view.x = ev.clientX - drag.x;
  state.view.y = ev.clientY - drag.y;
  applyView();
});
canvas.addEventListener("pointerup", () => { canvas.classList.remove("dragging"); setTimeout(() => (drag = null), 0); });

gNodes.addEventListener("click", (ev) => {
  const i = ev.target.dataset && ev.target.dataset.i;
  if (i === undefined || (drag && drag.moved)) return;
  go(state.scene.nodes[Number(i)].id);
});
gLabels.addEventListener("click", (ev) => {
  const i = ev.target.dataset && ev.target.dataset.i;
  if (i !== undefined) go(state.scene.nodes[Number(i)].id);
});

gNodes.addEventListener("mousemove", (ev) => {
  const i = ev.target.dataset && ev.target.dataset.i;
  if (i === undefined) { tip.style.opacity = 0; return; }
  const n = state.scene.nodes[Number(i)];
  tip.innerHTML = `<b>${esc(n.n)}</b><span class="id mono muted">${esc(n.id)}</span>` +
    `<div class="muted">${esc(n.t)} · ${n.d} edges</div>` +
    (n.s ? `<div>${esc(n.s)}</div>` : "");
  const r = canvas.getBoundingClientRect();
  tip.style.left = Math.min(ev.clientX - r.left + 14, Math.max(4, r.width - 352)) + "px";
  tip.style.top = Math.min(ev.clientY - r.top + 14, r.height - 90) + "px";
  tip.style.opacity = 1;
});
gNodes.addEventListener("mouseleave", () => { tip.style.opacity = 0; });

/* ----------------------------------------------------------------- sidebar */

const RELATION_DOC = {
  PART_OF: "sub-project of", DEPENDS_ON: "cannot work without",
  DECIDES: "settles", AFFECTS: "constrains", SUPERSEDES: "replaces",
  APPLIES_TO: "applies to", EVIDENCED_BY: "evidence", INVOLVES: "people",
  LIVES_IN: "code lives in", DEPLOYS_TO: "runs at",
  OPENED_IN: "opened in", AUTHORED_BY: "author", REVIEWS: "reviewer",
  TRACKS: "tracks in Linear", SCOPED_TO: "in Linear project", PLANNED_IN: "in cycle",
  ASSIGNED_TO: "assignee", BLOCKS: "blocks", SUBTASK_OF: "subtask of",
  MENTIONS: "untyped legacy link",
};

const pillClass = (text) => {
  const t = String(text).toLowerCase();
  if (/(^|\b)(active|merged|completed|done|accepted|implemented|green|ontrack|started)/.test(t)) return "ok";
  if (/(^|\b)(blocked|red|failing|canceled|cancelled|rejected|closed|atrisk|offtrack)/.test(t)) return "bad";
  if (/(^|\b)(dormant|stale|unknown|proposed|draft|triage|backlog|superseded)/.test(t)) return "warn";
  return "";
};

const EXT = `<svg viewBox="0 0 12 12" width="11" height="11" aria-hidden="true"><path d="M4.5 1.5h6v6M10.5 1.5 5 7M8 9.5v1h-7v-7h1" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

function relRow(item) {
  const link = item.link
    ? `<a class="ext" href="${esc(item.link)}" target="_blank" rel="noopener" title="${esc(item.link)}">${EXT}</a>`
    : "";
  const bits = [];
  if (item.type) bits.push(esc(item.type));
  if (item.state) bits.push(esc(item.state));
  if (item.status) bits.push(esc(item.status));
  if (item.updated) bits.push(esc(item.updated));
  const prov = item.prov && item.prov !== "page"
    ? `<span class="ev" title="${esc(item.evidence || "")}">${esc(item.prov)}${item.conf < 1 ? " " + item.conf.toFixed(2) : ""}</span>`
    : "";
  return `<div class="row${item.untitled ? " untitled" : ""}">${glyph(item.type)}
    <div class="mid">
      <button class="t" data-node="${esc(item.id)}">${esc(item.title)}</button>
      <div class="sub">${bits.map((b) => `<span>${b}</span>`).join("<span>·</span>")}${prov}</div>
    </div>${link}</div>`;
}

function relGroup(group) {
  const arrow = group.dir === "out" ? "→" : "←";
  const doc = RELATION_DOC[group.rel] ? ` · ${RELATION_DOC[group.rel]}` : "";
  return `<div class="rel${group.deprecated ? " deprecated" : ""}">
    <div class="rel-head"><span class="dir">${arrow}</span><span class="verb">${esc(group.rel)}</span>
      <span class="muted">${esc(doc.slice(3))}</span><span class="n">${group.count}</span></div>
    ${group.items.map(relRow).join("")}</div>`;
}

function renderPanel(detail) {
  const n = detail.node;
  const pills = [];
  if (n.status) pills.push(`<span class="pill ${pillClass(n.status)}">${esc(n.status)}</span>`);
  if (n.state && n.state !== n.status) pills.push(`<span class="pill ${pillClass(n.state)}">${esc(n.state)}</span>`);
  if (n.updated) pills.push(`<span class="pill">${esc(n.updated)}</span>`);

  const actions = [];
  if (n.link) actions.push(`<a class="primary" href="${esc(n.link)}" target="_blank" rel="noopener">${EXT} ${esc(linkLabel(n))}</a>`);
  if (n.local) {
    actions.push(`<button data-open="edit" title="${esc(n.local)}">Open locally</button>`);
    actions.push(`<button data-open="reveal">Reveal</button>`);
  }
  actions.push(`<button data-copy="${esc(n.id)}">Copy id</button>`);
  if (n.type === "project" || n.type === "concept" || n.type === "decision") {
    actions.push(`<button data-copy="[[${esc(n.id)}]]">Copy [[link]]</button>`);
  }

  const trail = state.trail.slice(-4, -1).map((id) => esc(id)).join(" › ");

  const head = `<div class="p-head">
    <div class="p-nav">
      <button data-nav="back" title="Back [">‹</button>
      <button data-nav="forward" title="Forward ]">›</button>
      <span class="trail">${trail}</span>
    </div>
    <div class="p-kind"><span class="type-chip">${glyph(n.type, 10)}${esc(n.type)}</span>${pills.join("")}
      <span class="muted tabular">${n.degree} edges</span></div>
    <h1>${esc(n.title)}</h1>
    <div class="p-id mono">${esc(n.id)}</div>
    ${n.summary ? `<p class="p-summary">${inline(n.summary)}</p>` : ""}
    <div class="p-actions">${actions.join("")}</div>
  </div>`;

  const sections = [];
  if (detail.out.length) {
    sections.push(`<section class="p-section"><h2>${esc(n.title)} → ${detail.counts.out} outbound</h2>
      ${detail.out.map(relGroup).join("")}</section>`);
  }
  if (detail.in.length) {
    sections.push(`<section class="p-section"><h2>${detail.counts.in} inbound → ${esc(n.title)}</h2>
      ${detail.in.map(relGroup).join("")}</section>`);
  }
  if (!detail.out.length && !detail.in.length) {
    sections.push(`<section class="p-section muted">Nothing links to this node yet.</section>`);
  }
  if (detail.body) {
    sections.push(`<section class="page-body">${markdown(detail.body)}</section>`);
  }

  const body = $("panel-body");
  body.innerHTML = head + sections.join("");
  body.hidden = false;
  $("panel-empty").hidden = true;
  $("panel").scrollTop = 0;
}

function linkLabel(n) {
  const l = n.link || "";
  if (l.includes("github.com")) return n.type === "pr" ? "Open the PR" : "Open on GitHub";
  if (l.includes("linear.app")) return "Open in Linear";
  if (n.type === "service") return "Open the site";
  return "Open";
}

$("panel").addEventListener("click", async (ev) => {
  const t = ev.target.closest("[data-node],[data-open],[data-copy],[data-nav]");
  if (!t) return;
  if (t.dataset.node) return go(t.dataset.node);
  if (t.dataset.copy) {
    navigator.clipboard.writeText(t.dataset.copy);
    const was = t.textContent; t.textContent = "copied"; setTimeout(() => (t.textContent = was), 900);
    return;
  }
  if (t.dataset.nav) return t.dataset.nav === "back" ? history.back() : history.forward();
  if (t.dataset.open && state.focus) {
    const was = t.textContent;
    try {
      await fetch(`/api/open?id=${encodeURIComponent(state.focus)}&how=${t.dataset.open}`, { method: "POST" });
      t.textContent = "opened";
    } catch { t.textContent = "failed"; }
    setTimeout(() => (t.textContent = was), 1100);
  }
});

/* ---------------------------------------------------------------- markdown */

function inline(text) {
  let out = esc(text);
  out = out.replace(/`([^`]+)`/g, (_, c) => `<code>${c}</code>`);
  out = out.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_, id, label) => {
    const known = state.ids.has(id.trim());
    return `<button class="wikilink${known ? "" : " missing"}"${known ? ` data-node="${esc(id.trim())}"` : ""}>${esc(label || id)}</button>`;
  });
  out = out.replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, (_, t, u) => `<a href="${u}" target="_blank" rel="noopener">${t}</a>`);
  out = out.replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, (_, p, u) => `${p}<a href="${u}" target="_blank" rel="noopener">${u}</a>`);
  out = out.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  out = out.replace(/(^|[^*\w])\*([^*\n]+)\*/g, "$1<em>$2</em>");
  return out;
}

/// A deliberately small block parser: this renders the wiki's own pages, which
/// are bullets, tables, fences and headings, and nothing exotic.
function markdown(src) {
  const lines = src.split("\n");
  const out = [];
  let i = 0;
  const closeList = (stack) => { while (stack.length) out.push(stack.pop()); };
  const listStack = [];

  while (i < lines.length) {
    const line = lines[i];

    if (/^```/.test(line)) {
      const buf = [];
      i++;
      while (i < lines.length && !/^```/.test(lines[i])) buf.push(lines[i++]);
      i++;
      closeList(listStack);
      out.push(`<pre><code>${esc(buf.join("\n"))}</code></pre>`);
      continue;
    }
    if (/^\s*$/.test(line)) { closeList(listStack); i++; continue; }
    if (/^---+\s*$/.test(line)) { closeList(listStack); out.push("<hr>"); i++; continue; }

    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      closeList(listStack);
      const level = Math.min(h[1].length, 3);
      out.push(`<h${level}>${inline(h[2])}</h${level}>`);
      i++;
      continue;
    }

    if (/^\s*\|/.test(line) && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1] || "")) {
      closeList(listStack);
      const cells = (row) => row.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const head = cells(line);
      i += 2;
      const rows = [];
      while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(cells(lines[i++]));
      out.push(`<div class="tablewrap"><table><thead><tr>${head.map((c) => `<th>${inline(c)}</th>`).join("")}</tr></thead>` +
        `<tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`);
      continue;
    }

    if (/^>\s?/.test(line)) {
      closeList(listStack);
      const buf = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) buf.push(lines[i++].replace(/^>\s?/, ""));
      out.push(`<blockquote>${inline(buf.join(" "))}</blockquote>`);
      continue;
    }

    const li = line.match(/^(\s*)([-*+]|\d+[.)])\s+(.*)$/);
    if (li) {
      const depth = Math.floor(li[1].length / 2);
      const ordered = /\d/.test(li[2]);
      while (listStack.length > depth + 1) out.push(listStack.pop());
      if (listStack.length < depth + 1) {
        out.push(ordered ? "<ol>" : "<ul>");
        listStack.push(ordered ? "</ol>" : "</ul>");
      }
      const task = li[3].match(/^\[([ xX])\]\s*(.*)$/);
      if (task) {
        const done = task[1].toLowerCase() === "x";
        out.push(`<li class="task${done ? " done" : ""}"><span class="box">${done ? "☑" : "☐"}</span>${inline(task[2])}</li>`);
      } else {
        out.push(`<li>${inline(li[3])}</li>`);
      }
      i++;
      continue;
    }

    // A wrapped bullet — "  …and the rest of the sentence" — belongs to the
    // item above it, not to a paragraph that closes the list.
    if (listStack.length && /^\s+\S/.test(line) && out.length && out[out.length - 1].endsWith("</li>")) {
      out[out.length - 1] = out[out.length - 1].slice(0, -5) + " " + inline(line.trim()) + "</li>";
      i++;
      continue;
    }

    const buf = [];
    while (i < lines.length && !/^\s*$/.test(lines[i]) && !/^(#{1,6}\s|```|>|\s*\||\s*([-*+]|\d+[.)])\s)/.test(lines[i])) {
      buf.push(lines[i++]);
    }
    if (buf.length) { closeList(listStack); out.push(`<p>${inline(buf.join(" "))}</p>`); }
    else if (i < lines.length && !listStack.length) i++;
  }
  closeList(listStack);
  return out.join("");
}

/* ----------------------------------------------------------------- palette */

const palette = $("palette");
const pInput = $("palette-input");
const pResults = $("palette-results");
let pHits = [];
let pSel = 0;
let pKinds = new Set();
let pTimer = null;

function openPalette(seed) {
  palette.hidden = false;
  if (seed !== undefined) pInput.value = seed;
  pInput.focus();
  pInput.select();
  runSearch();
}
function closePalette() { palette.hidden = true; }

function renderFilters() {
  $("palette-filters").innerHTML = (state.meta?.types || []).map((t) =>
    `<button class="chip" data-kind="${esc(t.t)}" aria-pressed="${pKinds.has(t.t)}">${glyph(t.t, 12)}<b>${esc(t.t)}</b> <span class="n">${t.c}</span></button>`
  ).join("");
}

$("palette-filters").addEventListener("click", (ev) => {
  const b = ev.target.closest("[data-kind]");
  if (!b) return;
  const k = b.dataset.kind;
  pKinds.has(k) ? pKinds.delete(k) : pKinds.add(k);
  renderFilters();
  runSearch();
  pInput.focus();
});

function highlightMatch(text, query) {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  let html = esc(text);
  for (const t of terms) {
    if (t.length < 2) continue;
    const at = html.toLowerCase().indexOf(t);
    if (at < 0 || html.slice(0, at).includes("<")) continue;
    html = html.slice(0, at) + `<span class="hl">` + html.slice(at, at + t.length) + `</span>` + html.slice(at + t.length);
  }
  return html;
}

async function runSearch() {
  const q = pInput.value.trim();
  const mine = ++state.seq;
  let data;
  try {
    data = await api("/api/search", { q, limit: 50, type: [...pKinds].join(",") });
  } catch { return; }
  if (mine !== state.seq) return;
  pHits = data.hits;
  pSel = 0;
  $("palette-count").textContent = data.count ? `${data.count}` : "";
  if (!pHits.length) {
    pResults.innerHTML = `<li class="none">Nothing matches <b>${esc(q)}</b>.</li>`;
    return;
  }
  pResults.innerHTML = pHits.map((h, i) => {
    const bits = [h.type, h.state || h.status, h.updated].filter(Boolean).map(esc).join(" · ");
    return `<li role="option" data-i="${i}" aria-selected="${i === 0}">
      ${glyph(h.type, 13)}
      <div class="mid">
        <span class="t">${highlightMatch(h.title, q)}</span>
        <span class="s mono">${highlightMatch(h.id + (h.summary ? " — " + h.summary : ""), q)}</span>
      </div>
      <span class="acc">${bits}<span class="deg tabular">${h.degree} edges</span></span>
    </li>`;
  }).join("");
  pResults.firstElementChild?.scrollIntoView({ block: "nearest" });
}

function moveSel(delta) {
  if (!pHits.length) return;
  pSel = (pSel + delta + pHits.length) % pHits.length;
  [...pResults.children].forEach((li, i) => li.setAttribute("aria-selected", String(i === pSel)));
  pResults.children[pSel]?.scrollIntoView({ block: "nearest" });
}

pInput.addEventListener("input", () => { clearTimeout(pTimer); pTimer = setTimeout(runSearch, 70); });
pInput.addEventListener("keydown", (ev) => {
  if (ev.key === "ArrowDown") { ev.preventDefault(); moveSel(1); }
  else if (ev.key === "ArrowUp") { ev.preventDefault(); moveSel(-1); }
  else if (ev.key === "Enter") {
    ev.preventDefault();
    const hit = pHits[pSel];
    if (!hit) return;
    if ((ev.metaKey || ev.ctrlKey) && hit.link) window.open(hit.link, "_blank", "noopener");
    else { closePalette(); go(hit.id); }
  } else if (ev.key === "Escape") { ev.preventDefault(); closePalette(); }
});
pResults.addEventListener("click", (ev) => {
  const li = ev.target.closest("[data-i]");
  if (!li) return;
  closePalette();
  go(pHits[Number(li.dataset.i)].id);
});
palette.addEventListener("mousedown", (ev) => { if (ev.target === palette) closePalette(); });

/* ------------------------------------------------------------------- status */

const PRIORITY = ["", "urgent", "high", "medium", "low"];

// "2026-11-17T09:13:21Z" → "today", "3d", "5w"; a bare day when that is all there is.
function ago(stamp) {
  if (!stamp) return "";
  const then = new Date(stamp.length === 10 ? stamp + "T12:00:00Z" : stamp);
  if (isNaN(then)) return stamp;
  const days = Math.floor((Date.now() - then) / 864e5);
  if (days <= 0) return "today";
  if (days < 14) return days + "d";
  if (days < 70) return Math.floor(days / 7) + "w";
  return Math.floor(days / 30) + "mo";
}

const pill = (text, cls) => text ? `<span class="pill ${cls ?? pillClass(text)}">${esc(text)}</span>` : "";

function boardRow(row, kind) {
  const known = state.ids.has(row.id);
  const pills = kind === "pr"
    ? [row.draft ? pill("draft") : "", pill(row.review, row.review === "approved" ? "ok" : "bad"),
       row.checks === "red" ? `<span class="pill bad" title="${esc((row.failing || []).join(", "))}">red</span>` : ""]
    : [pill(row.state), pill(PRIORITY[row.priority], row.priority === 1 ? "bad" : row.priority === 2 ? "warn" : ""),
       row.blocked_by?.length ? `<span class="pill bad" title="${esc(row.blocked_by.join(", "))}">blocked</span>` : "",
       row.prs?.length ? `<span class="pill">${row.prs.length} PR${row.prs.length > 1 ? "s" : ""}</span>` : ""];
  const where = kind === "pr" ? row.slug : [row.id, row.project].filter(Boolean).join(" · ");
  const when = row.created ? `opened ${ago(row.created)}` : `updated ${ago(row.updated)}`;
  return `<li class="b-row">
    <a class="b-title" href="${esc(row.url)}" target="_blank" rel="noopener">${esc(row.title)}</a>
    <div class="b-sub"><span class="mono">${esc(where)}</span><span title="${esc(row.created || row.updated)}">${esc(when)}</span>${pills.join("")}
      ${known ? `<button class="b-graph" data-node="${esc(row.id)}" title="Show in the graph">graph</button>` : ""}</div>
  </li>`;
}

function boardList(title, hint, list, kind) {
  const more = list.total > list.rows.length ? ` of ${list.total}` : "";
  const body = list.rows.length
    ? `<ul>${list.rows.map((r) => boardRow(r, kind)).join("")}</ul>`
    : `<p class="muted">Nothing here.</p>`;
  return `<section class="b-col"><h2>${esc(title)} <span class="muted tabular">${list.rows.length}${more}</span></h2>
    <p class="muted b-hint">${esc(hint)}</p>${body}</section>`;
}

async function showBoard() {
  const board = $("board");
  let data;
  try {
    data = await api("/api/status");
  } catch (e) {
    board.innerHTML = `<p class="muted b-empty">${esc(e.message)}</p>`;
    return;
  }
  const linear = data.linear
    ? boardList("Working on", "Linear issues assigned to me, in a started state", data.doing, "issue") +
      boardList("Pick up next", "assigned to me, not started — newest first", data.next, "issue")
    : `<section class="b-col"><h2>Linear</h2><p class="muted">Not watched. Add a <code>linear:</code> key to <code>os.yaml</code>.</p></section>`;
  board.innerHTML = `<div class="b-head"><h1>status</h1>
      <span class="muted">${esc(data.login)} · polled <span title="${esc(data.taken_at)}">${esc(ago(data.taken_at))}</span>
      ${data.taken_at ? "at " + esc(new Date(data.taken_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })) : ""}</span></div>
    <div class="b-grid">
      ${boardList("Latest PRs I opened", "open, newest first", data.opened, "pr")}
      ${boardList("Waiting on my review", "open, review requested of me", data.review, "pr")}
      ${linear}
    </div>`;
}

$("board").addEventListener("click", (ev) => {
  const b = ev.target.closest("[data-node]");
  if (b) go(b.dataset.node);
});

/* ------------------------------------------------------------------ routing */

function go(id) {
  const next = "#/n/" + encodeURIComponent(id);
  if (location.hash === next) return route();
  location.hash = next;
}

async function route() {
  const onBoard = location.hash === "#/status";
  $("board").hidden = !onBoard;
  $("open-board").setAttribute("aria-current", String(onBoard));
  if (onBoard) return showBoard();
  const m = location.hash.match(/^#\/n\/(.+)$/);
  if (!m) {
    state.focus = null;
    $("panel-body").hidden = true;
    $("panel-empty").hidden = false;
    await showAll();
    return;
  }
  const id = decodeURIComponent(m[1]);
  state.focus = id;
  if (state.trail[state.trail.length - 1] !== id) state.trail.push(id);
  $("empty").hidden = true;

  const detail = api("/api/node", { id });
  const scene = state.mode === "focus"
    ? api("/api/subgraph", { id, depth: state.depth })
    : Promise.resolve(null);

  try {
    state.detail = await detail;
    renderPanel(state.detail);
  } catch (e) {
    $("panel-body").innerHTML = `<div class="p-head"><h1>${esc(id)}</h1><p class="muted">${esc(e.message)}</p></div>`;
    $("panel-body").hidden = false;
    $("panel-empty").hidden = true;
    return;
  }

  const s = await scene;
  if (s) {
    renderScene(s, id);
    fitVisible();
  } else {
    await showAll();
  }
}

function centreOn(id) {
  const i = state.scene?.nodes.findIndex((n) => n.id === id);
  if (i === undefined || i < 0) return;
  const n = state.scene.nodes[i];
  const r = canvas.getBoundingClientRect();
  state.view.k = Math.max(state.view.k, 1.1);
  state.view.x = r.width / 2 - n.x * state.view.k;
  state.view.y = r.height / 2 - n.y * state.view.k;
  applyView();
}

let wholeScene = null;
async function showAll() {
  $("empty").hidden = state.mode === "all" || !!state.focus;
  if (state.mode !== "all" && !state.focus) {
    $("status").textContent = "";
    return;
  }
  if (!wholeScene) {
    $("status").textContent = "laying out the whole graph…";
    wholeScene = await api("/api/scene");
  }
  renderScene(wholeScene, state.focus);
  fitVisible();
}

/* ------------------------------------------------------------------- chrome */

function renderLegend() {
  const legend = $("legend");
  legend.replaceChildren();
  for (const w of state.meta.worlds) {
    const label = document.createElement("span");
    label.className = "world";
    label.textContent = w.label;
    legend.appendChild(label);
    state.meta.types.filter((t) => t.w === w.id).sort((a, b) => b.c - a.c).forEach((t) => {
      const chip = document.createElement("button");
      chip.className = "chip";
      chip.dataset.type = t.t;
      chip.setAttribute("aria-pressed", "true");
      chip.innerHTML = `${glyph(t.t, 14)}<b>${esc(t.t)}</b> <span class="n">${t.c}</span>`;
      chip.onclick = () => {
        const on = chip.getAttribute("aria-pressed") === "true";
        chip.setAttribute("aria-pressed", String(!on));
        on ? state.hidden.add(t.t) : state.hidden.delete(t.t);
        refreshVisibility();
      };
      legend.appendChild(chip);
    });
  }
}

$("mode").addEventListener("click", async (ev) => {
  const b = ev.target.closest("[data-mode]");
  if (!b) return;
  state.mode = b.dataset.mode;
  $("stage").classList.toggle("all", state.mode === "all");
  [...ev.currentTarget.children].forEach((c) => c.setAttribute("aria-pressed", String(c === b)));
  $("depth").style.opacity = state.mode === "focus" ? 1 : 0.4;
  if (state.mode === "all") await showAll();
  else if (state.focus) await route();
  else { state.scene = null; renderScene({ nodes: [], edges: [] }, null); $("empty").hidden = false; }
});

$("depth").addEventListener("click", (ev) => {
  const b = ev.target.closest("[data-depth]");
  if (!b) return;
  state.depth = Number(b.dataset.depth);
  [...ev.currentTarget.children].forEach((c) => c.setAttribute("aria-pressed", String(c === b)));
  if (state.mode === "focus" && state.focus) route();
});

$("open-search").onclick = () => openPalette("");
$("brand").onclick = () => { location.hash = ""; };
$("help-btn").onclick = () => { $("help").hidden = false; };
$("help").addEventListener("mousedown", (ev) => { if (ev.target === $("help")) $("help").hidden = true; });
$("theme").onclick = () => {
  const root = document.documentElement;
  const dark = root.getAttribute("data-theme") === "dark" ||
    (!root.hasAttribute("data-theme") && matchMedia("(prefers-color-scheme: dark)").matches);
  root.setAttribute("data-theme", dark ? "light" : "dark");
  localStorage.setItem("ikusi-theme", dark ? "light" : "dark");
};

document.addEventListener("keydown", (ev) => {
  const typing = ev.target.tagName === "INPUT" || ev.target.tagName === "TEXTAREA";
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "k") { ev.preventDefault(); return openPalette(); }
  if (typing) return;
  if (ev.key === "Escape") { closePalette(); $("help").hidden = true; return; }
  if (ev.key === "/") { ev.preventDefault(); return openPalette(""); }
  if (ev.key === "?") { $("help").hidden = false; return; }
  if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "e" && state.focus) {
    ev.preventDefault();
    fetch(`/api/open?id=${encodeURIComponent(state.focus)}&how=edit`, { method: "POST" });
    return;
  }
  if ((ev.metaKey || ev.ctrlKey) && ev.key === "Enter" && state.detail?.node.link) {
    window.open(state.detail.node.link, "_blank", "noopener");
    return;
  }
  if (ev.key === "[") return history.back();
  if (ev.key === "]") return history.forward();
  if (ev.key === "s") { location.hash = location.hash === "#/status" ? "" : "#/status"; return; }
  if (ev.key === "f") return fitVisible();
  if (ev.key === "g") return $("mode").querySelector(`[data-mode="${state.mode === "all" ? "focus" : "all"}"]`).click();
  if (["1", "2", "3"].includes(ev.key)) return $("depth").querySelector(`[data-depth="${ev.key}"]`)?.click();
});

window.addEventListener("hashchange", route);
window.addEventListener("resize", () => { if (state.scene) fitVisible(); });

/* --------------------------------------------------------------------- boot */

(async function boot() {
  const saved = localStorage.getItem("ikusi-theme");
  if (saved) document.documentElement.setAttribute("data-theme", saved);

  state.meta = await api("/api/meta");
  state.meta.types.forEach((t) => { state.shapeOf[t.t] = t.s; state.worldOf[t.t] = t.w; });
  $("totals").textContent = `${state.meta.nodes} nodes · ${state.meta.edges} edges`;
  document.title = `ikusi — ${state.meta.nodes} nodes`;
  renderLegend();
  renderFilters();
  api("/api/ids").then((d) => { state.ids = new Set(d.ids); }).catch(() => {});

  await route();
  if (!state.focus && location.hash !== "#/status") openPalette("");

  // The server re-reads the TSVs when they change; poll the load stamp so a
  // `sare build` in another window lands here instead of going stale.
  setInterval(async () => {
    try {
      if (location.hash === "#/status") showBoard();
      const m = await api("/api/meta");
      if (m.loaded_at !== state.meta.loaded_at) {
        state.meta = m;
        wholeScene = null;
        $("totals").textContent = `${m.nodes} nodes · ${m.edges} edges`;
        renderLegend();
        renderFilters();
        route();
      }
    } catch {}
  }, 8000);
})();
