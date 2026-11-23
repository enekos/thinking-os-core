<script lang="ts">
  import { Button, Checkbox, Empty, Modal, QueryError, Segmented, query } from '@enekos/osagai';
  import { api, type Meta } from './api';
  import { collection, nodeHref, number } from './navigation';
  import CollectionMap from './CollectionMap.svelte';
  import GraphCanvas from './GraphCanvas.svelte';
  import Inspector from './Inspector.svelte';
  let {
    id,
    meta,
    ids,
    theme,
    search,
    depth = $bindable(1),
  }: {
    id: string;
    meta: Meta;
    ids: Set<string> | undefined;
    theme: string;
    search: () => void;
    depth: number;
  } = $props();
  let lastFocus = $state('');
  let filters = $state(false);
  let hidden = $state(new Set<string>());
  let plot: GraphCanvas | undefined = $state();
  let inspector: Inspector | undefined = $state();
  $effect(() => {
    if (id) lastFocus = id;
  });
  let preferredView = $state<'collections' | 'nodes'>();
  const mapView = $derived(
    preferredView ?? (meta.nodes > 256 || meta.edges > 1000 ? 'collections' : 'nodes'),
  );
  const grouped = $derived(!id && mapView === 'collections');
  const scene = query(
    () => {
      return api.scene(id, id ? depth : 0, meta.revision);
    },
    { enabled: () => !grouped },
  );
  const detail = query(
    () => {
      void meta.revision;
      return api.node(id);
    },
    { enabled: () => !!id },
  );
  const shown = $derived(
    grouped
      ? meta.types.reduce((sum, t) => sum + (hidden.has(t.t) ? 0 : t.c), 0)
      : (scene.data?.nodes.filter((n) => !hidden.has(n.t)).length ?? 0),
  );
  const shownEdges = $derived.by(() => {
    if (grouped)
      return meta.type_links.reduce(
        (sum, e) => sum + (hidden.has(e.source) || hidden.has(e.target) ? 0 : e.count),
        0,
      );
    const data = scene.data;
    return (
      data?.edges.filter((e) => !hidden.has(data.nodes[e.s].t) && !hidden.has(data.nodes[e.t].t)).length ?? 0
    );
  });
  const title = $derived(id ? 'Neighborhood' : 'Whole graph');
  export function key(key: string) {
    if (key === 'f') plot?.recenter();
    if (key === 'edit' || key === 'source') inspector?.command(key === 'edit');
    if (key === 'g') {
      if (id) location.hash = '#/graph';
      else if (lastFocus) location.hash = nodeHref(lastFocus);
      else search();
    }
    if (id && ['1', '2', '3'].includes(key)) depth = Number(key);
  }
</script>

<header class="page-intro">
  <h1>Graph explorer</h1>
  <p>
    {id
      ? 'Follow the connections around a selected node.'
      : 'Explore connections across the workspace or open an individual node.'}
  </p>
</header>
<div class="explore-grid" class:focused={!!id}>
  <section class="panel graph-card">
    <div class="panel-heading">
      <h2>{title}</h2>
      <Button size="sm" icon="filter" onclick={() => (filters = true)}>
        Types{hidden.size ? ' (' + hidden.size + ' hidden)' : ''}
      </Button>
    </div>
    <div class="map-tools">
      {#if id}<Segmented
          label="Neighborhood depth"
          size="sm"
          bind:value={depth}
          options={[
            { value: 1, label: '1 hop' },
            { value: 2, label: '2 hops' },
            { value: 3, label: '3 hops' },
          ]}
        />
        <Button size="sm" variant="ghost" href="#/graph">Whole graph</Button>
        <Button
          size="sm"
          class="jump-details"
          onclick={() => document.getElementById('node-details')?.focus()}
        >
          Node details
        </Button>
      {:else}<Segmented
          label="Whole graph view"
          size="sm"
          value={mapView}
          options={[
            { value: 'collections', label: 'Collections' },
            { value: 'nodes', label: 'Individual nodes' },
          ]}
          onchange={(value: 'collections' | 'nodes') => (preferredView = value)}
        />
        <Button size="sm" icon="search" variant="ghost" onclick={search}>Find a node</Button>{/if}
    </div>
    {#if grouped && shown}<CollectionMap {meta} {hidden} />
    {:else if grouped && !meta.nodes}<Empty
        icon="network"
        title="The graph is empty"
        text="Build the wiki with sare build pages to begin."
      />
    {:else if grouped}<Empty icon="filter" title="All visible types are hidden">
        <Button onclick={() => (hidden = new Set())}>Show all types</Button>
      </Empty>
    {:else if scene.error}<QueryError query={scene} title="Could not open this graph" />
    {:else if !scene.data || scene.pending}<div class="surface-loading">Drawing the graph…</div>
    {:else if !scene.data.nodes.length}<Empty
        icon="network"
        title="The graph is empty"
        text="Build the wiki with sare build pages to begin."
      />
    {:else if !shown}<Empty icon="filter" title="All visible types are hidden">
        <Button onclick={() => (hidden = new Set())}>Show all types</Button>
      </Empty>
    {:else}<GraphCanvas bind:this={plot} scene={scene.data} {meta} focus={id} {hidden} {theme} />{/if}
    <div class="graph-caption">
      <span><i class="wiki-dot"></i>Wiki</span><span>
        <i class="code-dot"></i>Code &amp; runtime
      </span>{#if grouped || (scene.data && !scene.pending && !scene.error)}<small>
          {number(shown)} nodes · {number(shownEdges)} connections
        </small>{/if}
    </div>
    {#if !grouped && !scene.pending && !scene.error && (scene.data?.held || scene.data?.truncated)}<small
        class="graph-note"
      >
        {scene.data.held ? scene.data.held + ' high-degree hubs left unexpanded. ' : ''}{scene.data.truncated
          ? 'Neighborhood size limit reached.'
          : ''}
      </small>{/if}
  </section>
  {#if id}
    {#if detail.error}<QueryError query={detail} title="Could not open this node" />
    {:else if detail.data?.node.id === id}{#key id}<Inspector
          bind:this={inspector}
          detail={detail.data}
          {ids}
        />{/key}
    {:else}<div class="panel message">Loading node details…</div>{/if}
  {/if}
</div>
<Modal bind:open={filters} title="Visible node types">
  {#each ['wiki', 'code'] as world}
    <div class="filter-group">
      <h3>{world === 'wiki' ? 'Wiki' : 'Code & runtime'}</h3>
      {#each meta.types.filter((t) => t.w === world) as kind (kind.t)}
        <Checkbox
          checked={!hidden.has(kind.t)}
          label={collection(kind.t) + ' (' + number(kind.c) + ')'}
          onchange={() => {
            const next = new Set(hidden);
            if (next.has(kind.t)) next.delete(kind.t);
            else next.add(kind.t);
            hidden = next;
          }}
        />
      {/each}
    </div>
  {/each}
  {#snippet footer()}<Button onclick={() => (hidden = new Set())}>Show all types</Button><Button
      variant="primary"
      onclick={() => (filters = false)}
    >
      Done
    </Button>{/snippet}
</Modal>
