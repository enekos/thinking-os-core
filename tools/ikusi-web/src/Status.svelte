<script lang="ts">
  import { Badge, Icon, QueryError, query } from '@enekos/osagai';
  import { api, type WorkList } from './api';
  import { date, external, nodeHref } from './navigation';
  let { ids }: { ids: Set<string> | undefined } = $props();
  const board = query(api.status, { every: 8000 });
  const priorities = ['', 'Urgent', 'High', 'Medium', 'Low'];
</script>

<header class="page-intro">
  <h1>Work status</h1>
  {#if board.data}<p>
      {board.data.login}
      {#if board.data.taken_at && !Number.isNaN(Date.parse(board.data.taken_at))}
      · Last poll <time datetime={board.data.taken_at}>
        {date(board.data.taken_at)}
        {new Date(board.data.taken_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
      </time>{/if}
    </p>
  {:else}<p>Your pull requests, review requests, and assigned issues.</p>{/if}
</header>
{#snippet lane(title: string, icon: string, list: WorkList, empty: string)}
  <section class="panel lane">
    <div class="panel-heading">
      <h2><Icon name={icon} />{title}</h2>
      <span class="type-tag">{list.total}</span>
    </div>
    {#if list.rows.length}
      <ul>
        {#each list.rows as row (row.id)}<li>
            <a href={external(row.url)} target="_blank" rel="noopener">
              <strong>{row.title}</strong><Icon name="external" size={14} />
            </a>
            <small>
              {[row.slug || row.id, row.project, date(row.created || row.updated)]
                .filter(Boolean)
                .join(' · ')}
            </small>
            <div class="work-tags">
              {#if row.draft}<Badge>Draft</Badge>{/if}{#if row.review}<Badge
                  tone={row.review === 'approved' ? 'ok' : 'warn'}
                >
                  {row.review}
                </Badge>{/if}
              {#if row.checks === 'red'}<Badge tone="danger" title={row.failing?.join(', ')}>
                  Checks failing
                </Badge>{/if}
              {#if row.state}<Badge>{row.state}</Badge>{/if}{#if row.priority}<Badge>
                  {priorities[row.priority]}
                </Badge>{/if}
              {#if row.blocked_by?.length}<Badge tone="danger" title={row.blocked_by.join(', ')}>
                  Blocked
                </Badge>{/if}
              {#if row.prs?.length}<Badge>{row.prs.length} linked PRs</Badge>{/if}
              {#if ids?.has(row.id)}<a href={nodeHref(row.id)}>View in graph</a>{/if}
            </div>
          </li>{/each}
      </ul>
      {#if list.total > list.rows.length}<small class="message">
          Showing the latest {list.rows.length} of {list.total}.
        </small>{/if}
    {:else}<p class="message">{empty}</p>{/if}
  </section>
{/snippet}
{#if board.error}<QueryError query={board} title="Work status is unavailable" />
{:else if board.data}
  {@const data = board.data}
  <div class="board-grid">
    {@render lane('My open pull requests', 'branch', data.opened, 'You have no open pull requests.')}
    {@render lane(
      'Waiting for my review',
      'eye',
      data.review,
      'No pull requests are waiting for your review.',
    )}
    {#if data.linear || data.github_issues}
      {@render lane('In progress', 'clock', data.doing, 'You have no issues in progress.')}
      {@render lane('Up next', 'inbox', data.next, 'No assigned issues are waiting to start.')}
    {:else}<section class="panel">
        <div class="panel-heading"><h2>Assigned issues</h2></div>
        <p class="message">Run <code>zaindu poll</code> to load an issue snapshot.</p>
      </section>{/if}
  </div>
{:else}<div class="surface-loading">Loading work status…</div>{/if}
