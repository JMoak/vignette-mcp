use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_TIMING_INFO, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_COLOR_NONE, DwmFlush,
    DwmGetCompositionTimingInfo, DwmSetWindowAttribute,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetThreadDpiAwarenessContext,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, GA_ROOT, GW_HWNDPREV, GetAncestor, GetWindow,
    GetWindowRect, GetWindowThreadProcessId, HWND_TOP, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

use crate::protocol::{Paint, Rgb};
use crate::screen::{Point, Rect, Size, Window};

pub const POSITION_ONLY: u32 = SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;

const LONGEST_WALK_UP: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Top,
    Window(Window),
}

/// What one call may change about a window: where it is, how large it is, what it stands under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub window: Window,
    pub to: Option<Point>,
    pub size: Option<Size>,
    pub after: Option<After>,
}

// Every call starts from a move that touches nothing else and gives up only what it was asked to
// change. None of them can show, hide, activate or reframe a window.
pub fn flags_of(placement: &Placement) -> u32 {
    let mut flags = POSITION_ONLY;
    if placement.to.is_none() {
        flags |= SWP_NOMOVE;
    }
    if placement.size.is_some() {
        flags &= !SWP_NOSIZE;
    }
    if placement.after.is_some() {
        flags &= !SWP_NOZORDER;
    }
    flags
}

pub struct Parent {
    pub pid: u32,
    process: usize,
}

impl Parent {
    pub fn discover() -> Option<Parent> {
        let pid = parent_pid_of(std::process::id())?;
        let process = open_to_watch(pid)?;
        let older = started_at(process)? <= started_at(unsafe { GetCurrentProcess() })?;
        older.then_some(Parent { pid, process: process as usize })
    }

    pub fn wait_until_gone(&self) {
        unsafe { WaitForSingleObject(self.process as HANDLE, INFINITE) };
    }
}

pub fn enable_per_monitor_dpi() -> bool {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let current = unsafe { GetThreadDpiAwarenessContext() };
    unsafe { AreDpiAwarenessContextsEqual(current, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 }
}

pub fn owner_pid(window: Window) -> Option<u32> {
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(handle(window), &mut pid) };
    (thread != 0).then_some(pid)
}

pub fn is_top_level(window: Window) -> bool {
    unsafe { GetAncestor(handle(window), GA_ROOT) == handle(window) }
}

pub fn corner(window: Window) -> Option<Point> {
    frame(window).map(|frame| frame.origin)
}

pub fn frame(window: Window) -> Option<Rect> {
    let mut frame = RECT::default();
    let found = unsafe { GetWindowRect(handle(window), &mut frame) } != 0;
    found.then_some(Rect {
        origin: Point { x: frame.left, y: frame.top },
        size: Size { width: frame.right - frame.left, height: frame.bottom - frame.top },
    })
}

pub fn nearest_above(base: Window, skipping: &[Window]) -> Option<Window> {
    let mut above = unsafe { GetWindow(handle(base), GW_HWNDPREV) };
    for _ in 0..LONGEST_WALK_UP {
        if above.is_null() {
            return None;
        }
        let window = Window(above as usize);
        if !skipping.contains(&window) {
            return Some(window);
        }
        above = unsafe { GetWindow(above, GW_HWNDPREV) };
    }
    None
}

pub fn place_each_async(placements: &[Placement]) {
    for placement in placements {
        let Asked { after, x, y, width, height, flags } = asked(placement);
        let window = handle(placement.window);
        unsafe { SetWindowPos(window, after, x, y, width, height, flags | SWP_ASYNCWINDOWPOS) };
    }
}

// One call after another, each finished before the next: the only way an order that names
// earlier windows of the same list comes out right when a batch cannot be used.
pub fn place_each_in_turn(placements: &[Placement]) {
    for placement in placements {
        let Asked { after, x, y, width, height, flags } = asked(placement);
        unsafe { SetWindowPos(handle(placement.window), after, x, y, width, height, flags) };
    }
}

pub fn place_as_one_batch(placements: &[Placement]) -> bool {
    let mut batch = unsafe { BeginDeferWindowPos(placements.len() as i32) };
    for placement in placements {
        if batch.is_null() {
            return false;
        }
        let Asked { after, x, y, width, height, flags } = asked(placement);
        batch = unsafe { DeferWindowPos(batch, handle(placement.window), after, x, y, width, height, flags) };
    }
    !batch.is_null() && unsafe { EndDeferWindowPos(batch) } != 0
}

struct Asked {
    after: HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    flags: u32,
}

fn asked(placement: &Placement) -> Asked {
    let Point { x, y } = placement.to.unwrap_or(Point { x: 0, y: 0 });
    let Size { width, height } = placement.size.unwrap_or(Size { width: 0, height: 0 });
    let after = match placement.after {
        Some(After::Window(window)) => handle(window),
        Some(After::Top) | None => HWND_TOP,
    };
    Asked { after, x, y, width, height, flags: flags_of(placement) }
}

