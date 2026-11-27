//! macOS, through CoreGraphics directly: `CGEvent`s posted to the session, behind the
//! Accessibility check, and a listen-only event tap that notices events the helper did not post.
//! Positions are global display points, the same units Electron gives the console.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread;
use std::time::Instant;

use crate::backend::{Act, Backend, Event, Fault, Notches};
use crate::keys::{Key, Modifier};
use crate::protocol::{Button, Rect};
use crate::screen::Monitor;

type Ref = *mut c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

type TapCallback = unsafe extern "C" fn(proxy: Ref, kind: u32, event: Ref, user: *mut c_void) -> Ref;

#[cfg_attr(target_os = "macos", link(name = "ApplicationServices", kind = "framework"))]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[cfg_attr(target_os = "macos", link(name = "CoreGraphics", kind = "framework"))]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> Ref;
    fn CGEventCreateMouseEvent(source: Ref, kind: u32, at: CGPoint, button: u32) -> Ref;
    fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    fn CGEventCreateScrollWheelEvent2(
        source: Ref,
        units: u32,
        count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> Ref;
    fn CGEventSetIntegerValueField(event: Ref, field: u32, value: i64);
    fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    fn CGEventSetFlags(event: Ref, flags: u64);
    fn CGEventPost(tap: u32, event: Ref);
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: TapCallback,
        user: *mut c_void,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
}

#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
unsafe extern "C" {
    static kCFRunLoopCommonModes: Ref;
    fn CFRelease(object: Ref);
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRun();
}

const HID_SYSTEM_STATE: i32 = 1;
const HID_EVENT_TAP: u32 = 0;
const SESSION_EVENT_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const LISTEN_ONLY: u32 = 1;
const SCROLL_IN_PIXELS: u32 = 0;
const MOUSE_CLICK_STATE: u32 = 1;
const SOURCE_USER_DATA: u32 = 42;

const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SCROLL_WHEEL: u32 = 22;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xffff_fffe;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xffff_ffff;

const SHIFT: u64 = 0x0002_0000;
const CONTROL: u64 = 0x0004_0000;
const ALTERNATE: u64 = 0x0008_0000;
const COMMAND: u64 = 0x0010_0000;

/// Carried by every event the helper posts, so its tap can tell its own input from anyone else's.
const OURS: i64 = 0x4b57_4948;
/// macOS counts a second press as a double click within this long and this near, by default.
const DOUBLE_CLICK_MS: f64 = 500.0;
const DOUBLE_CLICK_POINTS: f64 = 4.0;
const LARGEST_DISPLAY_COUNT: usize = 32;

static WATCHER: OnceLock<Sender<Event>> = OnceLock::new();
static TAP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

pub struct Mac {
    source: Ref,
    events: Sender<Event>,
    watching: bool,
    started: Instant,
    pointer: Pointer,
    modifiers: Vec<&'static Key>,
    vertical: Notches,
    horizontal: Notches,
}

impl Mac {
    pub fn open(events: Sender<Event>) -> Result<Mac, &'static str> {
        let source = unsafe { CGEventSourceCreate(HID_SYSTEM_STATE) };
        if source.is_null() {
            return Err("cannot create an event source");
        }
        Ok(Mac {
            source,
            events,
            watching: false,
            started: Instant::now(),
            pointer: Pointer::default(),
            modifiers: Vec::new(),
            vertical: Notches::default(),
            horizontal: Notches::default(),
        })
    }

    fn post(&self, event: Ref) -> Result<(), Fault> {
        if event.is_null() {
            return Err(Fault::Blocked);
        }
        unsafe { CGEventSetIntegerValueField(event, SOURCE_USER_DATA, OURS) };
        unsafe { CGEventSetFlags(event, flags_of(&self.modifiers)) };
        unsafe { CGEventPost(SESSION_EVENT_TAP, event) };
        unsafe { CFRelease(event) };
        Ok(())
    }

    fn mouse(&self, kind: u32, button: u32, clicks: Option<i64>) -> Result<(), Fault> {
        let at = CGPoint { x: self.pointer.at.0, y: self.pointer.at.1 };
        let event = unsafe { CGEventCreateMouseEvent(self.source, kind, at, button) };
        if let (false, Some(clicks)) = (event.is_null(), clicks) {
            unsafe { CGEventSetIntegerValueField(event, MOUSE_CLICK_STATE, clicks) };
        }
        self.post(event)
    }

    fn key(&mut self, key: &'static Key, down: bool) -> Result<(), Fault> {
        let code = key.mac.ok_or(Fault::Blocked)?;
        if key.modifier.is_some() {
            self.modifiers.retain(|held| held.code != key.code);
            if down {
                self.modifiers.push(key);
            }
        }
        let event = unsafe { CGEventCreateKeyboardEvent(self.source, code, down) };
        self.post(event)
    }

    fn act(&mut self, act: Act) -> Result<(), Fault> {
        let now = self.started.elapsed().as_secs_f64() * 1000.0;
        match act {
            Act::Move(spot) => {
                self.pointer.at = (spot.x, spot.y);
                let (kind, button) = self.pointer.moving();
                self.mouse(kind, button, None)
            }
            Act::Press(button) => {
                let clicks = self.pointer.press(button, now);
                let (kind, number) = pressing(button);
                self.mouse(kind, number, Some(clicks))
            }
            Act::Release(button) => {
                let clicks = self.pointer.release(button);
                let (kind, number) = releasing(button);
                self.mouse(kind, number, Some(clicks))
            }
            Act::Wheel { dx, dy } => {
                let (vertical, horizontal) = (self.vertical.take(-dy), self.horizontal.take(-dx));
                if vertical == 0 && horizontal == 0 {
                    return Ok(());
                }
                let event = unsafe {
                    CGEventCreateScrollWheelEvent2(self.source, SCROLL_IN_PIXELS, 2, vertical, horizontal, 0)
                };
                self.post(event)
            }
            Act::KeyDown(key) => self.key(key, true),
            Act::KeyUp(key) => self.key(key, false),
        }
    }
}

