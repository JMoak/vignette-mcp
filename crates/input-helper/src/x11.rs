//! X11, spoken over the X protocol itself: XTest injects, XInput 2 raw events notice input that
//! came from any device but XTest's own. It needs no C library, so it builds for any Linux target.
//! Under Wayland, X11 programs cannot reach other windows' input; Wayland is not supported yet.

use std::sync::mpsc::Sender;
use std::thread;

use x11rb::CURRENT_TIME;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::backend::{Act, BROWSER_PIXELS_PER_NOTCH, Backend, Event, Fault, Notches};
use crate::protocol::{Button, Rect};
use crate::screen::Monitor;

/// X11 numbers keys from 8; the evdev driver, and so Xorg and Xwayland today, puts evdev's codes there.
const FIRST_KEYCODE: u16 = 8;
const WHEEL_UP: u8 = 4;
const WHEEL_DOWN: u8 = 5;
const WHEEL_LEFT: u8 = 6;
const WHEEL_RIGHT: u8 = 7;

pub struct X11 {
    connection: RustConnection,
    screen: usize,
    events: Sender<Event>,
    watching: bool,
    vertical: Notches,
    horizontal: Notches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fake {
    kind: u8,
    detail: u8,
    x: i16,
    y: i16,
}

impl X11 {
    pub fn open(events: Sender<Event>) -> Result<X11, &'static str> {
        let (connection, screen) =
            x11rb::connect(None).map_err(|_| "cannot reach the X display DISPLAY names")?;
        if connection.setup().roots.get(screen).is_none() {
            return Err("the X display has no such screen");
        }
        let xtest = connection.xtest_get_version(2, 2).ok().and_then(|cookie| cookie.reply().ok());
        if xtest.is_none() {
            return Err("the X display does not offer XTest");
        }
        // Monitors come from RandR 1.5 where the display has it, else the whole screen is one.
        let _ = connection.randr_query_version(1, 5).ok().and_then(|cookie| cookie.reply().ok());
        Ok(X11 {
            connection,
            screen,
            events,
            watching: false,
            vertical: Notches::default(),
            horizontal: Notches::default(),
        })
    }

    fn root(&self) -> Option<&xproto::Screen> {
        self.connection.setup().roots.get(self.screen)
    }
}

impl Backend for X11 {
    fn monitors(&mut self) -> Vec<Monitor> {
        let Some(root) = self.root() else { return Vec::new() };
        let whole = Monitor {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: root.width_in_pixels.into(),
                height: root.height_in_pixels.into(),
            },
            scale: None,
        };
        let listed =
            self.connection.randr_get_monitors(root.root, true).ok().and_then(|cookie| cookie.reply().ok());
        match listed {
            Some(reply) if !reply.monitors.is_empty() => reply
                .monitors
                .iter()
                .map(|monitor| Monitor {
                    rect: Rect {
                        x: monitor.x.into(),
                        y: monitor.y.into(),
                        width: monitor.width.into(),
                        height: monitor.height.into(),
                    },
                    scale: None,
                })
                .collect(),
            _ => vec![whole],
        }
    }

    fn watch(&mut self) -> Result<(), Fault> {
        if !self.watching {
            start_watching(self.events.clone())?;
            self.watching = true;
        }
        Ok(())
    }

    fn send(&mut self, acts: &[Act]) -> Result<(), Fault> {
        let root = self.root().map(|screen| screen.root).ok_or(Fault::Access)?;
        for fake in fakes_of(acts, &mut self.vertical, &mut self.horizontal) {
            let sent = self.connection.xtest_fake_input(
                fake.kind,
                fake.detail,
                CURRENT_TIME,
                root,
                fake.x,
                fake.y,
                0,
            );
            sent.map_err(|_| Fault::Blocked)?;
        }
        self.connection.flush().map_err(|_| Fault::Blocked)?;
        // An X server drops what a client sent if the client has gone before the server reads it,
        // and the helper leaves straight after letting go. A release must never be lost that way,
        // so wait for the server to answer once it has taken one.
        if acts.iter().any(|act| matches!(act, Act::Release(_) | Act::KeyUp(_))) {
            self.connection.sync().map_err(|_| Fault::Blocked)?;
        }
        Ok(())
    }
}

fn fakes_of(acts: &[Act], vertical: &mut Notches, horizontal: &mut Notches) -> Vec<Fake> {
    let mut fakes = Vec::new();
    let plain = |kind, detail| Fake { kind, detail, x: 0, y: 0 };
    for act in acts {
        match *act {
            Act::Move(spot) => {
                let (x, y) = (spot.x.floor() as i16, spot.y.floor() as i16);
                fakes.push(Fake { kind: xproto::MOTION_NOTIFY_EVENT, detail: 0, x, y });
            }
            Act::Press(button) => fakes.push(plain(xproto::BUTTON_PRESS_EVENT, number(button))),
            Act::Release(button) => fakes.push(plain(xproto::BUTTON_RELEASE_EVENT, number(button))),
            Act::Wheel { dx, dy } => {
                let down = vertical.take(dy / BROWSER_PIXELS_PER_NOTCH);
                let right = horizontal.take(dx / BROWSER_PIXELS_PER_NOTCH);
                for (steps, forward, back) in [(down, WHEEL_DOWN, WHEEL_UP), (right, WHEEL_RIGHT, WHEEL_LEFT)]
                {
                    let button = if steps > 0 { forward } else { back };
                    for _ in 0..steps.unsigned_abs() {
                        fakes.push(plain(xproto::BUTTON_PRESS_EVENT, button));
                        fakes.push(plain(xproto::BUTTON_RELEASE_EVENT, button));
                    }
                }
            }
            Act::KeyDown(key) => fakes.push(plain(xproto::KEY_PRESS_EVENT, keycode(key.evdev))),
            Act::KeyUp(key) => fakes.push(plain(xproto::KEY_RELEASE_EVENT, keycode(key.evdev))),
        }
    }
    fakes
}

