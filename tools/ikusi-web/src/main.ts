import { mount } from 'svelte';
import '@enekos/osagai/styles/theme.css';
import { registerIcons } from '@enekos/osagai';
registerIcons({
  network:
    'M8 7l8 3M7 9l3 7M16 13l-4 4M7 7a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 5 0zm14 4a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 5 0zm-8 8a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 5 0z',
  overview: 'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',
  layers: 'M12 3l9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5',
  bulb: 'M8.5 15a6 6 0 1 1 7 0L15 18H9zM10 21h4',
  checks: 'M3 6l1.7 1.7L8 4M11 6h10M3 13l1.7 1.7L8 11M11 13h10M11 19h10',
  keyboard: 'M2 5h20v14H2zM6 9h.1M10 9h.1M14 9h.1M18 9h.1M6 12h.1M10 12h.1M14 12h.1M18 12h.1M8 15h8',
  compass: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM16 8l-2.5 5.5L8 16l2.5-5.5z',
});
import App from './App.svelte';
import './theme.css';

const saved = localStorage.getItem('ikusi-theme');
document.documentElement.dataset.theme =
  saved === 'dark' || saved === 'light'
    ? saved
    : matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light';
const root = document.getElementById('app');
if (root) mount(App, { target: root });
