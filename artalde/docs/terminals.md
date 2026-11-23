# Terminals

artalde embeds terminals. The emulator is Ghostty's (`libghostty-vt`); the
programs belong to a small host process, so they outlive the window.

```
artalde (window)                artalde host (detached, one per state dir)
  term_view  ── Vt (draw) ◄──┐    Terminal: PTY + child + Vt (headless)
  terms      ── Client ──────┴──► unix socket  <state>/host.sock
artalde run|ls|read|send|kill ──►   (same protocol; agents use it too)
```

## Crates

| Crate | Owns |
|---|---|
| `artalde-vt` | Building and binding `libghostty-vt`; the safe `Vt` type. The only crate with emulator FFI. |
| `artalde-host` | PTYs, the host daemon, the wire protocol and its client. No UI. |
| `artalde` | The egui terminal widget, panes, and the CLI verbs. |

## Building the emulator

`artalde-vt/build.rs` runs Zig 0.16 on the pinned submodule `vendor/ghostty`
(`git submodule update --init --depth 1 artalde/vendor/ghostty`):

```
zig build -Demit-lib-vt -Demit-xcframework=false -Doptimize=ReleaseFast \
  -Dtarget=<zig triple> --prefix $OUT_DIR/zig-out --cache-dir $OUT_DIR/zig-cache
```

- `ZIG` overrides the binary, `GHOSTTY_SRC` the source directory.
- `GHOSTTY_VT_LIB_DIR` points at a prebuilt `libghostty-vt.a` and skips Zig.
- Zig downloads Ghostty's own dependencies on the first build. If a mirror is
  down, fetch the tarball by any means and `zig fetch <file>` from the
  package's directory; the hash is checked either way.

## In the window

- Panes are a binary split tree (`artalde-core/src/panes.rs`), kept in
  `<state>/panes.json`. They live in the layout's `terminal` block when the
  layout has one, otherwise in terminal tabs. A session tab shows the
  terminal its agent runs in.
- Keys (template `keys.json`): ⌘T new, ⌘D / ⇧⌘D split right / down,
  ⌥⌘arrows focus, ⇧⌘↵ zoom, ⌘F find, ⌘C / ⌘V copy / paste. Bound chords
  go to artalde; everything else goes to the program.
- Settings: `terminal` (`embedded` or `iterm`), `terminal_font_size`,
  `option_as_alt`, `shell`.
- A program sees `TERM=xterm-256color`, `COLORTERM=truecolor`,
  `TERM_PROGRAM=artalde`, `ARTALDE_TERMINAL=<id>` and `ARTALDE_HOST_SOCKET`.
- Text is JetBrains Mono, then a Nerd Font found in the user's font
  directories, then Menlo / DejaVu. Box-drawing and block characters
  (U+2500–U+259F) are drawn as shapes (`term_sprites.rs`) so borders join.
- `artalde drive` logs terminal commands instead of running them;
  `ARTALDE_DRIVE_TERMINALS=1` gives the run a private host under its state
  directory, for screenshots of real programs.
- Only macOS has been exercised. Linux builds are expected to work but the
  key handling and font fallbacks are untested there.

## Host

- Single instance: `flock` on `<state>/host.lock`. A client that cannot
  connect takes the lock, spawns `artalde host` detached (`setsid`, stdio to
  `/dev/null`, stderr to `<state>/host.log`, a scrubbed environment) and
  polls the socket.
- One reader thread per PTY that always drains; one writer thread per client;
  no socket or PTY IO under the state lock.
- Every terminal has a headless `Vt`. Attaching sends `CAN`, a repaint made by
  the emulator (screen, modes, optional scrollback), then live bytes. A client
  more than 4 MiB behind has its backlog replaced by a fresh repaint.
- The PTY's size follows the client that last typed, resized or focused.
- Kill is `HUP`, `TERM`, `KILL` on the process group. Exit is reported after
  the output has drained.
- The registry (`<state>/terminals.json`) keeps finished terminals for a day.

## Status

Two sources, merged by the app:

1. The hook (`artalde hook`) — Claude Code. A terminal the host starts has
   `ARTALDE_TERMINAL=<id>` in its environment; the hook stores it on the
   session record, which is how a session finds its pane.
2. `OSC 7501` — any program. The emulator parses it; the host keeps the
   state and counts turns.

## Wire protocol

Frames are `[kind u8][terminal u32 LE][len u32 LE][payload]`, payload at most
16 MiB. `terminal` is 0 for control frames. Control payloads are JSON.

| Kind | Dir | Payload |
|---|---|---|
| 1 `REQUEST` | c→h | `{"id": n, "method": "...", "params": {...}}` |
| 2 `REPLY` | h→c | `{"id": n, "result": ...}` or `{"id": n, "error": "..."}` |
| 3 `EVENT` | h→c | `{"type": "...", "terminal": n, ...}` after `subscribe` |
| 10 `OUTPUT` | h→c | bytes for the emulator |
| 11 `INPUT` | c→h | bytes for the PTY |
| 12 `RESIZE` | c→h | `cols u16, rows u16, cell_w u16, cell_h u16` (LE) |
| 13 `DETACH` | c→h | empty |
| 14 `ENDED` | h→c | `{"code": n|null, "signal": n|null}` |

Methods: `hello`, `run`, `list`, `get`, `attach`, `read`, `send`, `kill`,
`remove`, `rename`, `label`, `subscribe`, `shutdown`.