impl Backend for Mac {
    fn monitors(&mut self) -> Vec<Monitor> {
        let mut displays = [0_u32; LARGEST_DISPLAY_COUNT];
        let mut count = 0_u32;
        let listed =
            unsafe { CGGetActiveDisplayList(displays.len() as u32, displays.as_mut_ptr(), &mut count) };
        if listed != 0 {
            return Vec::new();
        }
        let active = displays.get(..count as usize).unwrap_or(&[]);
        active
            .iter()
            .map(|display| {
                let bounds = unsafe { CGDisplayBounds(*display) };
                let rect = Rect {
                    x: bounds.origin.x,
                    y: bounds.origin.y,
                    width: bounds.size.width,
                    height: bounds.size.height,
                };
                Monitor { rect, scale: Some(1.0) }
            })
            .collect()
    }

    fn watch(&mut self) -> Result<(), Fault> {
        if !unsafe { AXIsProcessTrusted() } {
            return Err(Fault::Access);
        }
        if !self.watching {
            start_watching(self.events.clone())?;
            self.watching = true;
        }
        Ok(())
    }

    fn send(&mut self, acts: &[Act]) -> Result<(), Fault> {
        acts.iter().try_for_each(|act| self.act(*act))
    }
}

/// What the pointer is doing, which macOS needs told: a move with a button held is a drag, and a
/// quick second press in the same place is a double click.
#[derive(Debug, Default)]
struct Pointer {
    at: (f64, f64),
    held: Vec<Button>,
    last_press: Option<(Button, f64, (f64, f64))>,
    clicks: i64,
}

impl Pointer {
    fn moving(&self) -> (u32, u32) {
        match self.held.first() {
            None => (MOVED, 0),
            Some(Button::Left) => (LEFT_DRAGGED, 0),
            Some(Button::Right) => (RIGHT_DRAGGED, 1),
            Some(Button::Middle) => (OTHER_DRAGGED, 2),
        }
    }

    fn press(&mut self, button: Button, now: f64) -> i64 {
        let near = |(x, y): (f64, f64)| {
            (x - self.at.0).abs() <= DOUBLE_CLICK_POINTS && (y - self.at.1).abs() <= DOUBLE_CLICK_POINTS
        };
        let again = self
            .last_press
            .is_some_and(|(last, at, place)| last == button && now - at <= DOUBLE_CLICK_MS && near(place));
        self.clicks = if again { self.clicks + 1 } else { 1 };
        self.last_press = Some((button, now, self.at));
        if !self.held.contains(&button) {
            self.held.push(button);
        }
        self.clicks
    }

    fn release(&mut self, button: Button) -> i64 {
        self.held.retain(|held| *held != button);
        self.clicks.max(1)
    }
}

fn pressing(button: Button) -> (u32, u32) {
    match button {
        Button::Left => (LEFT_DOWN, 0),
        Button::Right => (RIGHT_DOWN, 1),
        Button::Middle => (OTHER_DOWN, 2),
    }
}

