<script lang="ts">
  import { Button, Icon, QueryError, query } from '@enekos/osagai';
  import { api, type Scene, type GraphNode } from './api';
  import { colorOf, nodeHref } from './navigation';
  let { node, version }: { node: GraphNode; version: string } = $props();
  const detail = query(() => {
    void version;
    return api.spotlight(node.id);
  });
  function neighbors(data: Scene) {
    const center = data.nodes.findIndex((p) => p.id === node.id);
    const candidates = data.edges
      .filter((e) => e.s === center || e.t === center)
      .map((e) => ({ item: data.nodes[e.s === center ? e.t : e.s], rel: e.r }));
    const picks: typeof candidates = [];
    const seen = new Set([node.id]);
    // Show a mix of types before filling the remaining places with other neighbors.
    for (const distinct of [true, false]) {
      for (const entry of candidates) {
        if (seen.has(entry.item.id) || (distinct && picks.some((p) => p.item.t === entry.item.t))) continue;
        seen.add(entry.item.id);
        picks.push(entry);
        if (picks.length === 5) return picks;
      }
    }
    return picks;
  }
  const positions = [
    [25, 25],
    [75, 20],
    [84, 60],
    [58, 83],
    [19, 75],
  ];
  const picks = $derived(detail.data ? neighbors(detail.data) : []);
</script>

<section class="spotlight" aria-label="Neighborhood spotlight">
  <div class="spot-copy">
    <div class="spot-kicker"><Icon name="compass" size={16} /> Neighborhood spotlight</div>
    <h2>Explore your graph</h2>
    <p>Start with <strong>{node.title}</strong> and follow its connections across the workspace.</p>
    <Button href={nodeHref(node.id)} class="spot-button" icon="network" flat>Open neighborhood</Button>
  </div>
  <div class="constellation" aria-label={'Connections to ' + node.title}>
    <svg class="orbits" viewBox="0 0 400 224" preserveAspectRatio="none" aria-hidden="true">
      <g fill="none" stroke="currentColor">
        <ellipse cx="201" cy="109" rx="79" ry="70" />
        <ellipse cx="201" cy="109" rx="139" ry="116" stroke-dasharray="3 5" />
        <ellipse cx="201" cy="109" rx="208" ry="168" />
      </g>
      {#each picks as pick, i (pick.item.id)}
        <path
          d={'M200 108 Q' +
            positions[i][0] * 4 +
            ' 108 ' +
            positions[i][0] * 4 +
            ' ' +
            positions[i][1] * 2.24}
          class="orbit-link"
        />
      {/each}
    </svg>
    {#if detail.error}<div class="spot-error"><QueryError query={detail} compact /></div>{/if}
    <a class="orbit-node center" style="left:50%;top:48%" href={nodeHref(node.id)} title={node.title}>
      <i></i><span>{node.title}</span>
    </a>
    {#each picks as pick, i (pick.item.id)}
      <a
        class={'orbit-node sat ' + colorOf(pick.item.t)}
        style:left={positions[i][0] + '%'}
        style:top={positions[i][1] + '%'}
        title={pick.rel + ' · ' + pick.item.n}
        href={nodeHref(pick.item.id)}
      >
        <i></i><span>{pick.item.n}</span>
      </a>
    {/each}
  </div>
</section>
