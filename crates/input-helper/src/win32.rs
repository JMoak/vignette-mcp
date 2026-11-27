//! Every Windows call the helper makes: `SendInput` with absolute virtual-desktop coordinates in a
//! per-monitor DPI aware (v2) process, low-level hooks that notice input the helper did not inject,
//! and the console's Ctrl+C. The hooks look only at whether an event was injected and by whom,
//! never at which key or where.

use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    GetThreadDpiAwarenessContext, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, GetSystemMetrics, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
};
use windows_sys::core::BOOL;

use crate::backend::{Act, BROWSER_PIXELS_PER_NOTCH, Backend, Event, Fault, Notches};
use crate::keys::Key;
use crate::protocol::{Button, Rect};
use crate::screen::{self, Monitor};

/// Carried by every event the helper injects, so its hooks can tell its own input from anyone else's.
const OURS: usize = 0x4b57_4948;
const WHEEL_DELTA: f64 = 120.0;

static WATCHER: OnceLock<Sender<Event>> = OnceLock::new();
static STOPPER: OnceLock<Sender<Event>> = OnceLock::new();

pub struct Win32 {
    events: Sender<Event>,
    watching: bool,
    wheels: Wheels,
}

#[derive(Default)]
struct Wheels {
    vertical: Notches,
    horizontal: Notches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Desk {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Win32 {
    pub fn open(events: Sender<Event>) -> Result<Win32, &'static str> {
        if !enable_per_monitor_dpi() {
            return Err("per-monitor DPI awareness (v2) is unavailable");
        }
        Ok(Win32 { events, watching: false, wheels: Wheels::default() })
    }
}

impl Backend for Win32 {
    fn monitors(&mut self) -> Vec<Monitor> {
        let mut found: Vec<Monitor> = Vec::new();
        let list = (&raw mut found) as LPARAM;
        unsafe { EnumDisplayMonitors(null_mut(), null(), Some(each_monitor), list) };
        found
    }

    fn watch(&mut self) -> Result<(), Fault> {
        if !self.watching {
            start_watching(self.events.clone())?;
            self.watching = true;
        }
        Ok(())
    }

    fn send(&mut self, acts: &[Act]) -> Result<(), Fault> {
        let inputs = inputs_of(acts, virtual_desk(), &mut self.wheels);
        if inputs.is_empty() {
            return Ok(());
        }
        let size = size_of::<INPUT>() as i32;
        let taken = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size) };
        if taken as usize == inputs.len() { Ok(()) } else { Err(Fault::Blocked) }
    }
}

/// Ctrl+C, Ctrl+Break or a closed console window stop the helper the way a closed input does,
/// letting go of everything first. Nothing can be done when it is terminated outright.
pub fn forward_stops(events: Sender<Event>) {
    if STOPPER.set(events).is_ok() {
        unsafe { SetConsoleCtrlHandler(Some(on_console_stop), 1) };
    }
}

unsafe extern "system" fn on_console_stop(kind: u32) -> BOOL {
    if !matches!(kind, CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT) {
        return 0;
    }
    if let Some(events) = STOPPER.get() {
        let _ = events.send(Event::Stop);
    }
    // The main thread lets go of everything and ends the process while this waits.
    thread::sleep(Duration::from_secs(2));
    1
}

fn inputs_of(acts: &[Act], desk: Desk, wheels: &mut Wheels) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(acts.len() + 1);
    for act in acts {
        match *act {
            Act::Move(spot) => {
                let x = screen::virtual_desk(spot.x.floor() as i32, desk.x, desk.width);
                let y = screen::virtual_desk(spot.y.floor() as i32, desk.y, desk.height);
                inputs.push(mouse(
                    x,
                    y,
                    0,
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                ));
            }
            Act::Press(button) => inputs.push(mouse(0, 0, 0, pressing(button))),
            Act::Release(button) => inputs.push(mouse(0, 0, 0, releasing(button))),
            Act::Wheel { dx, dy } => {
                let vertical = wheels.vertical.take(-dy / BROWSER_PIXELS_PER_NOTCH * WHEEL_DELTA);
                let horizontal = wheels.horizontal.take(dx / BROWSER_PIXELS_PER_NOTCH * WHEEL_DELTA);
                if vertical != 0 {
                    inputs.push(mouse(0, 0, vertical as u32, MOUSEEVENTF_WHEEL));
                }
                if horizontal != 0 {
                    inputs.push(mouse(0, 0, horizontal as u32, MOUSEEVENTF_HWHEEL));
                }
            }
            Act::KeyDown(key) => inputs.push(keyboard(key, false)),
            Act::KeyUp(key) => inputs.push(keyboard(key, true)),
        }
    }
    inputs
}

fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    let mi = MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: OURS };
    INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } }
}

// By place on the keyboard (a scan code), so this machine's layout decides the character; by
// virtual key only for the few keys whose scan codes Windows reads otherwise.
fn keyboard(key: &Key, up: bool) -> INPUT {
    let mut flags = if key.vk == 0 { KEYEVENTF_SCANCODE } else { 0 };
    if key.scan & 0xff00 == 0xe000 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    let ki = KEYBDINPUT { wVk: key.vk, wScan: key.scan & 0xff, dwFlags: flags, time: 0, dwExtraInfo: OURS };
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki } }
}

fn pressing(button: Button) -> u32 {
    match button {
        Button::Left => MOUSEEVENTF_LEFTDOWN,
        Button::Middle => MOUSEEVENTF_MIDDLEDOWN,
        Button::Right => MOUSEEVENTF_RIGHTDOWN,
    }
}

fn releasing(button: Button) -> u32 {
    match button {
        Button::Left => MOUSEEVENTF_LEFTUP,
        Button::Middle => MOUSEEVENTF_MIDDLEUP,
        Button::Right => MOUSEEVENTF_RIGHTUP,
    }
}

/// Input is the keeper's own unless the helper injected it: anything else, including another
/// program's injected input, counts as someone at this machine.
fn is_physical(injected: bool, extra: usize) -> bool {
    !(injected && extra == OURS)
}

fn tell_physical() {
    if let Some(events) = WATCHER.get() {
        let _ = events.send(Event::Physical);
    }
}

unsafe extern "system" fn on_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if is_physical(event.flags & LLMHF_INJECTED != 0, event.dwExtraInfo) {
            tell_physical();
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if is_physical(event.flags & LLKHF_INJECTED != 0, event.dwExtraInfo) {
            tell_physical();
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
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

// The hooks are called on this thread while it waits for messages, and go when the process does.
fn watch_until_exit(ready: &SyncSender<bool>) {
    let module = unsafe { GetModuleHandleW(null()) };
    let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), module, 0) };
    let keys = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module, 0) };
    let hooked = !mouse.is_null() && !keys.is_null();
    let _ = ready.send(hooked);
    if !hooked {
        for hook in [mouse, keys].into_iter().filter(|hook| !hook.is_null()) {
            unsafe { UnhookWindowsHookEx(hook) };
        }
        return;
    }
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {}
}

unsafe extern "system" fn each_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
    let found = unsafe { &mut *(list as *mut Vec<Monitor>) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi_x, mut dpi_y) = (0, 0);
    let known = unsafe { GetMonitorInfoW(monitor, &mut info) } != 0;
    let scaled = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } >= 0;
    if known && scaled && dpi_x > 0 {
        let area = info.rcMonitor;
        let rect = Rect {
            x: f64::from(area.left),
            y: f64::from(area.top),
            width: f64::from(area.right - area.left),
            height: f64::from(area.bottom - area.top),
        };
        found.push(Monitor { rect, scale: Some(f64::from(dpi_x) / 96.0) });
    }
    1
}

fn virtual_desk() -> Desk {
    let metric = |index| unsafe { GetSystemMetrics(index) };
    Desk {
        x: metric(SM_XVIRTUALSCREEN),
        y: metric(SM_YVIRTUALSCREEN),
        width: metric(SM_CXVIRTUALSCREEN),
        height: metric(SM_CYVIRTUALSCREEN),
    }
}

