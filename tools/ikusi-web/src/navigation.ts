export const nodeHref = (id: string) => '#/n/' + encodeURIComponent(id);
export function directoryHref(type = '', term = '', page = 1) {
  const params = new URLSearchParams();
  if (term) params.set('q', term);
  if (page > 1) params.set('page', String(page));
  const query = params.toString();
  return '#/nodes' + (type ? '/' + encodeURIComponent(type) : '') + (query ? '?' + query : '');
}
export function currentRoute() {
  const [path, query] = location.hash.slice(2).split('?');
  const [page, raw = ''] = path.split('/');
  const params = new URLSearchParams(query);
  let id = '';
  try {
    id = decodeURIComponent(raw);
  } catch {
    /* malformed bookmark */
  }
  switch (page) {
    case 'n':
      return { page: 'graph', id, type: '' } as const;
    case 'graph':
    case 'status':
      return { page, id: '', type: '' } as const;
    case 'nodes': {
      const index = Number(params.get('page'));
      return {
        page,
        id: '',
        type: id,
        term: params.get('q') ?? '',
        index: Number.isSafeInteger(index) && index > 0 ? index : 1,
      } as const;
    }
    default:
      return { page: 'overview', id: '', type: '' } as const;
  }
}
const kinds: Record<string, [string, string, string]> = {
  project: ['Projects', 'folder', 'mint'],
  concept: ['Concepts', 'bulb', 'violet'],
  decision: ['Decisions', 'branch', 'amber'],
  task: ['Tasks', 'checks', 'blue'],
  repo: ['Repositories', 'code', 'blue'],
  commit: ['Commits', 'history', 'amber'],
  path: ['Paths', 'file', 'slate'],
  milestone: ['Milestones', 'calendar', 'amber'],
  cycle: ['Cycles', 'refresh', 'blue'],
  attempt: ['Attempts', 'wrench', 'slate'],
  source: ['Sources', 'file', 'amber'],
  person: ['People', 'users', 'rose'],
  pr: ['Pull requests', 'branch', 'violet'],
  briefing: ['Briefings', 'file', 'blue'],
  issue: ['Issues', 'inbox', 'rose'],
  service: ['Services', 'database', 'mint'],
  test: ['Tests', 'shield', 'mint'],
};
export const collection = (type: string) =>
  kinds[type]?.[0] ?? (type.replaceAll('_', ' ') + 's').replace(/^./, (c) => c.toUpperCase());
export const iconOf = (type: string) => kinds[type]?.[1] ?? 'file';
export const colorOf = (type: string) => kinds[type]?.[2] ?? 'slate';
export const number = (value: number) => value.toLocaleString();
export function external(url: string | null | undefined) {
  if (!url) return undefined;
  try {
    const u = new URL(url);
    return ['http:', 'https:'].includes(u.protocol) ? u.href : undefined;
  } catch {
    return undefined;
  }
}
export const date = (stamp: string) => {
  if (!stamp) return '';
  const parsed = new Date(stamp.length === 10 ? stamp + 'T12:00:00' : stamp);
  return Number.isNaN(parsed.getTime())
    ? stamp
    : parsed.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
};
