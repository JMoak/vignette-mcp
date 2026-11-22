# kw-panes-mover

A small Windows program an app starts to move, size and order its own pane windows together, in
step with the display. It reads one command a line on standard input and writes one event a line
on standard output. It exits when its input closes or its parent process ends.

## What it may do

- It touches only top-level windows that belong to the process that started it. Any other window
  is refused by name.
- It changes a window's position, its size and what it stands under. It never shows, hides,
  activates or reframes a window, and never moves a window's owner.
- It has no file or network access, and it costs nothing while nothing is moving.

`glide` changes position only. Size and order change only through `play` and `stack`.

## Commands

Numbers are whole and in physical pixels. A window is its handle in hexadecimal.

| Command | What it does |
|---|---|
| `glide <id> <ms> <window>@<x>,<y>><x>,<y> ...` | Moves up to 64 windows from where their owner says they are to where they go, all setting off now and taking `<ms>`. |
| `play <id> <part> ...` | Plays up to 32 windows, each with its own frame, delay and duration. A part is `<window>@<x>,<y>,<w>,<h>><x>,<y>,<w>,<h>`, then optionally `+<delay ms>`, then optionally `~<duration ms>`, then optionally `!`. A window waits untouched until its delay has passed. Its size grows along the way, or with `!` changes at once as it sets off. A part whose size does not change never sizes its window. |
| `stack <base> <window> ...` | Orders up to 64 windows from the bottom up, directly above `<base>` and under whatever already stood above it. Done before the next command is read. |
| `release <window>` | Stops moving a window and leaves it where it is. |
| `border <window> <rrggbb>`, `border <window> none` or `border <window> clear` | Colours a window's frame, draws no frame at all, or gives the frame back the colour the system chooses (Windows 11; `none` since 0.3.0). Before Windows 11 it is refused with `border-failed`, and the window keeps the frame it has. |
| `stats` | Reports the frames issued since the last report. Its answer comes after every command before it has been taken up, so the pair is a round trip. |

A newer `glide` or `play` takes a window out of an older one and carries on from where that one
had brought it, including a size it was part-way through.

The first position (and size) in a target or part is what the app believes about the window.
The mover measures the difference from the window's real frame once and keeps it, so a window
whose app leaves out a border still lands where the app means.

## Events

| Event | When |
|---|---|
| `ready 1 <parent pid>` | Once, at start. |
| `settled <id>` | Every window of that glide or play has landed, been released or been taken by a newer one. |
| `refused <window> <reason>` | That window was left out of a command. The rest of the command goes ahead. |
| `refused - <reason>` | A line could not be read. Nothing was done. |
| `stats frames=<n> missed=<n> mean_us=<n> worst_us=<n>` | The answer to `stats`. |

## What sizing costs

Windows sizes a window while the call waits, so a frame that sizes many windows takes longer than
a frame that only moves them. Measured in Keywork-app on 22 November 2026 (60 Hz, sixteen real
panes, one run each): moving only, or sizing at once, 62 updates a second; growing on the way,
62 with four panes, 56 with eight and 30 with sixteen. In every case all the panes of a frame
changed inside one refresh. The app chooses `!` when many panes change size at once.

## How it keeps windows together

Each refresh it waits for the compositor, works out where every moving window should be on the
app's own easing curve, and issues all of them as one batch. A play's delays and durations are
counted on the mover's clock, so a staggered arrangement does not depend on the app's timers.
