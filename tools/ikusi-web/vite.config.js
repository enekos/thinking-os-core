import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [
    svelte(),
    {
      name: 'bundled-font-license',
      generateBundle: {
        order: 'post',
        handler(_, bundle) {
          const licenses = bundle['licenses.md'];
          if (licenses?.type !== 'asset') this.error('Missing dependency licenses');
          const ofl = readFileSync(
            new URL(import.meta.resolve('@fontsource-variable/dm-sans/LICENSE')),
            'utf8',
          );
          licenses.source += '\n## DM Sans (SIL Open Font License)\n\n' + ofl;
        },
      },
    },
  ],
  server: { proxy: { '/api': 'http://127.0.0.1:4747' } },
  build: {
    outDir: '../src/ikusi/web',
    emptyOutDir: true,
    assetsInlineLimit: 100000,
    license: { fileName: 'licenses.md' },
    rolldownOptions: { output: { entryFileNames: 'app.js', assetFileNames: 'app.[ext]' } },
  },
});
