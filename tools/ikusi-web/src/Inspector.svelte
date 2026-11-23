<script lang="ts">
  import { Badge, Button, Empty, Menu, MenuItem, Prose, Segmented, copy, task } from '@enekos/osagai';
  import { api, type Detail, type NodeView } from './api';
  import { external, date, collection, directoryHref } from './navigation';
  import { inline, markdown } from './markdown';
  import Kind from './Kind.svelte';
  import NeighborGroup from './NeighborGroup.svelte';
  import type { Snippet } from 'svelte';
  import PullRequestView from './PullRequestView.svelte';
  let {
    detail,
    ids,
    view,
    onview,
    graph,
  }: {
    detail: Detail;
    ids: Set<string> | undefined;
    view: NodeView;
    onview: (view: NodeView) => void;
    graph: Snippet;
  } = $props();
  const n = $derived(detail.node);
  const href = $derived(external(n.link));
  const open = task((how: 'edit' | 'reveal') => api.open(n.id, how), { success: 'Opened locally' });
  export function command(edit: boolean) {
    if (edit && n.local) void open('edit');
    if (!edit && href) window.open(href, '_blank', 'noopener');
  }
</script>

<article id="node-details" tabindex="-1" class="panel inspector">
  <header class="inspect-head">
    <div class="node-toolbar">
      <div class="node-meta">
        <Kind type={n.type} small /><a href={directoryHref(n.type)}>{collection(n.type)}</a>
        <span aria-hidden="true">/</span><code class="id-text">{n.id}</code>
      </div>
      <div class="node-actions">
        {#if href}<Button {href} target="_blank" rel="noopener" icon="external" size="sm">
            {n.type === 'pr' ? 'Open on GitHub' : 'Open source'}
          </Button>{/if}
        {#if n.local}<Button size="sm" icon="edit" loading={open.pending} onclick={() => command(true)}>
            Open locally
          </Button>{/if}
        <Menu label="Node actions">
          <MenuItem icon="copy" onclick={() => copy(n.id, { toast: 'Node ID copied' })}>Copy ID</MenuItem>
          {#if ['project', 'decision', 'concept'].includes(n.type)}
            <MenuItem icon="link" onclick={() => copy('[[' + n.id + ']]', { toast: 'Wiki link copied' })}>
              Copy wiki link
            </MenuItem>
          {/if}
          {#if n.local}<MenuItem icon="folder" onclick={() => open('reveal')}>Reveal file</MenuItem>{/if}
        </Menu>
      </div>
    </div>
    <h1>{n.title}</h1>
    {#if n.summary && n.type !== 'pr'}<div class="summary">{@html inline(n.summary, ids)}</div>{/if}
    {#if n.type !== 'pr' && (n.updated || n.status || n.state)}
      <div class="node-meta node-stamps">
        {#if n.status}<Badge>{n.status}</Badge>{/if}
        {#if n.state && n.state !== n.status}<Badge>{n.state}</Badge>{/if}
        {#if n.updated}<small>Updated {date(n.updated)}</small>{/if}
      </div>
    {/if}
  </header>
  <div class="inspect-tabs">
    <Segmented
      value={view}
      onchange={onview}
      label="Node view"
      options={[
        ...(n.type === 'pr' ? [{ value: 'pr' as const, label: 'Pull request' }] : []),
        ...(detail.body ? [{ value: 'page' as const, label: 'Wiki page' }] : []),
        { value: 'connections', label: 'Connections (' + n.degree + ')' },
        { value: 'map', label: 'Neighborhood' },
      ]}
    />
  </div>
  {#if view === 'page' && detail.body}
    <div class="wiki-body"><Prose size="lg" html={markdown(detail.body, ids, n.title)} /></div>
  {:else if view === 'pr'}<PullRequestView node={n} />
  {:else if view === 'map'}{@render graph()}
  {:else}
    {#each ['out', 'in'] as dir}
      {@const groups = dir === 'out' ? detail.out : detail.in}
      {#if groups.length}<section class="relations">
          <h3>
            {dir === 'out' ? 'Outgoing' : 'Incoming'}
            <span>{dir === 'out' ? detail.counts.out : detail.counts.in}</span>
          </h3>
          {#each groups as group (group.rel)}
            <NeighborGroup id={n.id} {group} expanded={group === groups[0]} />
          {/each}
        </section>{/if}
    {/each}
    {#if !n.degree}<Empty
        compact
        icon="network"
        title="No connections yet"
        text="Connections to this node will appear here."
      />{/if}
  {/if}
</article>
