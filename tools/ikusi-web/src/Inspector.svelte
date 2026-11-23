<script lang="ts">
  import { Badge, Button, Empty, Icon, Prose, Segmented, copy, task } from '@enekos/osagai';
  import { api, type Detail } from './api';
  import { external, nodeHref, date } from './navigation';
  import { inline, markdown } from './markdown';
  import Kind from './Kind.svelte';
  let { detail, ids }: { detail: Detail; ids: Set<string> | undefined } = $props();
  let active = $state('connections');
  const n = $derived(detail.node);
  const href = $derived(external(n.link));
  const open = task((how: 'edit' | 'reveal') => api.open(n.id, how), { success: 'Opened locally' });
  export function command(edit: boolean) {
    if (edit && n.local) void open('edit');
    if (!edit && href) window.open(href, '_blank', 'noopener');
  }
</script>

<article id="node-details" tabindex="-1" class="panel inspector">
  <div class="inspect-head">
    <div class="node-meta">
      <Kind type={n.type} small /><span>{n.type}</span>{#if n.status}<Badge>
          {n.status}
        </Badge>{/if}{#if n.state && n.state !== n.status}<Badge>{n.state}</Badge>{/if}
    </div>
    <h2>{n.title}</h2>
    <code class="id-text">{n.id}</code>
    {#if n.summary}<div class="summary">{@html inline(n.summary, ids)}</div>{/if}
    {#if n.updated}<small>Updated {date(n.updated)}</small>{/if}
    <div class="actions">
      {#if href}<Button {href} target="_blank" rel="noopener" icon="external" size="sm">
          Open source
        </Button>{/if}
      {#if n.local}<Button size="sm" icon="edit" loading={open.pending} onclick={() => command(true)}>
          Open locally
        </Button>
        <Button size="sm" onclick={() => open('reveal')}>Reveal</Button>{/if}
      <Button size="sm" icon="copy" onclick={() => copy(n.id, { toast: 'Node ID copied' })}>Copy ID</Button>
      {#if ['project', 'decision', 'concept'].includes(n.type)}<Button
          size="sm"
          icon="link"
          onclick={() => copy('[[' + n.id + ']]', { toast: 'Wiki link copied' })}
        >
          Copy wiki link
        </Button>{/if}
    </div>
  </div>
  <div class="inspect-tabs">
    <Segmented
      bind:value={active}
      label="Node information"
      options={[
        { value: 'connections', label: 'Connections (' + n.degree + ')' },
        { value: 'page', label: 'Wiki page', disabled: !detail.body },
      ]}
    />
  </div>
  {#if active === 'page' && detail.body}
    <div class="wiki-body"><Prose>{@html markdown(detail.body, ids)}</Prose></div>
  {:else}
    {#each ['out', 'in'] as dir}
      {@const groups = dir === 'out' ? detail.out : detail.in}
      {#if groups.length}<section class="relations">
          <h3>
            {dir === 'out' ? 'Outgoing' : 'Incoming'}
            <span>{dir === 'out' ? detail.counts.out : detail.counts.in}</span>
          </h3>
          {#each groups as group}
            <details open={group === groups[0]}>
              <summary>
                <span>{group.rel.toLowerCase().replaceAll('_', ' ')}</span><small>{group.count}</small>
              </summary>
              {#each group.items as item}
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
              {/each}
            </details>
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
