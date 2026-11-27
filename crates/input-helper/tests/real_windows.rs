//! Real input on Windows: this machine's own pointer and keyboard. Never run these where someone
//! is working. Each is ignored, and each also refuses to start unless the machine is opted in:
//!
//! ```powershell
//! $env:KW_INPUT_HELPER_REAL_INPUT = "yes"
//! cargo test -p input-helper --test real_windows -- --ignored --test-threads=1
//! ```
//!
//! `reports_the_keeper_s_own_input` needs a person at the machine; the others need nobody to touch it.

#![cfg(windows)]

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::{Helper, grant, kind};
use input_helper::backend::{Act, Backend};
use input_helper::json;
use input_helper::screen::{self, Spot};
use input_helper::win32::Win32;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LSHIFT};
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

const OPT_IN: &str = "KW_INPUT_HELPER_REAL_INPUT";

fn allowed() {
    let opted_in = std::env::var(OPT_IN).is_ok_and(|said| said == "yes");
    assert!(
        opted_in,
        "this injects real input into this machine: set {OPT_IN}=yes only where that is allowed"
    );
}

fn system() -> Win32 {
    let (events, _inbox) = mpsc::channel();
    Win32::open(events).expect("per-monitor DPI awareness")
}

fn eventually(check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    check()
}

fn cursor() -> (i32, i32) {
    let mut at = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut at) };
    (at.x, at.y)
}

fn left_shift_is_down() -> bool {
    let state = unsafe { GetAsyncKeyState(i32::from(VK_LSHIFT)) };
    state as u16 & 0x8000 != 0
}

/// The primary display as Electron describes it, with its pixels.
fn primary_display() -> String {
    let monitors = system().monitors();
    let primary = monitors.iter().find(|monitor| monitor.rect.x == 0.0 && monitor.rect.y == 0.0);
    let primary = primary.expect("a primary display at the origin");
    let (scale, rect) = (primary.scale.unwrap_or(1.0), primary.rect);
    format!(
        r#"{{"x":0,"y":0,"width":{},"height":{},"scaleFactor":{scale},"physical":{{"x":0,"y":0,"width":{},"height":{}}}}}"#,
        (rect.width / scale).round(),
        (rect.height / scale).round(),
        rect.width,
        rect.height
    )
}

fn granted() -> Helper {
    let mut helper = Helper::injecting();
    assert!(helper.greet().contains(r#""kind":"hello""#));
    helper.send(&grant(60_000.0, &primary_display()));
    assert!(
        helper.reply().is_some_and(|line| line.contains(r#""kind":"granted""#)),
        "the helper takes the grant"
    );
    helper
}

#[test]
#[ignore = "moves this machine's pointer; needs KW_INPUT_HELPER_REAL_INPUT=yes and nobody touching it"]
fn a_move_lands_on_the_pixel_it_names_on_every_display() {
    allowed();
    let mut backend = system();
    let start = cursor();
    for monitor in backend.monitors() {
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.5, 0.5), (0.0, 1.0), (1.0, 1.0)] {
            let spot = screen::spot_on(&monitor.rect, x, y);
            backend.send(&[Act::Move(spot)]).expect("the system takes the move");
            let expected = (spot.x.floor() as i32, spot.y.floor() as i32);
            assert!(eventually(|| cursor() == expected), "{monitor:?} at {x},{y}: {:?}", cursor());
        }
    }
    let _ = backend.send(&[Act::Move(Spot { x: f64::from(start.0), y: f64::from(start.1) })]);
}

#[test]
#[ignore = "holds and lets go of this machine's left Shift; needs KW_INPUT_HELPER_REAL_INPUT=yes and nobody touching it"]
fn a_key_it_holds_is_let_go_when_its_input_closes() {
    allowed();
    let mut helper = granted();
    helper.send(r#"{"v":1,"kind":"key","grantId":"g1","seq":1,"code":"ShiftLeft","down":true}"#);
    assert_eq!(helper.reply().as_deref(), Some(r#"{"v":1,"kind":"applied","seq":1}"#));
    assert!(eventually(left_shift_is_down));
    helper.close_input();
    assert_eq!(helper.exit_code(), Some(0));
    assert!(eventually(|| !left_shift_is_down()), "left Shift is still down");
}

#[test]
#[ignore = "moves this machine's pointer; needs KW_INPUT_HELPER_REAL_INPUT=yes and nobody touching it"]
fn its_own_input_is_never_taken_for_the_keeper_s() {
    allowed();
    let mut helper = granted();
    for seq in 1..=20 {
        let x = f64::from(seq) / 20.0;
        helper.send(&format!(
            r#"{{"v":1,"kind":"ptr","grantId":"g1","seq":{seq},"action":"move","x":{x},"y":0.5}}"#
        ));
    }
    let mut replies = Vec::new();
    while let Some(line) = helper.reply_within(Duration::from_millis(500)) {
        replies.push(line);
    }
    assert_eq!(replies.iter().filter(|line| line.contains(r#""kind":"applied""#)).count(), 20);
    assert!(!replies.iter().any(|line| line.contains(r#""kind":"local-input""#)), "{replies:?}");
    helper.stop();
}

#[test]
#[ignore = "needs a person to move this machine's mouse or press a key when asked; needs KW_INPUT_HELPER_REAL_INPUT=yes"]
fn reports_the_keeper_s_own_input() {
    allowed();
    let helper = granted();
    eprintln!("move this machine's mouse, or press a key on its keyboard, within 15 seconds");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reported = false;
    while !reported && Instant::now() < deadline {
        let line = helper.reply_within(Duration::from_millis(200));
        reported =
            line.and_then(|line| json::parse(&line)).is_some_and(|reply| kind(&reply) == Some("local-input"));
    }
    assert!(reported, "no local-input came");
    helper.stop();
}
