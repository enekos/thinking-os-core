---
title: osagai
summary: "the owner's Svelte 5 UI kit (@enekos/osagai): zero-dep components, query()/task()/draft() helpers, --o-* theme tokens; default UI for new apps"
type: project
status: active
updated: 2026-11-17
---

# osagai

**One-liner.** A small, dependency-free component kit for Svelte 5 covering what
an app UI needs every day. Basque *osagai* = component. Public:
github.com/enekos/osagai · npm `@enekos/osagai`. Version at template time:
**0.9.0**.

Clone it and add `path:` to this page's frontmatter once it is on this machine.

## Setup

```sh
pnpm add @enekos/osagai
```

```svelte
<!-- +layout.svelte -->
<script lang="ts">
  import '@enekos/osagai/styles/theme.css';   // required: --o-* tokens, light + dark
  import '@enekos/osagai/styles/base.css';    // optional: element defaults + utilities
  import { Overlays } from '@enekos/osagai';
  let { children } = $props();
</script>
{@render children()}
<Overlays />                                   <!-- toasts + dialogs render here, mount once -->
```

## What to reach for

| Need | Use |
|---|---|
| Load data, show loading/error, poll | `query(() => api.get(…), { every })` + `QueryError`, `Skeleton`, `Empty` |
| A write with spinner + error toast | `task(fn, { success })` → `save.pending`, never throws |
| Unsaved changes | `draft(() => form)` → `d.dirty`, `d.commit()` |
| Ask / confirm | `await confirm({ title, action })`, `await prompt({ … })` |
| Feedback | `toast.ok / info / error(err)`, with an undo `action` |
| Keyboard | `shortcuts({ 'mod+k': … })`, `Kbd`, `CommandPalette` |
| Forms | `Field` (wires id/aria) + `Input` `Select` `Combobox` `DatePicker` `Checkbox` `Switch` `RadioGroup` `Choice` |
| Layout | `Page`, `Card`, `Tile`, `SplitView`, `Tabs`, `Segmented`, `Drawer`, `Modal`, `Popover` |
| Data display | `Table`, `DescriptionList`, `Stat`, `Badge`, `Progress`, `Meter`, `StackedBar`, `CodeView`, `Prose` |
| Bulk actions | `SelectionBar` |

The repo README is the full prop reference.

## Rules

- Use the kit before writing a component; missing piece → add it to osagai, not
  to the app.
- Style through `--o-*` tokens; app CSS only adds app variables.
- Accessibility (focus traps, roving tabindex, aria wiring) is the kit's job.
- A domain type named like a kit component (`Table`): import the domain one
  aliased (`type Table as TableMeta`).
- App architecture around it → [[ts-architecture]].

## Next up
- [ ] Clone it here and set `path:` if you'll build on or contribute to it.
