# Bundled typefaces

All three families are licensed under the SIL Open Font License 1.1. They are
shipped once, from azala, by adar, taula and lemazain.

- **Bricolage Grotesque** (Bold, ExtraBold) — Ateliertriay. https://github.com/ateliertriay/bricolage
- **Instrument Sans** (Regular, Medium, SemiBold, Bold) — Instrument. https://github.com/Instrument/instrument-sans
- **JetBrains Mono** (Regular, Medium) — JetBrains. https://github.com/JetBrains/JetBrainsMono

Each weight ships as its own family name, so weight resolution is exact and
never silently falls back.

Copied from azala (the design system adar, taula and lemazain share), so artalde reads like them.

## Icons

- **Lucide** — ISC License (`LICENSE-Lucide.txt`). https://lucide.dev
  `lucide.ttf` is a subset holding only the glyphs in `icons.txt`; add a name
  there and run `python3 tools/icons.py <lucide dir>`, which rebuilds the
  subset and `src/icons.rs`.
