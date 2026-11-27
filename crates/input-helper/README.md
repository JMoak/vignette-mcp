# kw-input-helper

A small program the app starts while a remote keeper controls this machine. It turns the
keeper's pointer and keys, sent by the app under a live grant, into real input here, and tells the
app the moment someone at this machine uses its own keyboard or mouse, so control can pause.
It reads one JSON object a line on standard input, writes one a line on standard output, and exits
when its input closes.

Keywork-app's console drives it through `screen/helper/port.ts`, exactly as it drives its
TypeScript stand-in.

## What it may do

- It injects nothing without a live grant from the app: one display, until a time. Input for
  another grant, after the grant's time or before any grant is refused by name.
- It lets go of every key and button it holds when the session ends, when its input closes, when
  the grant runs out or is replaced, on `release-all`, and on Ctrl+C, hang-up or terminate. A
  process killed outright cannot do this; the app sends `release-all` and `bye` before it kills.
- It lifts only keys and buttons it is holding, so it never lifts one the person here is holding.
- It watches the machine's input only to ask "did this helper inject it?". It never looks at
  which key or where. Anything it did not inject, including another program's injected input,
  counts as someone here.
- No UIAccess, no service, never the secure desktop. Keys that act on the machine rather than on
  what is in front of the keeper (power, sleep, media, browser and launch keys) are not offered.
- Its own code opens no file and no network connection (`clippy.toml` holds it to that). On Linux
  `x11rb` reads the X authority file and connects to the X display, and nothing else.
- Pointer moves and wheel turns are limited to 500 a second (bursts of 100), presses to 50 a
  second (bursts of 25). A release is never limited.

## Platforms

| | Injects with | Notices the keeper's own input with |
|---|---|---|
| Windows | `SendInput`, absolute virtual-desktop coordinates, per-monitor DPI aware (v2); keys by scan code | low-level mouse and keyboard hooks, telling its own events by their injected flag and its mark |
| Linux, X11 | XTest, through the X protocol itself (`x11rb`, no C library) | XInput 2 raw events from any device but XTest's own |
| macOS | `CGEvent`, after the Accessibility check | a listen-only event tap, telling its own events by their mark |

Wayland is not supported yet. Keys are pressed by their place on the keyboard
(`KeyboardEvent.code`), so this machine's layout decides the character.

## Protocol, version 1

Every line carries `"v":1`. Until `hello` succeeds, everything else is refused with `version`.

| Request | What it does |
|---|---|
| `{"kind":"hello","versions":[1]}` | Answered `hello` with `version` 1 and `impl`. A list without 1 is refused, then the helper leaves with code 2. |
| `{"kind":"grant","grantId","expiresAt","display":{x,y,width,height,scaleFactor}}` | Holds this grant until `expiresAt` (milliseconds since 1970). `display` is the captured display as Electron gives it; an optional `physical: {x,y,width,height}` gives the same display in the system's pixels. Answered `granted`. A new grant id lets go of what the old one held. |
| `{"kind":"ptr","grantId","seq","action","x","y","button"?,"dx"?,"dy"?}` | `move`, `down`, `up` or `wheel` at `x`,`y` from 0 to 1 across the granted display. `dx`,`dy` are browser wheel pixels. |
| `{"kind":"key","grantId","seq","code","down"}` | A key by its `KeyboardEvent.code`. |
| `{"kind":"release-all"}` | Lets go of everything it holds; answered `released` with what that was. |
| `{"kind":"bye"}` | Lets go of everything, says `bye` and leaves. |

| Reply | When |
|---|---|
| `applied` with `seq` | The input went to the system. |
| `refused` with `reason`, and `seq` for input | `no-grant`, `wrong-grant`, `expired`, `version`, `malformed`; and four the TypeScript stand-in never sends: `display` (the grant names a display this machine does not have), `access` (the system will not let the helper inject or watch input: on macOS, Accessibility is not granted), `rate` (over the limits above) and `blocked` (the system did not take this input). |
| `released` with `keys` and `buttons` | After `release-all`, a replaced grant, and on the way out. |
| `local-input` | Someone at this machine used its keyboard, mouse, touchpad or touch screen while a grant is held. At most every 250 ms, except at once after the helper has sent input. |
| `bye` | Last, before it leaves. |

## Finding the display

On macOS Electron's bounds are the system's own points, and on X11 every display shares one scale,
so the display is found by its place and size. On Windows Electron lays displays of different
scales out in its own units, which the system's pixels cannot be turned back into: the display is
found by its size and scale, and two alike are refused with `display` unless the app sends
`physical`. Each press looks again, so a display that has changed or gone is never pressed blind.

## Checking it without real input

```powershell
cargo test -p input-helper                  # unit tests, and the built helper run --dry-run
```

`kw-input-helper --dry-run` speaks the whole protocol against one 1920 by 1080 display and injects
and watches nothing. The tests run Keywork-app's shared consent table
(`tests/consent-rules.json`, a byte-for-byte copy of its `fixtures/screen/consent-rules.json`)
against it as a process.

`RUSTFLAGS="--cfg check_foreign" cargo clippy -p input-helper --all-targets` compiles and lints the
X11 and macOS backends on any host, without their targets installed.

`tests/real_windows.rs` and `tests/real_x11.rs` inject real input. They are ignored, refuse to run
unless `KW_INPUT_HELPER_REAL_INPUT=yes` (the X11 ones also run on Xvfb, which CI uses), and say
how to run them at their top.
