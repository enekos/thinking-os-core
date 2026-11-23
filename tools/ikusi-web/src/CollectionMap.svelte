<script lang="ts">
  import { Icon } from '@enekos/osagai';
  import type { Meta } from './api';
  import { collection, directoryHref, iconOf, number } from './navigation';

  let { meta, hidden }: { meta: Meta; hidden: Set<string> } = $props();
  let width = $state(0);
  let hovered = $state(''),
    focused = $state('');
  const active = $derived(focused || hovered);
  const kinds = $derived(meta.types.filter((kind) => !hidden.has(kind.t)));
  const links = $derived(meta.type_links.filter((e) => !hidden.has(e.source) && !hidden.has(e.target)));
  const cols = $derived(Math.max(2, Math.min(5, Math.floor(width / 180))));
  const rows = $derived(Math.ceil(kinds.length / cols));
  const height = $derived(Math.max(260, rows * (width < 520 ? 76 : 102) + 56));

  // Keep the strongest connections near each other in reading order. Fixed slots
  // leave room for labels on a phone as well as on a wide canvas.
  const map = $derived.by(() => {
    const weights = new Map<string, Map<string, number>>();
    for (const kind of kinds) weights.set(kind.t, new Map());
    for (const link of links) {
      if (link.source === link.target) continue;
      const out = weights.get(link.source)!,
        back = weights.get(link.target)!;
      const count = (out.get(link.target) ?? 0) + link.count;
      out.set(link.target, count);
      back.set(link.source, count);
    }
    const remaining = [...kinds];
    const ordered = [];
    while (remaining.length) {
      const prev = ordered.at(-1);
      const neighbors = prev && weights.get(prev.t);
      remaining.sort(
        (a, b) =>
          (neighbors?.get(b.t) ?? 0) - (neighbors?.get(a.t) ?? 0) || b.c - a.c || a.t.localeCompare(b.t),
      );
      ordered.push(remaining.shift()!);
    }
    const nodes = ordered.map((kind, i) => {
      const row = Math.floor(i / cols);
      const rowSize = Math.min(cols, ordered.length - row * cols);
      const col = row % 2 ? rowSize - 1 - (i % cols) : i % cols;
      return {
        ...kind,
        x: ((col + 0.5 + (cols - rowSize) / 2) * width) / cols,
        y: ((row + 0.5) * (height - 44)) / rows + 22,
      };
    });
    const edges = [];
    for (const [i, a] of nodes.entries()) {
      for (const b of nodes.slice(i + 1)) {
        const count = weights.get(a.t)?.get(b.t);
        if (count) edges.push({ a, b, count });
      }
    }
    edges.sort((a, b) => b.count - a.count);
    // Include each collection's strongest neighbor in the resting view.
    const connected = new Set<string>();
    const backbone = new Set(
      edges.filter(({ a, b }) => {
        const add = !connected.has(a.t) || !connected.has(b.t);
        connected.add(a.t);
        connected.add(b.t);
        return add;
      }),
    );
    return { nodes, edges, backbone };
  });
  const selected = $derived(map.nodes.find((node) => node.t === active));
  const related = $derived(
    new Set(
      links.filter((e) => e.source === active || e.target === active).flatMap((e) => [e.source, e.target]),
    ),
  );
  const connections = $derived(
    links.reduce((sum, e) => sum + (e.source === active || e.target === active ? e.count : 0), 0),
  );
</script>

<div
  class="collection-map"
  bind:clientWidth={width}
  style:height={height + 'px'}
  role="group"
  aria-label="Connections between collections"
>
  <svg width="100%" height="100%" aria-hidden="true">
    {#each map.edges as edge}
      {@const { a, b } = edge}
      {@const lit = a.t === active || b.t === active}
      {#if active ? lit : map.backbone.has(edge)}
        <path
          d={'M' +
            a.x +
            ' ' +
            a.y +
            ' Q' +
            ((a.x + b.x) / 2 + (b.y - a.y) * 0.18) +
            ' ' +
            ((a.y + b.y) / 2 - Math.min(88, Math.abs(a.x - b.x) * 0.18)) +
            ', ' +
            b.x +
            ' ' +
            b.y}
          class:lit
          stroke-width={Math.min(4, 1 + Math.log2(edge.count + 1) * 0.23)}
        />
      {/if}
    {/each}
  </svg>
  {#each map.nodes as node (node.t)}
    <a
      class="collection-node"
      class:wiki={node.w === 'wiki'}
      class:dimmed={!!active && node.t !== active && !related.has(node.t)}
      style:left={node.x + 'px'}
      style:top={node.y + 'px'}
      style:max-width={Math.max(88, width / cols - 14) + 'px'}
      href={directoryHref(node.t)}
      aria-label={'Browse ' + collection(node.t) + ' (' + number(node.c) + ' nodes)'}
      onpointerenter={() => (hovered = node.t)}
      onpointerleave={() => (hovered = '')}
      onfocus={() => (focused = node.t)}
      onblur={() => (focused = '')}
    >
      <i><Icon name={iconOf(node.t)} size={18} /></i><span>
        <strong>{collection(node.t)}</strong><small>{number(node.c)} nodes</small>
      </span>
    </a>
  {/each}
</div>
<div class="collection-hint">
  {#if selected}<strong>{collection(selected.t)}</strong> · {number(connections)} connections within visible collections
  {:else}Select a collection to browse its nodes. Hover or focus one to see its connections.{/if}
</div>

<style>
  .collection-map {
    position: relative;
    isolation: isolate;
    background-color: var(--plot-bg);
    background-image: radial-gradient(var(--dots) 0.8px, transparent 0.8px);
    background-size: 18px 18px;
    overflow: hidden;
  }
  svg {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  path {
    fill: none;
    stroke: var(--graph-edge);
    opacity: 0.44;
  }
  path.lit {
    stroke: var(--o-accent);
    opacity: 0.8;
  }
  .collection-node {
    position: absolute;
    transform: translate(-50%, -50%);
    width: max-content;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 10px 7px 7px;
    min-height: 48px;
    text-decoration: none;
    color: var(--o-text);
    background: var(--o-surface);
    border: 1px solid var(--o-border-strong);
    border-radius: 10px;
    box-shadow: 0 2px 5px #142e1e0a;
  }
  .collection-node:hover,
  .collection-node:focus-visible {
    border-color: var(--o-accent);
  }
  .collection-node.dimmed {
    opacity: 0.55;
  }
  .collection-node i {
    flex: none;
    display: grid;
    place-items: center;
    width: 31px;
    height: 31px;
    border-radius: 8px;
    color: var(--violet-ink);
    background: var(--violet-bg);
  }
  .collection-node.wiki i {
    color: var(--mint-ink);
    background: var(--mint-bg);
  }
  .collection-node span {
    min-width: 0;
  }
  strong,
  small {
    display: block;
  }
  strong {
    font-size: 0.875rem;
    line-height: 1.2;
  }
  small {
    font-size: 0.75rem;
    white-space: nowrap;
  }
  .collection-hint {
    padding: 8px 14px;
    font-size: 0.8rem;
    color: var(--o-text-2);
  }
  .collection-hint strong {
    display: inline;
  }
</style>
