<script lang="ts">
  import { Button, Icon, QueryError, query } from '@enekos/osagai';
  import { api, type Group } from './api';
  import { external, nodeHref } from './navigation';
  import Kind from './Kind.svelte';
  import { untrack } from 'svelte';
  let { id, group, expanded }: { id: string; group: Group; expanded: boolean } = $props();
  let open = $state(untrack(() => expanded));
  let page = $state(0);
  const start = $derived(Math.min(page, Math.max(0, Math.ceil(group.count / 25) - 1)) * 25);
  const more = query(() => api.connections(id, group, start), { enabled: () => open && start > 0 });
  const items = $derived(start === 0 ? group.items : more.data?.offset === start ? more.data.items : []);
</script>

<details bind:open>
  <summary>
    <span>{group.rel.toLowerCase().replaceAll('_', ' ')}</span><small>{group.count}</small>
  </summary>
  {#if open}{#if start && more.error}<QueryError query={more} />{:else if start && more.pending}<small>
        Loading connections…
      </small>{:else}{#each items as item}
        <div class="neighbor">
          <a href={nodeHref(item.id)}>
            <Kind type={item.type} small /><span>
              <strong>{item.title}</strong>
              <small>
                {[item.type, item.state, item.status !== item.state ? item.status : '', item.updated]
                  .filter(Boolean)
                  .join(' · ')}
              </small>
              {#if item.prov}<small title={item.evidence}>
                  {item.prov}{item.conf < 1 ? ' · ' + item.conf.toFixed(2) : ''}
                </small>{/if}
            </span>
          </a>
          {#if external(item.link)}<a
              class="neighbor-source"
              href={external(item.link)}
              target="_blank"
              rel="noopener"
              aria-label={'Open source for ' + item.title}
            >
              <Icon name="external" size={15} />
            </a>{/if}
        </div>
      {/each}{/if}
    {#if group.count > 25}
      <div class="relation-pages">
        <small>{start + 1}–{Math.min(start + 25, group.count)} of {group.count}</small>
        <Button size="sm" disabled={!start} onclick={() => (page = start / 25 - 1)}>Previous</Button>
        <Button size="sm" disabled={start + 25 >= group.count} onclick={() => (page = start / 25 + 1)}>
          Next
        </Button>
      </div>
    {/if}{/if}
</details>

<style>
  .relation-pages {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin: 10px 0;
  }
  .relation-pages small {
    margin-right: auto;
  }
</style>
