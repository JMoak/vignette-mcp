# Vignette

Vignette is the stage: the Rust side of panes that stand outside an app's window. Its job is
motion. It moves many frameless windows together, in step with the display, so they never drift
apart or flicker. Keywork-app (Stead and Keywork) is its first user. `README.md` says what exists
and what was measured. It also holds the app's other native helper, the input helper for remote
control.

What Vignette does not own: the words an agent uses (`show`, `arrange`, seats, moods, marks) and
their answers live in Keywork's engine, in TypeScript, in one copy. What is drawn inside a pane is
the app's. Do not grow a second copy of either here. Who may control a machine, and for how long,
is the app's grant book; the input helper only holds to the grant it is given.

## Layout

| Path | What it is |
|---|---|
| `crates/panes-mover` | The stage today: `kw-panes-mover`, a sidecar that moves its parent app's pane windows in one batch a frame. `protocol` (text in, text out), `lines`, `easing`, `tracks`, `stats`, `mover` (the timing thread), `win32` (every Win32 call). `tests/sidecar.rs` runs the built helper against real hidden windows. |
| `crates/input-helper` | `kw-input-helper`, the remote-control input helper Keywork-app's screen console drives. `protocol` and `json` (the console's JSON lines, exactly), `session` (grants, held keys, release, preemption, every decision), `screen` (finding the display, coordinates), `keys`, `limits`, and one backend each behind `backend::Backend`: `win32`, `x11`, `macos`, plus `posix` for stop signals. `tests/dry_run.rs` runs the built helper `--dry-run` with the console's shared consent table. |
| `bench/` | The Electron bench that produced the mover's measurements. |
| `crates/core`, `crates/host`, `crates/vignette` | A demo and test double: `vignette demo` opens real panes natively, arranges them and reports timings. It exists to measure and to give the stage windows to move in tests without an app. Its seats, moods and markdown are a small copy for the demo only; do not extend them. |

## Rules

- **Supply chain.** The mover's only dependency is Microsoft's `windows-sys`, pinned exactly; it
  brings `windows-link` from the same repository. Elsewhere a well-known crate may be added when it
  saves real work: pin the exact version, review `Cargo.lock` like source, and keep `crates/core`
  free of dependencies. Jordan decides each addition.
- **The mover's safety properties** hold and stay tested: it touches only its parent's windows,
  changes their position, size and order only (never visibility, focus, frame or an owner), has
  no file or network access (`crates/panes-mover/clippy.toml` and a manifest test enforce it),
  cannot outlive the app, is per-monitor DPI aware, and costs nothing when idle. `glide` changes
  position only; size and order change only through `play` and `stack`
  (`crates/panes-mover/README.md` has the protocol).
- **Never takes focus.** Every window call carries the no-activate flag.
- **The input helper's safety properties** hold and stay tested: it injects nothing without a
  live grant, lifts only what it holds and lets go of everything whenever a session ends, asks
  the system only whether input was its own (never which key or where), never uses UIAccess, a
  service or the secure desktop, and has no file or network access of its own. Its dependencies
  are `windows-sys` on Windows and `x11rb` (the X protocol in Rust, no C library) on Linux, both
  pinned exactly; macOS calls CoreGraphics directly.
- **Never send input to the machine you work on, and never watch its input.** Every test of the
  helper drives a fake backend or the built helper `--dry-run`. `tests/real_*.rs` inject real
  input: they are ignored, refuse to run without `KW_INPUT_HELPER_REAL_INPUT=yes` (the X11 ones
  also run on Xvfb), and are run only on a machine where that is allowed.
- **`unsafe` lives only in the system-facing files**: a `win32.rs`, the helper's `macos.rs` and
  `posix.rs`, and tests. One call to a block where possible.
- **One easing.** Everything eases with the fifth-power curve the app's own glides use.
- Leave changes uncommitted unless asked to commit. Do not push.

## Commands

Rust lives in `%USERPROFILE%\.cargo\bin`; put it on `PATH` first.

```powershell
cargo test                                   # unit tests, and the sidecar tests against real hidden windows
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build --release                        # target\release\kw-panes-mover.exe, kw-input-helper.exe and vignette.exe
target\release\vignette.exe demo 12 playful --hold 1500
$env:RUSTFLAGS = "--cfg check_foreign"; cargo clippy -p input-helper --all-targets -- -D warnings; $env:RUSTFLAGS = ""
                                             # the helper's X11 and macOS backends, compiled and linted on any host
```

Only the input helper builds on Linux and macOS: `cargo test -p input-helper` there.
