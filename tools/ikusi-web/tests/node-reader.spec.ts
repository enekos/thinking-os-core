import { expect, test, type Page } from '@playwright/test';
import type { GraphNode, ChangedFile, PullStatus } from '../src/api';

const wiki: GraphNode = {
  id: 'guide',
  type: 'concept',
  title: 'Reading guide',
  summary: 'Workspace notes',
  state: '',
  status: '',
  updated: '',
  degree: 0,
  local: null,
  link: null,
};
const pr: GraphNode = {
  ...wiki,
  id: 'gh:acme/reader#8',
  type: 'pr',
  title: 'Update reader',
  status: 'open',
  link: 'https://github.com/acme/reader/pull/8',
};
const live: PullStatus = {
  state: 'MERGED',
  isDraft: false,
  checks: 'SUCCESS',
  reviewDecision: 'APPROVED',
  headRefName: 'reader',
  baseRefName: 'main',
  additions: 11,
  deletions: 1,
  changedFiles: 11,
};
const file: ChangedFile = {
  filename: 'guide.md',
  previous_filename: null,
  status: 'modified',
  additions: 1,
  deletions: 1,
  patch: '@@ -1 +1 @@\n-old title\n+new title',
  truncated: false,
};

async function workspace(page: Page) {
  const requests = { scenes: 0, status: 0, pages: [] as number[], online: true };
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    switch (url.pathname) {
      case '/api/meta':
        return route.fulfill({
          json: { nodes: 2, edges: 0, root: '/wiki', types: [], type_links: [], revision: '1' },
        });
      case '/api/ids':
        return route.fulfill({ json: { ids: [wiki.id, pr.id] } });
      case '/api/node': {
        const isPr = url.searchParams.get('id') === pr.id;
        return route.fulfill({
          json: {
            node: isPr ? pr : wiki,
            body: isPr
              ? null
              : '# Reading guide\n\n## Getting started\n\nRead the [[gh:acme/reader#8|pull request]].',
            in: [],
            out: [],
            counts: { in: 0, out: 0 },
          },
        });
      }
      case '/api/subgraph':
        requests.scenes++;
        return route.fulfill({ json: { nodes: [], edges: [] } });
      case '/api/pr':
        requests.status++;
        return route.fulfill(
          requests.online ? { json: live } : { status: 502, json: { error: 'GitHub is unavailable' } },
        );
      case '/api/pr/files': {
        const index = Number(url.searchParams.get('page'));
        requests.pages.push(index);
        if (!requests.online) return route.fulfill({ status: 502, json: { error: 'GitHub is unavailable' } });
        const files =
          index === 1
            ? [file, ...Array.from({ length: 9 }, (_, i) => ({ ...file, filename: 'other-' + i }))]
            : [{ ...file, filename: 'image.png', patch: null }];
        return route.fulfill({ json: files });
      }
      default:
        return route.abort();
    }
  });
  return requests;
}

test('wiki opens across the workspace and a linked PR opens on its current status and diff', async ({
  page,
}) => {
  const requests = await workspace(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/#/n/guide');
  await expect(page.getByRole('heading', { name: 'Reading guide' })).toHaveCount(1);
  await expect(page.getByRole('heading', { name: 'Getting started' })).toBeVisible();
  expect(
    await page
      .locator('.wiki-body .o-prose')
      .evaluate((el) => el.clientWidth / document.querySelector('main')!.clientWidth),
  ).toBeGreaterThan(0.85);
  expect(requests.scenes).toBe(0);
  expect(requests.status).toBe(0);
  await page.getByRole('link', { name: 'pull request', exact: true }).click();
  await expect(page.getByText('Merged', { exact: true })).toBeVisible();
  await expect(page.getByText('Approved', { exact: true })).toBeVisible();
  await expect(page.getByText('Passing', { exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Diff for guide.md' })).toContainText('+new title');
  await page.getByRole('button', { name: 'Next files' }).click();
  await expect(page.getByText('GitHub did not provide a text diff for this file.')).toBeVisible();
  await expect(page.getByText('Files 11–11 of 11')).toBeVisible();
  expect(requests.pages).toEqual([1, 2]);
  await page.setViewportSize({ width: 375, height: 800 });
  await page.getByRole('button', { name: 'Previous files' }).click();
  await expect(page.getByRole('region', { name: 'Diff for guide.md' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'Getting started' })).toBeVisible();
  await page.keyboard.press('2');
  await expect(page.getByRole('radio', { name: '2 hops' })).toBeChecked();
  expect(requests.scenes).toBe(1);
});

test('offline PRs preserve graph status and can refresh live data', async ({ page }) => {
  const requests = await workspace(page);
  requests.online = false;
  await page.goto('/#/n/' + encodeURIComponent(pr.id));
  await expect(page.getByText('Could not load live PR status')).toBeVisible();
  await expect(page.getByText(/Last graph update.*open/)).toBeVisible();
  await expect(page.getByText('Could not load changed files')).toBeVisible();
  requests.online = true;
  await page.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(page.getByText('Merged', { exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Diff for guide.md' })).toContainText('-old title');
  await page.getByRole('radio', { name: 'Connections (0)' }).click();
  await expect(page.getByText('No connections yet', { exact: true })).toBeVisible();
});
