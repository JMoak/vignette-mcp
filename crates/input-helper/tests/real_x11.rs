//! Real input into an X display. CI runs these under Xvfb, a display with no input devices of its
//! own. Each is ignored, and each also refuses any display that has real input devices unless the
//! machine is opted in with `KW_INPUT_HELPER_REAL_INPUT=yes`:
//!
//! ```sh
//! xvfb-run -a -s "-screen 0 1280x1024x24 -noreset" cargo test -p input-helper --test real_x11 -- --ignored --test-threads=1 --skip reports_the_keeper
//! ```
//!
//! `-noreset` matters: an X server starts itself over when its last client leaves and refuses
//! whoever connects in that moment, and these tests connect and leave one after another.
//!
//! `reports_the_keeper_s_own_input` needs a person at a real X desktop.

#![cfg(any(target_os = "linux", check_foreign))]

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::{Helper, grant, kind};
use input_helper::backend::{Act, Backend};
use input_helper::json;
use input_helper::screen::{self, Monitor};
use input_helper::x11::X11;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

const OPT_IN: &str = "KW_INPUT_HELPER_REAL_INPUT";
/// Left Shift's X keycode: evdev 42, plus 8.
const LEFT_SHIFT: usize = 50;

struct Display {
    connection: RustConnection,
    root: u32,
}

/// The X display these tests may use: Xvfb (its only devices are its own and the core's), or any
/// display on a machine opted in.
fn allowed() -> Display {
    let (connection, screen) = x11rb::connect(None).expect("an X display: run under xvfb-run");
    let root = connection.setup().roots[screen].root;
    let devices =
        connection.xinput_xi_query_device(xinput::Device::ALL).expect("XInput").reply().expect("devices");
    let names: Vec<String> =
        devices.infos.iter().map(|device| String::from_utf8_lossy(&device.name).into_owned()).collect();
    let virtual_only = names.iter().all(|name| name.starts_with("Virtual core") || name.starts_with("Xvfb "));
    let opted_in = std::env::var(OPT_IN).is_ok_and(|said| said == "yes");
    assert!(
        virtual_only || opted_in,
        "this X display has input devices of its own ({names:?}): set {OPT_IN}=yes only where injecting is allowed"
    );
    Display { connection, root }
}

impl Display {
    fn pointer(&self) -> (i32, i32) {
        let pointer = self.connection.query_pointer(self.root).expect("query").reply().expect("pointer");
        (i32::from(pointer.root_x), i32::from(pointer.root_y))
    }

    fn left_shift_is_down(&self) -> bool {
        let keymap = self.connection.query_keymap().expect("query").reply().expect("keymap");
        keymap.keys[LEFT_SHIFT / 8] & (1 << (LEFT_SHIFT % 8)) != 0
    }
}

fn system() -> X11 {
    let (events, _inbox) = mpsc::channel();
    X11::open(events).expect("an X display with XTest")
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

fn first_monitor() -> Monitor {
    system().monitors().into_iter().next().expect("a monitor")
}

fn granted() -> Helper {
    let rect = first_monitor().rect;
    let display = format!(
        r#"{{"x":{},"y":{},"width":{},"height":{},"scaleFactor":1}}"#,
        rect.x, rect.y, rect.width, rect.height
    );
    let mut helper = Helper::injecting();
    assert!(helper.greet().contains(r#""kind":"hello""#));
    helper.send(&grant(60_000.0, &display));
    assert!(
        helper.reply().is_some_and(|line| line.contains(r#""kind":"granted""#)),
        "the helper takes the grant"
    );
    helper
}

#[test]
#[ignore = "moves the X display's pointer; refuses any display but Xvfb unless KW_INPUT_HELPER_REAL_INPUT=yes"]
fn a_move_lands_on_the_pixel_it_names_on_every_monitor() {
    let display = allowed();
    let mut backend = system();
    for monitor in backend.monitors() {
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.5, 0.5), (0.0, 1.0), (1.0, 1.0)] {
            let spot = screen::spot_on(&monitor.rect, x, y);
            backend.send(&[Act::Move(spot)]).expect("the server takes the move");
            let expected = (spot.x.floor() as i32, spot.y.floor() as i32);
            assert!(
                eventually(|| display.pointer() == expected),
                "{monitor:?} at {x},{y}: {:?}",
                display.pointer()
            );
        }
    }
}

#[test]
#[ignore = "holds and lets go of the X display's left Shift; refuses any display but Xvfb unless KW_INPUT_HELPER_REAL_INPUT=yes"]
fn a_key_it_holds_is_let_go_when_its_input_closes() {
    let display = allowed();
    let mut helper = granted();
    helper.send(r#"{"v":1,"kind":"key","grantId":"g1","seq":1,"code":"ShiftLeft","down":true}"#);
    assert_eq!(helper.reply().as_deref(), Some(r#"{"v":1,"kind":"applied","seq":1}"#));
    assert!(eventually(|| display.left_shift_is_down()));
    helper.close_input();
    assert_eq!(helper.exit_code(), Some(0));
    assert!(eventually(|| !display.left_shift_is_down()), "left Shift is still down");
}

#[test]
#[ignore = "moves the X display's pointer; refuses any display but Xvfb unless KW_INPUT_HELPER_REAL_INPUT=yes"]
fn its_own_input_is_never_taken_for_the_keeper_s() {
    let _display = allowed();
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
#[ignore = "needs a person at a real X desktop to move its mouse or press a key; needs KW_INPUT_HELPER_REAL_INPUT=yes"]
fn reports_the_keeper_s_own_input() {
    let _display = allowed();
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
