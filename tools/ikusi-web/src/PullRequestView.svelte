<script lang="ts">
  import { Button, QueryError, query } from '@enekos/osagai';
  import { api, type GraphNode } from './api';
  import { date, external, number } from './navigation';
  import FileDiff from './FileDiff.svelte';

  let { node }: { node: GraphNode } = $props();
  let page = $state(1);
  const status = query(() => api.pullStatus(node.id));
  const changed = query(async () => {
    const requested = page;
    return { page: requested, files: await api.pullFiles(node.id, requested) };
  });
  const pr = $derived(status.data);
  const result = $derived(changed.data?.page === page ? changed.data : undefined);
  const pageSize = 10;
  const offset = $derived((page - 1) * pageSize);
  const next = $derived(
    result?.files.length === pageSize && page < 300 && (!pr || offset + pageSize < pr.changedFiles),
  );
  const href = $derived(external(node.link));
  const states: Record<string, string> = {
    OPEN: 'Open',
    CLOSED: 'Closed',
    MERGED: 'Merged',
    DRAFT: 'Draft',
    APPROVED: 'Approved',
    CHANGES_REQUESTED: 'Changes requested',
    REVIEW_REQUIRED: 'Review required',
    SUCCESS: 'Passing',
    FAILURE: 'Failing',
    ERROR: 'Error',
    PENDING: 'Pending',
    EXPECTED: 'Expected',
  };
  const lifecycle = $derived(pr?.isDraft && pr.state === 'OPEN' ? 'DRAFT' : pr?.state);
  function refresh() {
    void status.reload();
    if (page === 1) void changed.reload();
    else page = 1;
  }
</script>

<section class="pull-reader" aria-label="Pull request details">
  <div class="status-toolbar">
    <h2>Pull request status</h2>
    <Button
      size="sm"
      variant="ghost"
      icon="refresh"
      disabled={status.pending || changed.pending}
      onclick={refresh}
    >
      Refresh
    </Button>
  </div>
  {#if pr && lifecycle}
    <dl class="pr-facts">
      <div>
        <dt>Status</dt>
        <dd><span class={'state ' + lifecycle.toLowerCase()}>{states[lifecycle]}</span></dd>
      </div>
      <div>
        <dt>Review</dt>
        <dd>{states[pr.reviewDecision ?? ''] ?? 'No decision yet'}</dd>
      </div>
      <div>
        <dt>Checks</dt>
        <dd>{states[pr.checks ?? ''] ?? 'No checks reported'}</dd>
      </div>
      <div>
        <dt>Branches</dt>
        <dd class="branches">
          <code>{pr.headRefName}</code><span class="direction">into</span><code>{pr.baseRefName}</code>
        </dd>
      </div>
    </dl>
  {:else if node.status || node.state}
    <p class="snapshot">
      Last graph update{node.updated ? ' · ' + date(node.updated) : ''} · {[node.status, node.state]
        .filter(Boolean)
        .join(' · ')}
    </p>
  {/if}
  {#if status.error}<QueryError query={status} title="Could not load live PR status" compact />
  {:else if status.pending && !pr}<p class="wait" role="status">Loading GitHub status…</p>{/if}

  <div class="diff-heading">
    <h2>
      Files changed{#if pr}<span class="file-count">{number(pr.changedFiles)}</span>{/if}
    </h2>
    {#if pr}<span class="totals" aria-label={pr.additions + ' additions, ' + pr.deletions + ' deletions'}>
        <b>+{number(pr.additions)}</b> <em>−{number(pr.deletions)}</em>
      </span>{/if}
    {#if href}<a href={href + '/files'} target="_blank" rel="noopener">View on GitHub ↗</a>{/if}
  </div>
  {#if changed.error}<QueryError query={changed} title="Could not load changed files" compact />
  {:else if !result}<p class="wait" role="status">Loading changed files…</p>
  {:else if !result.files.length}<p class="wait">
      {page === 1 ? 'This pull request has no changed files.' : 'No more changed files.'}
    </p>
  {:else}
    {#each result.files as file, i (file.filename)}<FileDiff {file} expanded={i === 0} />{/each}
  {/if}
  {#if page > 1 || next}
    <div class="file-pages">
      {#if result?.files.length}<small>
          Files {number(offset + 1)}–{number(offset + result.files.length)}{pr
            ? ' of ' + number(pr.changedFiles)
            : ''}
        </small>{/if}
      <Button size="sm" disabled={page === 1 || changed.pending} onclick={() => page--}>
        Previous files
      </Button>
      <Button size="sm" disabled={!next || changed.pending} onclick={() => page++}>Next files</Button>
    </div>
  {/if}
  {#if page === 300 && pr && pr.changedFiles > 3000}<p class="wait">
      GitHub provides the first 3,000 file previews here.
    </p>{/if}
</section>

<style>
  .pull-reader {
    padding: var(--reader-pad);
    border-top: 1px solid var(--o-border);
  }
  .status-toolbar,
  .diff-heading,
  .file-pages {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }
  .status-toolbar {
    justify-content: space-between;
  }
  .pr-facts {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr)) minmax(0, 2fr);
    margin: 12px 0 0;
    border-block: 1px solid var(--o-border);
  }
  .pr-facts > div {
    min-width: 0;
    padding: 15px 14px 15px 0;
  }
  dt,
  .direction,
  .snapshot,
  .wait {
    color: var(--o-text-2);
  }
  dt {
    font-size: 0.8rem;
    margin-bottom: 4px;
  }
  dd {
    font-weight: 600;
    font-size: 0.94rem;
  }
  dd code {
    font-size: 0.85em;
  }
  .branches {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 0.35em;
  }
  .direction {
    font-weight: 400;
  }
  .state {
    border-radius: 5px;
    padding: 2px 8px;
    background: var(--o-surface-2);
  }
  .merged {
    color: var(--violet-ink);
    background: var(--violet-bg);
  }
  .open {
    color: var(--mint-ink);
    background: var(--mint-bg);
  }
  .closed {
    color: var(--rose-ink);
    background: var(--rose-bg);
  }
  .diff-heading {
    margin: 26px 0 12px;
  }
  .diff-heading h2 {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .diff-heading a {
    margin-left: auto;
    font-size: 0.82rem;
  }
  .file-count {
    font: 600 0.8rem var(--o-font);
    background: var(--o-surface-2);
    border-radius: 5px;
    padding: 3px 7px;
  }
  .totals {
    font: 600 0.82rem var(--o-mono);
  }
  .totals b {
    color: var(--mint-ink);
  }
  .totals em {
    color: var(--rose-ink);
    font-style: normal;
  }
  .wait,
  .snapshot {
    padding-block: 10px;
    font-size: 0.9rem;
  }
  .file-pages {
    justify-content: flex-end;
    margin-top: 14px;
  }
  .file-pages small {
    margin-right: auto;
  }
  @media (max-width: 680px) {
    .pr-facts {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