fn releasing(button: Button) -> (u32, u32) {
    match button {
        Button::Left => (LEFT_UP, 0),
        Button::Right => (RIGHT_UP, 1),
        Button::Middle => (OTHER_UP, 2),
    }
}

fn flags_of(modifiers: &[&Key]) -> u64 {
    modifiers.iter().fold(0, |flags, key| {
        flags
            | match key.modifier {
                Some(Modifier::Shift) => SHIFT,
                Some(Modifier::Control) => CONTROL,
                Some(Modifier::Alt) => ALTERNATE,
                Some(Modifier::Meta) => COMMAND,
                None => 0,
            }
    })
}

fn tell_physical() {
    if let Some(events) = WATCHER.get() {
        let _ = events.send(Event::Physical);
    }
}

unsafe extern "C" fn on_event(_proxy: Ref, kind: u32, event: Ref, _user: *mut c_void) -> Ref {
    if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER_INPUT {
        let tap = TAP.load(Ordering::Acquire);
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
    } else if !event.is_null() && unsafe { CGEventGetIntegerValueField(event, SOURCE_USER_DATA) } != OURS {
        tell_physical();
    }
    event
}

fn watched_kinds() -> u64 {
    let kinds = [
        LEFT_DOWN,
        LEFT_UP,
        RIGHT_DOWN,
        RIGHT_UP,
        MOVED,
        LEFT_DRAGGED,
        RIGHT_DRAGGED,
        KEY_DOWN,
        KEY_UP,
        FLAGS_CHANGED,
        SCROLL_WHEEL,
        OTHER_DOWN,
        OTHER_UP,
        OTHER_DRAGGED,
    ];
    kinds.iter().fold(0, |mask, kind| mask | 1 << kind)
}

fn start_watching(events: Sender<Event>) -> Result<(), Fault> {
    let _ = WATCHER.set(events);
    let (ready, started) = mpsc::sync_channel(1);
    thread::spawn(move || watch_until_exit(&ready));
    match started.recv() {
        Ok(true) => Ok(()),
        _ => Err(Fault::Access),
    }
}

fn watch_until_exit(ready: &SyncSender<bool>) {
    let tap = unsafe {
        CGEventTapCreate(HID_EVENT_TAP, HEAD_INSERT, LISTEN_ONLY, watched_kinds(), on_event, null_mut())
    };
    if tap.is_null() {
        let _ = ready.send(false);
        return;
    }
    TAP.store(tap, Ordering::Release);
    let source = unsafe { CFMachPortCreateRunLoopSource(null_mut(), tap, 0) };
    if source.is_null() {
        let _ = ready.send(false);
        return;
    }
    unsafe { CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes) };
    let _ = ready.send(true);
    unsafe { CFRunLoopRun() };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    #[test]
    fn a_move_with_a_button_held_is_that_button_s_drag() {
        let mut pointer = Pointer::default();
        assert_eq!(pointer.moving(), (MOVED, 0));
        pointer.press(Button::Middle, 0.0);
        assert_eq!(pointer.moving(), (OTHER_DRAGGED, 2));
        pointer.release(Button::Middle);
        pointer.press(Button::Left, 0.0);
        assert_eq!(pointer.moving(), (LEFT_DRAGGED, 0));
        pointer.release(Button::Left);
        assert_eq!(pointer.moving(), (MOVED, 0));
    }

    #[test]
    fn counts_a_quick_second_press_in_the_same_place_as_a_double_click() {
        let mut pointer = Pointer::default();
        assert_eq!(pointer.press(Button::Left, 0.0), 1);
        assert_eq!(pointer.release(Button::Left), 1);
        assert_eq!(pointer.press(Button::Left, 300.0), 2);
        assert_eq!(pointer.release(Button::Left), 2);
        assert_eq!(pointer.press(Button::Left, 600.0), 3);
        assert_eq!(pointer.press(Button::Left, 1200.0), 1);
        assert_eq!(pointer.press(Button::Right, 1300.0), 1);
        pointer.at = (10.0, 0.0);
        assert_eq!(pointer.press(Button::Right, 1400.0), 1);
    }

    #[test]
    fn carries_the_held_modifiers_on_every_event() {
        let named = |code| keys::named(code).expect("known");
        assert_eq!(flags_of(&[]), 0);
        assert_eq!(flags_of(&[named("ShiftRight"), named("MetaLeft")]), SHIFT | COMMAND);
        assert_eq!(flags_of(&[named("ControlLeft"), named("AltRight")]), CONTROL | ALTERNATE);
    }
}
