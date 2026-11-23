<script lang="ts">
  import { Icon } from '@enekos/osagai';
  import type { GraphNode } from './api';
  import { nodeHref, number } from './navigation';
  import Kind from './Kind.svelte';
  const preview = (text: string) =>
    text
      .replace(/`([^`]+)`/g, '$1')
      .replace(/\[\[([^|\]]+)(?:\|([^\]]+))?\]\]/g, (_, id, label) => label || id);
  let { nodes }: { nodes: GraphNode[] } = $props();
</script>

<ul class="node-list">
  {#each nodes as node (node.id)}
    <li>
      <a href={nodeHref(node.id)}>
        <Kind type={node.type} />
        <span class="node-copy">
          <strong>{node.title}</strong><small>{preview(node.summary) || node.id}</small>
        </span>
        <span class="type-tag">{node.type}</span>
        <span class="row-count" title="Connections">
          <Icon name="network" size={14} />{number(node.degree)}
        </span>
        <Icon name="chevron" size={15} />
      </a>
    </li>
  {/each}
</ul>