pub fn set_border(window: Window, paint: Paint) -> Result<(), i32> {
    let colour = match paint {
        Paint::Colour(colour) => colorref(colour),
        Paint::System => DWMWA_COLOR_DEFAULT,
        Paint::Nothing => DWMWA_COLOR_NONE,
    };
    let size = size_of_val(&colour) as u32;
    let result = unsafe {
        DwmSetWindowAttribute(handle(window), DWMWA_BORDER_COLOR as u32, (&raw const colour).cast(), size)
    };
    if result < 0 { Err(result) } else { Ok(()) }
}

pub fn wait_for_composition() -> bool {
    unsafe { DwmFlush() >= 0 }
}

pub fn refresh_period_us() -> Option<u64> {
    let mut timing = DWM_TIMING_INFO { cbSize: size_of::<DWM_TIMING_INFO>() as u32, ..Default::default() };
    let known = unsafe { DwmGetCompositionTimingInfo(null_mut(), &mut timing) } >= 0;
    let rate = timing.rateRefresh;
    (known && rate.uiNumerator > 0)
        .then(|| 1_000_000 * u64::from(rate.uiDenominator) / u64::from(rate.uiNumerator))
}

fn handle(window: Window) -> HWND {
    window.0 as HWND
}

fn colorref(colour: Rgb) -> u32 {
    u32::from(colour.red) | u32::from(colour.green) << 8 | u32::from(colour.blue) << 16
}

fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}

fn parent_pid_of(pid: u32) -> Option<u32> {
    let processes = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if processes == INVALID_HANDLE_VALUE {
        return None;
    }
    let parent = find_process(processes, pid).map(|entry| entry.th32ParentProcessID);
    unsafe { CloseHandle(processes) };
    parent
}

fn find_process(processes: HANDLE, pid: u32) -> Option<PROCESSENTRY32W> {
    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut more = unsafe { Process32FirstW(processes, &mut entry) } != 0;
    while more && entry.th32ProcessID != pid {
        more = unsafe { Process32NextW(processes, &mut entry) } != 0;
    }
    more.then_some(entry)
}

fn open_to_watch(pid: u32) -> Option<HANDLE> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
    (!process.is_null()).then_some(process)
}

fn started_at(process: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let known = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) } != 0;
    known.then(|| ticks(created))
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_SHOWWINDOW};

    use super::*;

    fn every_placement() -> Vec<Placement> {
        let window = Window(1);
        let to = Some(Point { x: 1, y: 2 });
        let size = Some(Size { width: 3, height: 4 });
        let orders = [None, Some(After::Top), Some(After::Window(Window(2)))];
        let mut placements = Vec::new();
        for to in [None, to] {
            for size in [None, size] {
                for after in orders {
                    placements.push(Placement { window, to, size, after });
                }
            }
        }
        placements
    }

    #[test]
    fn a_placement_gives_up_only_what_it_was_asked_to_change() {
        let window = Window(1);
        let to = Some(Point { x: 1, y: 2 });
        let size = Some(Size { width: 3, height: 4 });
        let flags = |to, size, after| flags_of(&Placement { window, to, size, after });
        assert_eq!(flags(to, None, None), POSITION_ONLY);
        assert_eq!(flags(to, size, None), POSITION_ONLY & !SWP_NOSIZE);
        assert_eq!(flags(None, None, Some(After::Top)), (POSITION_ONLY | SWP_NOMOVE) & !SWP_NOZORDER);
        assert_eq!(flags(to, size, Some(After::Top)), SWP_NOOWNERZORDER | SWP_NOACTIVATE);
    }

    #[test]
    fn no_placement_activates_shows_hides_reframes_or_moves_an_owner() {
        let never = SWP_SHOWWINDOW | SWP_HIDEWINDOW | SWP_FRAMECHANGED;
        let always = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
        for placement in every_placement() {
            let flags = flags_of(&placement) | SWP_ASYNCWINDOWPOS;
            assert_eq!(flags & never, 0, "{placement:?}");
            assert_eq!(flags & always, always, "{placement:?}");
        }
    }

    #[test]
    fn a_move_never_sizes_reorders_or_activates() {
        let keeps = SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;
        assert_eq!(POSITION_ONLY & keeps, keeps);
    }

    #[test]
    fn a_move_never_shows_hides_or_reframes() {
        let forbidden = SWP_SHOWWINDOW | SWP_HIDEWINDOW | SWP_FRAMECHANGED | SWP_NOMOVE;
        assert_eq!((POSITION_ONLY | SWP_ASYNCWINDOWPOS) & forbidden, 0);
    }

    #[test]
    fn packs_a_colour_the_way_the_compositor_reads_it() {
        assert_eq!(colorref(Rgb { red: 0xff, green: 0x88, blue: 0x00 }), 0x0000_88ff);
        assert_eq!(colorref(Rgb { red: 0x12, green: 0x34, blue: 0x56 }), 0x0056_3412);
    }

    #[test]
    fn finds_the_process_that_started_this_one() {
        let parent = Parent::discover().expect("a test run has a parent");
        assert_ne!(parent.pid, std::process::id());
        assert_ne!(parent.pid, 0);
    }

    #[test]
    fn finds_no_owner_for_a_handle_that_is_not_a_window() {
        assert_eq!(owner_pid(Window(1)), None);
        assert_eq!(corner(Window(1)), None);
    }
}