fn number(button: Button) -> u8 {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    }
}

fn keycode(evdev: u16) -> u8 {
    u8::try_from(evdev + FIRST_KEYCODE).unwrap_or(0)
}

/// XTest's own devices, whose events are the helper's: every other device is someone at the machine.
fn xtest_sources<'a>(devices: impl Iterator<Item = (u16, &'a [u8])>) -> Vec<u16> {
    devices.filter(|(_, name)| name.windows(5).any(|part| part == b"XTEST")).map(|(id, _)| id).collect()
}

fn source_of(event: &XEvent) -> Option<u16> {
    match event {
        XEvent::XinputRawKeyPress(raw) => Some(raw.sourceid),
        XEvent::XinputRawButtonPress(raw) => Some(raw.sourceid),
        XEvent::XinputRawMotion(raw) => Some(raw.sourceid),
        XEvent::XinputRawTouchBegin(raw) => Some(raw.sourceid),
        _ => None,
    }
}

fn start_watching(events: Sender<Event>) -> Result<(), Fault> {
    let (connection, screen) = x11rb::connect(None).map_err(|_| Fault::Access)?;
    let root = connection.setup().roots.get(screen).map(|screen| screen.root).ok_or(Fault::Access)?;
    connection.extension_information(xinput::X11_EXTENSION_NAME).ok().flatten().ok_or(Fault::Access)?;
    let version = connection.xinput_xi_query_version(2, 2).ok().and_then(|cookie| cookie.reply().ok());
    if version.is_none_or(|version| version.major_version < 2) {
        return Err(Fault::Access);
    }
    let devices =
        connection.xinput_xi_query_device(xinput::Device::ALL).ok().and_then(|cookie| cookie.reply().ok());
    let devices = devices.ok_or(Fault::Access)?;
    let ours = xtest_sources(devices.infos.iter().map(|device| (device.deviceid, device.name.as_slice())));
    let mask = xinput::XIEventMask::RAW_KEY_PRESS
        | xinput::XIEventMask::RAW_BUTTON_PRESS
        | xinput::XIEventMask::RAW_MOTION
        | xinput::XIEventMask::RAW_TOUCH_BEGIN;
    let masks = [xinput::EventMask { deviceid: xinput::Device::ALL_MASTER.into(), mask: vec![mask] }];
    let selected =
        connection.xinput_xi_select_events(root, &masks).ok().and_then(|cookie| cookie.check().ok());
    selected.ok_or(Fault::Access)?;
    thread::spawn(move || {
        while let Ok(event) = connection.wait_for_event() {
            let physical = source_of(&event).is_some_and(|source| !ours.contains(&source));
            if physical && events.send(Event::Physical).is_err() {
                return;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use crate::screen::Spot;

    fn fakes(acts: &[Act]) -> Vec<(u8, u8)> {
        let (mut vertical, mut horizontal) = (Notches::default(), Notches::default());
        fakes_of(acts, &mut vertical, &mut horizontal).iter().map(|fake| (fake.kind, fake.detail)).collect()
    }

    #[test]
    fn presses_buttons_and_keys_by_their_x11_numbers() {
        let a = keys::named("KeyA").expect("known");
        let acts = [Act::Press(Button::Right), Act::Release(Button::Right), Act::KeyDown(a), Act::KeyUp(a)];
        assert_eq!(
            fakes(&acts),
            [
                (xproto::BUTTON_PRESS_EVENT, 3),
                (xproto::BUTTON_RELEASE_EVENT, 3),
                (xproto::KEY_PRESS_EVENT, 38),
                (xproto::KEY_RELEASE_EVENT, 38)
            ]
        );
    }

    #[test]
    fn turns_the_wheel_one_click_a_notch_in_its_direction() {
        let acts = [Act::Wheel { dx: -100.0, dy: 250.0 }];
        let clicks = [(xproto::BUTTON_PRESS_EVENT, WHEEL_DOWN), (xproto::BUTTON_RELEASE_EVENT, WHEEL_DOWN)];
        let left = [(xproto::BUTTON_PRESS_EVENT, WHEEL_LEFT), (xproto::BUTTON_RELEASE_EVENT, WHEEL_LEFT)];
        assert_eq!(fakes(&acts), [clicks, clicks, left].concat());
    }

    #[test]
    fn moves_to_the_root_window_pixel() {
        let acts = [Act::Move(Spot { x: 1919.7, y: -0.5 })];
        let (mut vertical, mut horizontal) = (Notches::default(), Notches::default());
        let moved = fakes_of(&acts, &mut vertical, &mut horizontal);
        assert_eq!(moved, [Fake { kind: xproto::MOTION_NOTIFY_EVENT, detail: 0, x: 1919, y: -1 }]);
    }

    #[test]
    fn treats_only_xtest_s_devices_as_the_helper_s() {
        let devices: [(u16, &[u8]); 6] = [
            (2, b"Virtual core pointer"),
            (3, b"Virtual core keyboard"),
            (4, b"Virtual core XTEST pointer"),
            (5, b"Virtual core XTEST keyboard"),
            (11, b"SynPS/2 Synaptics TouchPad"),
            (12, b"AT Translated Set 2 keyboard"),
        ];
        assert_eq!(xtest_sources(devices.into_iter()), [4, 5]);
    }
}