fn enable_per_monitor_dpi() -> bool {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let current = unsafe { GetThreadDpiAwarenessContext() };
    unsafe { AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 }
}

// These build the structures `SendInput` would be given and read them back; none is sent.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use crate::screen::Spot;

    const DESK: Desk = Desk { x: -2560, y: -300, width: 2560 + 3840, height: 2460 };

    fn mouse_of(input: &INPUT) -> MOUSEINPUT {
        assert_eq!(input.r#type, INPUT_MOUSE);
        unsafe { input.Anonymous.mi }
    }

    fn key_of(input: &INPUT) -> KEYBDINPUT {
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        unsafe { input.Anonymous.ki }
    }

    fn one(act: Act) -> INPUT {
        let inputs = inputs_of(&[act], DESK, &mut Wheels::default());
        assert_eq!(inputs.len(), 1);
        inputs[0]
    }

    #[test]
    fn a_move_is_absolute_across_the_whole_virtual_desktop_and_marked_as_the_helper_s() {
        let moved = mouse_of(&one(Act::Move(Spot { x: -2560.0, y: 2159.9 })));
        assert_eq!(moved.dwFlags, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK);
        assert_eq!((moved.dx, moved.dy), (0, screen::virtual_desk(2159, -300, 2460)));
        assert_eq!(moved.dwExtraInfo, OURS);
        let pressed = mouse_of(&one(Act::Press(Button::Middle)));
        assert_eq!(
            (pressed.dwFlags, pressed.dx, pressed.dy, pressed.dwExtraInfo),
            (MOUSEEVENTF_MIDDLEDOWN, 0, 0, OURS)
        );
        assert_eq!(mouse_of(&one(Act::Release(Button::Right))).dwFlags, MOUSEEVENTF_RIGHTUP);
    }

    #[test]
    fn presses_keys_by_place_and_the_awkward_few_by_virtual_key() {
        let key = |code, up| {
            let named = keys::named(code).expect("known");
            key_of(&one(if up { Act::KeyUp(named) } else { Act::KeyDown(named) }))
        };
        let a = key("KeyA", false);
        assert_eq!((a.wVk, a.wScan, a.dwFlags, a.dwExtraInfo), (0, 0x1e, KEYEVENTF_SCANCODE, OURS));
        let up = key("ArrowUp", true);
        assert_eq!(
            (up.wVk, up.wScan, up.dwFlags),
            (0, 0x48, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP)
        );
        let pause = key("Pause", false);
        assert_eq!((pause.wVk, pause.dwFlags & KEYEVENTF_SCANCODE), (0x13, 0));
        let num_lock = key("NumLock", false);
        assert_eq!((num_lock.wVk, num_lock.dwFlags), (0x90, KEYEVENTF_EXTENDEDKEY));
    }

    #[test]
    fn turns_browser_wheel_pixels_into_wheel_deltas_keeping_the_remainder() {
        let mut wheels = Wheels::default();
        let down = inputs_of(&[Act::Wheel { dx: 0.0, dy: 100.0 }], DESK, &mut wheels);
        assert_eq!(down.len(), 1);
        assert_eq!(mouse_of(&down[0]).dwFlags, MOUSEEVENTF_WHEEL);
        assert_eq!(mouse_of(&down[0]).mouseData as i32, -120);
        let sideways = [
            Act::Wheel { dx: 0.25, dy: 0.0 },
            Act::Wheel { dx: 0.5, dy: 0.0 },
            Act::Wheel { dx: 25.0, dy: 0.0 },
        ];
        let right = inputs_of(&sideways, DESK, &mut wheels);
        let amounts: Vec<(u32, i32)> =
            right.iter().map(|input| (mouse_of(input).dwFlags, mouse_of(input).mouseData as i32)).collect();
        assert_eq!(amounts, [(MOUSEEVENTF_HWHEEL, 30)]);
    }

    #[test]
    fn counts_only_its_own_injected_input_as_not_the_keeper_s() {
        assert!(!is_physical(true, OURS));
        assert!(is_physical(false, OURS));
        assert!(is_physical(true, 0));
        assert!(is_physical(false, 0));
    }
}
