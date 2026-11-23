<script lang="ts">
  import { Icon, Kbd, Modal, Overlays, QueryError, query } from '@enekos/osagai';
  import { api, type Meta } from './api';
  import { collection, currentRoute, directoryHref, iconOf } from './navigation';
  import Overview from './Overview.svelte';
  import Directory from './Directory.svelte';
  import Explorer from './Explorer.svelte';
  import Status from './Status.svelte';
  import Search from './Search.svelte';

  const metadata = query(api.meta, { every: 8000 });
  let meta = $state.raw<Meta>();
  $effect(() => {
    const next = metadata.data;
    if (next && next.revision !== meta?.revision) meta = next;
  });
  let route = $state(currentRoute());
  const ids = query(
    () => {
      void meta?.revision;
      return api.ids();
    },
    { enabled: () => !!meta && (!!route.id || route.page === 'status') },
  );
  let search = $state(false),
    help = $state(false),
    menu = $state(false);
  let theme = $state(document.documentElement.dataset.theme || 'light');
  let depth = $state(1);
  let explorer: Explorer | undefined = $state();
  const navigation = [
    { page: 'overview', label: 'Overview', icon: 'overview', href: '#/' },
    { page: 'graph', label: 'Graph explorer', icon: 'network', href: '#/graph' },
    { page: 'nodes', label: 'Node directory', icon: 'layers', href: '#/nodes' },
    { page: 'status', label: 'Work status', icon: 'inbox', href: '#/status' },
  ];
  const crumb = $derived(navigation.find((n) => n.page === route.page)?.label || 'Overview');
  const shortcuts = [
    ['Search workspace', 'mod+k'],
    ['Open selected result', 'Enter'],
    ['Open source', 'mod+Enter'],
    ['Open local file', 'mod+e'],
    ['Back / forward', '[ / ]'],
    ['Neighborhood depth', '1 / 2 / 3'],
    ['Toggle whole graph', 'g'],
    ['Fit graph', 'f'],
    ['Work status', 's'],
    ['Keyboard shortcuts', '?'],
  ];
  function navigate() {
    route = currentRoute();
    menu = false;
    search = false;
    help = false;
    window.scrollTo(0, 0);
  }
  function setTheme() {
    theme = theme === 'light' ? 'dark' : 'light';
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('ikusi-theme', theme);
  }
  function keydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey,
      key = e.key.toLowerCase();
    if (mod && key === 'k') {
      e.preventDefault();
      menu = false;
      help = false;
      search = !search;
      return;
    }
    if (search || help || menu || document.querySelector('[aria-modal="true"]') || e.defaultPrevented) return;
    if (
      e.target instanceof HTMLElement &&
      (e.target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(e.target.tagName))
    )
      return;
    if (mod && route.id && (key === 'e' || key === 'enter')) {
      e.preventDefault();
      explorer?.key(key === 'e' ? 'edit' : 'source');
      return;
    }
    if (mod || e.altKey) return;
    if (key === '/') {
      e.preventDefault();
      search = true;
    } else if (key === '?') help = true;
    else if (key === '[') history.back();
    else if (key === ']') history.forward();
    else if (key === 's') location.hash = route.page === 'status' ? '#/' : '#/status';
    else if (key === 'g' && route.page !== 'graph') location.hash = '#/graph';
    else explorer?.key(key);
  }
  const workspace = $derived(meta?.root.split('/').filter(Boolean).at(-1) || 'Local workspace');
</script>

<svelte:window onhashchange={navigate} onkeydown={keydown} />
<svelte:head><title>{route.id ? 'Node details' : route.type ? collection(route.type) : crumb} · Ikusi</title></svelte:head>
<a
  class="skip"
  href="#workspace"
  onclick={(e) => {
    e.preventDefault();
    document.getElementById('workspace')?.focus();
  }}
>
  Skip to content
