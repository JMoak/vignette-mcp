# Vignette

Vignette is the stage: the Rust side of panes that stand outside an app's window. Its job is
motion. It moves many frameless windows together, in step with the display, so they never drift
apart or flicker. Keywork-app (Stead and Keywork) is its first user. `README.md` says what exists
and what was measured.

What Vignette does not own: the words an agent uses (`show`, `arrange`, seats, moods, marks) and
their answers live in Keywork's engine, in TypeScript, in one copy. What is drawn inside a pane is
the app's. Do not grow a second copy of either here.

## Layout

| Path | What it is |
|---|---|
| `crates/panes-mover` | The stage today: `kw-panes-mover`, a sidecar that moves its parent app's pane windows in one batch a frame. `protocol` (text in, text out), `lines`, `easing`, `tracks`, `stats`, `mover` (the timing thread), `win32` (every Win32 call). `tests/sidecar.rs` runs the built helper against real hidden windows. |
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
- **`unsafe` lives only in a `win32.rs`**, one call to a block where possible.
- **One easing.** Everything eases with the fifth-power curve the app's own glides use.
- Leave changes uncommitted unless asked to commit. Do not push.

## Commands

Rust lives in `%USERPROFILE%\.cargo\bin`; put it on `PATH` first.

```powershell
cargo test                                   # unit tests, and the sidecar tests against real hidden windows
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build --release                        # target\release\kw-panes-mover.exe and vignette.exe
target\release\vignette.exe demo 12 playful --hold 1500
```
