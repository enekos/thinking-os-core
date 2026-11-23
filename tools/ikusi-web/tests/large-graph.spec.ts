import { expect, test } from '@playwright/test';
import { Plot } from '../src/plot';
import type { Meta, Scene } from '../src/api';

const meta: Meta = {
  root: '/wiki',
  loaded_at: '2026-01-01T00:00:00Z',
  revision: '1',
  nodes: 1200,
  edges: 2400,
  types: [
    { t: 'repo', c: 100, w: 'code', s: 0 },
    { t: 'project', c: 1100, w: 'wiki', s: 0 },
  ],
  type_links: [
    { source: 'repo', target: 'project', count: 1900 },
    { source: 'project', target: 'project', count: 500 },
  ],
};
const scene: Scene = {
  nodes: Array.from({ length: 1200 }, (_, i) => ({
    id: 'n' + i,
    t: i % 12 === 0 ? 'repo' : 'project',
    n: 'Node ' + i,
    s: '',
    d: i % 12 === 0 ? 12 : 1,
    h: 0,
    g: i - (i % 12),
    x: (Math.floor(i / 12) % 10) * 140 + (i % 4) * 8,
    y: Math.floor(i / 120) * 140 + (i % 3) * 8,
  })),
  edges: Array.from({ length: 1199 }, (_, i) => ({ s: i, t: i + 1, c: 1, r: 'LINK' })),
};

test('zoom preserves membership, respects hidden types and keeps focus selectable', () => {
  const plot = new Plot(scene, meta, new Set(['repo']), 'n1');
  const coarse = plot.level(0.1);
  expect(coarse.marks.length).toBeLessThan(200);
  expect(coarse.marks.reduce((sum, m) => sum + m.count, 0)).toBe(1100);
  expect(coarse.marks.find((m) => m.node.id === 'n1')?.count).toBe(1);
  expect(coarse.marks.every((m) => m.node.t === 'project')).toBe(true);
  const hubFocus = new Plot(scene, meta, new Set(), 'n0').level(0.1);
  expect(hubFocus.marks.filter((m) => m.node.id === 'n0').map((m) => m.count)).toEqual([1]);
  plot.level(0.3);
  expect(plot.level(0.1)).toBe(coarse);
  const overview = new Plot(scene, meta, new Set(), '').level(0.3);
  for (const [i, mark] of overview.marks.entries()) {
    expect(overview.marks.slice(i + 1).every((m) => Math.hypot(m.x - mark.x, m.y - mark.y) * 0.3 > 48)).toBe(
      true,
    );
  }
  const fine = plot.level(5);
  expect(fine.marks).toHaveLength(1100);
  expect(fine.marks.every((m) => m.count === 1)).toBe(true);
  expect(fine.visible({ x: 0, y: 0, k: 5 }, 350, 400).length).toBeLessThan(50);
});

test('large maps open from metadata and fetch one individual scene per revision', async ({ page }) => {
  let scenes = 0,
    ids = 0;
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/api/meta') return route.fulfill({ json: meta });
    if (path === '/api/scene') {
      scenes++;
      return route.fulfill({ json: scene });
    }
    if (path === '/api/ids') {
      ids++;
      return route.fulfill({ json: { ids: [] } });
    }
    return route.abort();
  });
  await page.goto('/#/graph');
  await expect(page.getByRole('link', { name: 'Browse Repositories (100 nodes)' })).toBeVisible();
  expect(scenes).toBe(0);
  expect(ids).toBe(0);
  await page.getByRole('button', { name: 'Types', exact: true }).click();
  await page.getByRole('checkbox', { name: 'Repositories (100)' }).uncheck();
  await page.getByRole('button', { name: 'Done', exact: true }).click();
  await expect(page.locator('.graph-caption')).toContainText('1,100 nodes · 500 connections');
  await expect(page.getByRole('link', { name: 'Browse Projects (1,100 nodes)' })).toHaveAttribute(
    'href',
    '#/nodes/project',
  );
  for (let i = 0; i < 2; i++) {
    await page.getByRole('radio', { name: 'Individual nodes' }).click();
    await expect(page.locator('canvas')).toBeVisible();
    await page.getByRole('radio', { name: 'Collections', exact: true }).click();
  }
  expect(scenes).toBe(1);
});
