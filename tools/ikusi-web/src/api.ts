export interface NodeType {
  t: string;
  c: number;
  w: string;
  s: number;
}
export interface Meta {
  nodes: number;
  edges: number;
  loaded_at: string;
  revision: string;
  root: string;
  types: NodeType[];
  type_links: { source: string; target: string; count: number }[];
}
export interface GraphNode {
  id: string;
  title: string;
  type: string;
  summary: string;
  state: string;
  status: string;
  degree: number;
  updated: string;
  link: string | null;
  local: string | null;
}
export type NodeView = 'page' | 'pr' | 'connections' | 'map';
export interface PullStatus {
  state: 'OPEN' | 'CLOSED' | 'MERGED';
  isDraft: boolean;
  reviewDecision: string | null;
  checks: string | null;
  headRefName: string;
  baseRefName: string;
  additions: number;
  deletions: number;
  changedFiles: number;
}
export interface ChangedFile {
  filename: string;
  previous_filename: string | null;
  status: string;
  additions: number;
  deletions: number;
  patch: string | null;
  truncated: boolean;
}
export interface Neighbor extends GraphNode {
  prov: string;
  conf: number;
  evidence: string;
  since: string;
}
export interface Group {
  dir: 'in' | 'out';
  rel: string;
  count: number;
  deprecated: boolean;
  items: Neighbor[];
}
export interface Detail {
  node: GraphNode;
  body: string | null;
  in: Group[];
  out: Group[];
  counts: { in: number; out: number };
}
export interface Search {
  hits: GraphNode[];
  total: number;
}
export interface Point {
  g?: number;
  id: string;
  n: string;
  t: string;
  d: number;
  h: number;
  s: string;
  x: number;
  y: number;
}
export interface Scene {
  nodes: Point[];
  edges: { s: number; t: number; r: string; c: number }[];
  held?: number;
  truncated?: boolean;
}
interface WorkItem {
  id: string;
  title: string;
  url: string;
  updated: string;
  created: string;
  slug?: string;
  project?: string;
  state?: string;
  draft?: boolean;
  review?: string;
  checks?: string;
  failing?: string[];
  priority?: number;
  blocked_by?: string[];
  prs?: string[];
}
export interface WorkList {
  total: number;
  rows: WorkItem[];
}
export interface Board {
  taken_at: string;
  login: string;
  linear: boolean;
  github_issues: boolean;
  opened: WorkList;
  review: WorkList;
  doing: WorkList;
  next: WorkList;
}

async function request<T>(path: string, params: Record<string, string> = {}, method = 'GET'): Promise<T> {
  const url = new URL('/api/' + path, location.origin);
  url.search = new URLSearchParams(params).toString();
  const response = await fetch(url, { method, headers: { Accept: 'application/json' } });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error || response.statusText);
  }
  return response.json();
}

let whole: { revision: string; result: Promise<Readonly<Scene>> } | undefined;

function scene(id: string, depth: number, revision: string) {
  if (id) return request<Scene>('subgraph', { id, depth: String(depth) }).then((data) => Object.freeze(data));
  if (whole?.revision === revision) return whole.result;
  const result = request<Scene>('scene')
    .then((data) => Object.freeze(data))
    .catch((error) => {
      if (whole?.result === result) whole = undefined;
      throw error;
    });
  whole = { revision, result };
  return result;
}

export const api = {
  meta: () => request<Meta>('meta'),
  ids: async () => new Set((await request<{ ids: string[] }>('ids')).ids),
  search: (q = '', type = '', limit = 20, offset = 0) =>
    request<Search>('search', { q, type, limit: String(limit), offset: String(offset) }),
  node: (id: string) => request<Detail>('node', { id, per_relation: '25' }),
  pullStatus: (id: string) => request<PullStatus>('pr', { id }),
  pullFiles: (id: string, page: number) => request<ChangedFile[]>('pr/files', { id, page: String(page) }),
  connections: (id: string, group: Group, offset: number) =>
    request<{ items: Neighbor[]; count: number; offset: number }>('connections', {
      id,
      dir: group.dir,
      rel: group.rel,
      offset: String(offset),
      limit: '25',
    }),
  scene,
  spotlight: (id: string) => request<Scene>('subgraph', { id, depth: '1', limit: '16' }),
  status: () => request<Board>('status'),
  open: (id: string, how: 'edit' | 'reveal') => request('open', { id, how }, 'POST'),
};
