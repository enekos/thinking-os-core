<script lang="ts">
  import { untrack } from 'svelte';
  import { Button } from '@enekos/osagai';
  import type { ChangedFile } from './api';
  let { file, expanded }: { file: ChangedFile; expanded: boolean } = $props();
  let open = $state(untrack(() => expanded));
  let limit = $state(120);
  const lines = $derived(open && file.patch ? file.patch.split('\n') : []);
</script>

<details class="file-diff" bind:open>
  <summary>
    <strong>{file.filename}</strong>
    <span class="file-meta">{file.status} <b>+{file.additions}</b> <em>−{file.deletions}</em></span>
  </summary>
  {#if open}
    {#if file.previous_filename}<p>Renamed from <code>{file.previous_filename}</code></p>{/if}
    {#if file.patch}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Long diff lines must be keyboard-scrollable.) -->
      <div class="patch" role="region" aria-label={'Diff for ' + file.filename} tabindex="0">
        <pre><code
            >{#each lines.slice(0, limit) as line}<span
                class:add={line.startsWith('+')}
                class:del={line.startsWith('-')}
                class:hunk={line.startsWith('@@')}>{line}{'\n'}</span
              >{/each}</code
          ></pre>
      </div>
      {#if lines.length > limit}<div class="more">
          <Button size="sm" onclick={() => (limit += 120)}>Show more lines</Button>
        </div>{/if}
      {#if file.truncated}<p>
          Preview limited to the first 65,536 characters. Open Files changed on GitHub for the full diff.
        </p>{/if}
    {:else}<p>
        {file.additions === 0 && file.deletions === 0 && ['renamed', 'copied'].includes(file.status)
          ? 'File contents unchanged.'
          : 'GitHub did not provide a text diff for this file.'}
      </p>{/if}
  {/if}
</details>

<style>
  details {
    min-width: 0;
    margin-top: 9px;
    border: 1px solid var(--o-border);
    border-radius: 9px;
    overflow: hidden;
  }
  summary {
    padding: 10px 13px;
    background: var(--o-surface-2);
    cursor: pointer;
    font-size: 0.86rem;
  }
  summary strong {
    font: 600 0.84rem var(--o-mono);
  }
  .file-meta {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: 12px;
    color: var(--o-text-2);
    font-size: 0.76rem;
  }
  .file-meta b {
    color: var(--mint-ink);
  }
  .file-meta em {
    color: var(--rose-ink);
    font-style: normal;
    font-weight: 600;
  }
  .patch {
    max-width: 100%;
    overflow-x: auto;
  }
  pre {
    margin: 0;
    min-width: 100%;
    width: max-content;
    font: 0.8rem/1.65 var(--o-mono);
    tab-size: 4;
  }
  pre span {
    display: block;
    padding-inline: 13px;
  }
  .add {
    background: var(--mint-bg);
  }
  .del {
    background: var(--rose-bg);
  }
  .hunk {
    color: var(--blue-ink);
    background: var(--blue-bg);
  }
  p,
  .more {
    padding: 9px 13px;
    font-size: 0.84rem;
    color: var(--o-text-2);
  }
</style>
