import { expect, test, type Page } from '@playwright/test';
import type { GraphNode } from '../src/api';

const nodes: GraphNode[] = Array.from({ length: 35 }, (_, index) => ({
  id: 'project/' + (index + 1),
  type: 'project',
  title: 'Project ' + (index + 1),
  summary: '',
  state: '',
  status: '',
  degree: 0,
  updated: '',
  link: null,
  local: null,
}));

async function workspace(page: Page) {
  const state = { version: 1, scenes: 0, beforePageTwo: Promise.resolve() };
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    const node = nodes.find((n) => n.id === url.searchParams.get('id')) ?? nodes[0];
    const reply = (json: unknown) => route.fulfill({ json });
    switch (url.pathname) {
      case '/api/meta':
        return reply({
          root: '/wiki',
          loaded_at: String(state.version),
          nodes: nodes.length,
          edges: 0,
          type_links: [],
          types: [{ t: 'project', c: nodes.length, s: 0, w: 'wiki' }],
        });
      case '/api/ids':
        return reply({ ids: nodes.map((n) => n.id) });
      case '/api/search': {
        const offset = Number(url.searchParams.get('offset'));
        if (offset > 0) await state.beforePageTwo;
        return reply({
          total: nodes.length,
          hits: nodes.slice(offset, offset + Number(url.searchParams.get('limit'))),
        });
      }
      case '/api/subgraph':
        state.scenes++;
        return reply({
          nodes: [{ id: node.id, n: node.title, s: '', t: 'project', d: 0, h: 0, x: 0, y: 0 }],
          edges: [],
        });
      case '/api/node':
        return reply({ node, body: null, in: [], out: [], counts: { in: 0, out: 0 } });
      default:
        return route.abort();
    }
  });
  return state;
}

test('a poll refreshes the open neighborhood only when its graph changes', async ({ page }) => {
  const state = await workspace(page);
  await page.clock.install();
  await page.goto('/#/n/project%2F1');
  await expect(page.locator('canvas')).toBeVisible();
  const tick = async () => {
    const response = page.waitForResponse((r) => r.url().endsWith('/api/meta'));
    await page.clock.fastForward(8100);
    await response;
  };
  await tick();
  // A second poll ensures the previous response and any reactive requests have settled.
  await tick();
  expect(state.scenes).toBe(1);
  state.version++;
  await tick();
  await expect.poll(() => state.scenes).toBe(2);
});

test('directory counts match the displayed page and back restores that page', async ({ page }) => {
  const state = await workspace(page);
  let release = () => {};
  state.beforePageTwo = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.goto('/#/nodes');
  await expect(page.getByText('1–30 of 35')).toBeVisible();
  const next = page.waitForRequest((r) => new URL(r.url()).searchParams.get('offset') === '30');
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await next;
  await expect(page.getByText('1–30 of 35')).toBeVisible();
  release();
  await expect(page.getByText('31–35 of 35')).toBeVisible();
  await page.getByRole('link', { name: /Project 31/ }).click();
  await expect(page.locator('.inspector h2')).toHaveText('Project 31');
  await page.goBack();
  await expect(page.getByText('31–35 of 35')).toBeVisible();
  await expect(page.getByRole('link', { name: /Project 31/ })).toBeVisible();
});
