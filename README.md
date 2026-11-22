# Vignette

The stage for panes that stand outside an app's window.

When an agent sets things out in real frameless windows, something has to move those windows
together: open them at once, glide them in step with the display, keep them from drifting or
flickering. Vignette is that part, in Rust. Keywork-app, which builds Stead and Keywork, is its
first user.

The words an agent uses for this (`show`, `arrange`, seats, moods, marks) and what is drawn inside
a pane are not here. They belong to Keywork's engine and to the app.

## What exists

- **`crates/panes-mover`** builds `kw-panes-mover`, a sidecar that moves its parent app's pane
  windows in one batch a frame. It touches only its parent's windows, changes their position, size
  and order only, never shows, hides or focuses one, and has no file or network access. It can
  play a whole arrangement from one line: each window with its own frame, delay and duration.
  [Its README](crates/panes-mover/README.md) has the protocol. It came from the `keywork-rust`
  repository on 2026-11-22 with its bench.
- **`vignette demo`** (`crates/core`, `crates/host`, `crates/vignette`) is a demo and test double:
  it opens up to sixteen panes natively, arranges them three ways and reports timings.

Not built yet: a pane's first showing as part of a play (Keywork-app still shows and moves those
one cue at a time), restacking through the mover from the app, and Linux (X11).

Each release publishes `kw-panes-mover-windows-x64.exe` with its SHA-256. Keywork-app pins a
release, checks the file against that checksum and bundles it.

## Measured

The mover, against Electron 45 on Windows 11, a 60 Hz display at 200% scale, 20 November 2026,
sixteen windows gliding together (full method and limits in `bench/` and the crate's tests):

| | Moved from JavaScript | Moved by the mover in one batch |
|---|---|---|
| Frames in which all sixteen windows landed in the same refresh | 253 of 559 (45%) | 388 of 388 |
| Load on the app's main process during a glide | about 80% of a core | about 15% |

A busy main thread still stalls every way alike: holding it for 100 ms delayed a window by about
110 ms whichever way it was moved.

The same comparison in the real app, with Keywork-app's own harness (`perf:panes-outside`), one
run each on 22 November 2026 with the mover built from this repository:

| | Moved from JavaScript | Moved by the mover |
|---|---|---|
| Frames in which all sixteen panes moved inside one refresh | 20 of 60 (33%) | 52 of 52 |
| First pane to last within a frame | 10.8 ms in the median frame | 0.1 ms |
| Load on the app's main process during the glide | 89% of a core | 18%, plus 5% in the mover |

The native demo, one run on 2026-11-21 on the same machine (release build):

| What | Result |
|---|---|
| First pane on screen | 27 ms after the program started |
| Twelve panes on screen | 55 ms after the program started |
| Twelve panes re-arranged (move and resize) | 36 frames, none late |

These are single-machine runs at one refresh rate; no frames were captured from the display itself.

## Try it

```powershell
cargo build --release
target\release\vignette.exe demo 12 playful --hold 1500
```

## Licence

MIT.
