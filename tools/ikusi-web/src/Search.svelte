<script lang="ts">
  import { Chip, Empty, Icon, Input, Kbd, Modal, QueryError, query } from '@enekos/osagai';
  import { api, type Meta } from './api';
  import { collection, external, nodeHref } from './navigation';
  import Kind from './Kind.svelte';
  let { open = $bindable(), meta }: { open: boolean; meta: Meta } = $props();
  let term = $state('');
  let types = $state<string[]>([]);
  let selected = $state(0);
  let filters = $state(false);
  let list: HTMLDivElement;
  const result = query(
    () => {
      void meta.loaded_at;
      return api.search(term, types.join(','), 50);
    },
    { enabled: () => open },
  );
  function choose(i: number, source = false) {
    if (result.pending) return;
    const hit = result.data?.hits[i];
    if (!hit) return;
    const link = external(hit.link);
    if (source && link) {
      window.open(link, '_blank', 'noopener');
      return;
    }
    open = false;
    location.hash = nodeHref(hit.id);
  }
  function keys(e: KeyboardEvent) {
    const n = result.data?.hits.length ?? 0;
    if (e.key === 'Enter') {
      e.preventDefault();
      choose(selected, e.ctrlKey || e.metaKey);
    }
    if (n && ['ArrowDown', 'ArrowUp'].includes(e.key)) {
      e.preventDefault();
      selected = (selected + (e.key === 'ArrowDown' ? 1 : -1) + n) % n;
      list?.children[selected]?.scrollIntoView({ block: 'nearest' });
    }
  }
</script>

<Modal bind:open title="Search workspace" size="lg">
  <div class="search-tools">
    <div class="field-with-icon">
      <Icon name="search" /><Input
        bind:value={term}
        oninput={() => (selected = 0)}
        onkeydown={keys}
        aria-label="Search workspace"
        placeholder="Search titles, IDs, and summaries…"
        role="combobox"
        aria-expanded="true"
        aria-controls="search-results"
        aria-autocomplete="list"
        aria-activedescendant={result.data?.hits[selected] ? 'hit-' + selected : undefined}
      />
    </div>
    <button class="filter-toggle" aria-expanded={filters} onclick={() => (filters = !filters)}>
      <Icon name="filter" />Types{types.length ? ' (' + types.length + ')' : ''}
    </button>
  </div>
  {#if filters}<div class="filter-set">
      {#each meta.types as kind (kind.t)}<Chip
          selected={types.includes(kind.t)}
          onclick={() => {
            types = types.includes(kind.t) ? types.filter((t) => t !== kind.t) : [...types, kind.t];
            selected = 0;
          }}
        >
          {collection(kind.t)}
        </Chip>{/each}
    </div>{/if}
  {#if result.error}<QueryError query={result} />{:else if result.loaded && !result.data?.hits.length}<Empty
      compact
      icon="search"
      title="No matches"
      text="Try fewer words or remove a type filter."
    />{/if}
  <div bind:this={list} id="search-results" role="listbox" aria-label="Matching nodes" class="search-list">
    {#each result.data?.hits ?? [] as node, i (node.id)}
      <button
        id={'hit-' + i}
        role="option"
        aria-selected={selected === i}
        tabindex="-1"
        onclick={() => choose(i)}
        onpointermove={() => (selected = i)}
      >
        <Kind type={node.type} small /><span><strong>{node.title}</strong><small>{node.id}</small></span><em>
          {node.type}
        </em>
      </button>
    {/each}
  </div>
  {#snippet footer()}<span class="search-hint">
      <Kbd>↑ ↓</Kbd> Navigate <Kbd>Enter</Kbd> Open node
    </span>{/snippet}
</Modal>
