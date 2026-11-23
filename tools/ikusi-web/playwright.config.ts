import { defineConfig } from '@playwright/test';

const baseURL = 'http://127.0.0.1:14747';
export default defineConfig({
  testDir: './tests',
  use: { baseURL },
  webServer: {
    command: 'pnpm exec vite preview --host 127.0.0.1 --port 14747 --strictPort',
    url: baseURL,
  },
});
