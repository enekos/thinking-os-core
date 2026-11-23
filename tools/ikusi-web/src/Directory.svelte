<script lang="ts">
  import { untrack } from 'svelte';
  import { Button, Empty, Icon, Input, QueryError, Select, query } from '@enekos/osagai';
  import { api, type Meta } from './api';
  import { collection, directoryHref, number } from './navigation';
  import NodeList from './NodeList.svelte';
  let {
    type,
    meta,
    initialTerm,
    initialPage,
  }: { type: string; meta: Meta; initialTerm: string; initialPage: number } = $props();
  const size = 30;
  let term = $state(untrack(() => initialTerm));
  let offset = $state(untrack(() => (initialPage - 1) * size));
  const options = $derived([
    { label: 'All types', value: '' },
    ...meta.types.map((t) => ({ label: collection(t.t), value: t.t })),
  ]);
  const result = query(() => {
    void meta.loaded_at;
    const start = offset;
    return api.search(term, type, size, start).then((page) => ({ ...page, start }));
  });
  $effect(() => {
    const href = directoryHref(type, term, 1 + offset / size);
    history.replaceState(history.state, '', href);
  });
  $effect(() => {
    if (result.pending || !result.data) return;
    const last = Math.max(0, Math.ceil(result.data.total / size) - 1) * size;
    if (offset > last) offset = last;
  });
</script>

<header class="page-intro">
  <h1>{type ? collection(type) : 'Node directory'}</h1>
  <p>Find nodes by title, ID, or a phrase from their summary.</p>
</header>
<section class="panel directory">
  <div class="dir-tools">
    <div class="field-with-icon">
      <Icon name="search" size={17} /><Input
        type="search"
        aria-label="Search nodes"
        placeholder="Search nodes…"
        bind:value={term}
        oninput={() => (offset = 0)}
      />
    </div>
    <Select
      value={type}
      {options}
      aria-label="Filter by type"
      onchange={(e) => (location.hash = directoryHref(e.currentTarget.value, term))}
    />
  </div>
  {#if result.error}<QueryError query={result} />{:else if !result.data}<p class="message">Loading nodes…</p>
  {:else if !result.data.hits.length}<Empty
      icon="search"
      title="No matching nodes"
      text="Try another phrase or choose a different node type."
    />
  {:else}<NodeList nodes={result.data.hits} />{/if}
  {#if result.data && result.data.total > 0}<div class="pagination">
      <span>
        {number(result.data.start + 1)}–{number(Math.min(result.data.start + size, result.data.total))} of {number(
          result.data.total,
        )}
      </span>
      <div>
        <Button size="sm" disabled={offset === 0 || result.pending} onclick={() => (offset -= size)}>
          Previous
        </Button>
        <Button
          size="sm"
          disabled={offset + size >= result.data.total || result.pending}
          onclick={() => (offset += size)}
        >
          Next
        </Button>
      </div>
    </div>{/if}
</section>