</a>
{#snippet nav()}
  <nav aria-label="Workspace">
    <div class="nav-caption">Workspace</div>
    {#each navigation as item}
      <a
        class="nav-item"
        class:active={route.page === item.page && !route.type}
        aria-current={route.page === item.page && !route.type ? 'page' : undefined}
        href={item.href}
        onclick={() => (menu = false)}
      >
        <Icon name={item.icon} size={18} />{item.label}
      </a>
    {/each}
  </nav>
  {#if meta?.types.length}
    <nav aria-label="Collections">
      <div class="nav-caption">Collections</div>
      {#each meta.types.filter( (t) => ['project', 'concept', 'decision', 'task'].includes(t.t) ) as kind (kind.t)}
        <a
          href={directoryHref(kind.t)}
          class="nav-item"
          class:active={route.type === kind.t}
          aria-current={route.type === kind.t ? 'page' : undefined}
          onclick={() => (menu = false)}
        >
          <Icon name={iconOf(kind.t)} size={17} />{collection(kind.t)}<span>{kind.c}</span>
        </a>
      {/each}
    </nav>
  {/if}
{/snippet}
<div class="shell" inert={menu || help || (search && !!meta)}>
  <aside class="sidebar">
    <a href="#/" class="brand" aria-label="Ikusi overview">
      <span class="brand-mark">
        <svg viewBox="0 0 38 38" aria-hidden="true">
          <g fill="none" stroke="currentColor" stroke-width="2.3">
            <ellipse cx="19" cy="19" rx="15" ry="6.7" transform="rotate(-40 19 19)" /><ellipse
              cx="19"
              cy="19"
              rx="15"
              ry="6.7"
              transform="rotate(40 19 19)"
            />
          </g><circle cx="19" cy="19" r="2.2" fill="currentColor" />
        </svg>
      </span>ikusi
    </a>
    <div class="side-nav">{@render nav()}</div>
    <div class="side-bottom">
      <button class="nav-item" onclick={() => (help = true)}>
        <Icon name="keyboard" size={18} />Keyboard shortcuts
      </button>
      <div class="workspace-id">
        <span class="workspace-icon"><Icon name="layers" size={19} /></span>
        <div><strong>{workspace}</strong><small>Local workspace</small></div>
      </div>
    </div>
  </aside>
  <div class="main-shell">
    <header class="topbar">
      <button class="mobile-menu" aria-label="Open navigation" onclick={() => (menu = true)}>
        <Icon name="menu" size={21} />
      </button>
      <div class="breadcrumbs">
        <a class="mobile-brand" href="#/">ikusi</a><Icon name="columns" size={16} /><span>
          Workspace
        </span><span>/</span><b>{route.id ? 'Node details' : route.type ? collection(route.type) : crumb}</b>
      </div>
      <div class="top-actions">
        <button class="search-trigger" aria-label="Search workspace" onclick={() => (search = true)}>
          <Icon name="search" size={16} /><span>Search workspace…</span><Kbd keys="mod+k" />
        </button>
        <button
          class="theme-button"
          aria-label={'Switch to ' + (theme === 'dark' ? 'light' : 'dark') + ' theme'}
          onclick={setTheme}
        >
          <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={18} />
        </button>
        <button
          class="mobile-help theme-button"
          aria-label="Keyboard shortcuts"
          onclick={() => (help = true)}
        >
          <Icon name="keyboard" />
        </button>
      </div>
    </header>
    <main id="workspace" class:node-workspace={!!route.id} tabindex="-1">
      {#if metadata.error}<QueryError query={metadata} title="Could not refresh the workspace" />{/if}
      {#if meta}
        {#if route.page === 'overview'}<Overview {meta} />
        {:else if route.page === 'nodes'}{#key route}<Directory
              type={route.type}
              initialTerm={route.term}
              initialPage={route.index}
              {meta}
            />{/key}
        {:else if route.page === 'graph'}<Explorer
            bind:this={explorer}
            id={route.id}
            {meta}
            ids={ids.data}
            {theme}
            bind:depth
            search={() => (search = true)}
          />
        {:else}<Status ids={ids.data} />{/if}
      {:else if !metadata.error}<div class="surface-loading">Opening workspace…</div>{/if}
    </main>
  </div>
</div>
{#if search && meta}<Search bind:open={search} {meta} />{/if}
<Modal bind:open={menu} title="Navigation" size="sm">
  {@render nav()}<button
    class="nav-item"
    onclick={() => {
      menu = false;
      help = true;
    }}
  >
    <Icon name="keyboard" size={18} />Keyboard shortcuts
  </button>
</Modal>
<Modal bind:open={help} title="Keyboard shortcuts" size="sm">
  <dl class="keys">
    {#each shortcuts as item}<div>
        <dt>{item[0]}</dt>
        <dd>
          {#if item[1].startsWith('mod+')}<Kbd keys={item[1]} />{:else}<Kbd>{item[1]}</Kbd>{/if}
        </dd>
      </div>{/each}
  </dl>
</Modal>
<Overlays />
