<script lang="ts">
  import { Button, Empty, Icon, QueryError, query } from '@enekos/osagai';
  import { api, type Meta } from './api';
  import { collection, directoryHref, number } from './navigation';
  import Kind from './Kind.svelte';
  import NodeList from './NodeList.svelte';
  import Spotlight from './Spotlight.svelte';
  let { meta }: { meta: Meta } = $props();
  const starts = query(() => {
    void meta.revision;
    return api.search('', '', 6);
  });
  const types = $derived([...meta.types].sort((a, b) => b.c - a.c).slice(0, 5));
  const anchor = $derived(starts.data?.hits.find((n) => n.degree > 0) ?? starts.data?.hits[0]);
</script>

<header class="page-intro">
  <h1>Workspace overview</h1>
  <p>Find a starting point and explore how your knowledge connects.</p>
</header>
{#if !meta.nodes}
  <section class="panel">
    <Empty
      icon="network"
      title="Your graph is ready to grow"
      text="Build your wiki with sare build pages to see its nodes and connections here."
    />
  </section>
{:else if anchor}
  <Spotlight node={anchor} version={meta.revision} />
{:else if starts.error}<QueryError query={starts} />{:else}<div class="surface-loading">
    Loading your neighborhood…
  </div>{/if}
<div class="metrics">
  {#each [{ label: 'Total nodes', value: meta.nodes, icon: 'layers', tone: 'mint' }, { label: 'Connections', value: meta.edges, icon: 'network', tone: 'violet' }, { label: 'Node types', value: meta.types.length, icon: 'overview', tone: 'amber' }] as stat}
    <div class="metric">
      <span class={'metric-icon ' + stat.tone}><Icon name={stat.icon} size={22} /></span>
      <div><span>{stat.label}</span><strong>{number(stat.value)}</strong></div>
    </div>
  {/each}
</div>
<div class="home-columns">
  <section class="panel">
    <div class="panel-heading">
      <h2>Starting points</h2>
      <Button href="#/nodes" variant="ghost" size="sm">Browse nodes</Button>
    </div>
    {#if starts.error}<QueryError query={starts} />{:else if starts.data?.hits.length}<NodeList
        nodes={starts.data.hits}
      />
    {:else if !starts.loaded}<p class="message">Loading nodes…</p>{:else}<p class="message">
        Nodes will appear after your first build.
      </p>{/if}
  </section>
  <section class="panel">
    <div class="panel-heading">
      <h2>Largest collections</h2>
      <Icon name="layers" size={18} />
    </div>
    {#if types.length}
      <ul class="collection-list">
        {#each types as kind (kind.t)}
          <li>
            <a href={directoryHref(kind.t)}>
              <Kind type={kind.t} small /><span>{collection(kind.t)}</span><strong>{number(kind.c)}</strong>
            </a>
          </li>
        {/each}
      </ul>
    {:else}<p class="message">Your collections will appear here.</p>{/if}
  </section>
</div>
