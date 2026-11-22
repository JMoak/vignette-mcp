use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_TIMING_INFO, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT, DwmFlush, DwmGetCompositionTimingInfo,
    DwmSetWindowAttribute,
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
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, GA_ROOT, GetAncestor, GetWindowRect,
    GetWindowThreadProcessId, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowPos,
};

use crate::protocol::Rgb;
use crate::screen::{Point, Window};
use crate::tracks::Move;

pub const POSITION_ONLY: u32 = SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;

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
    let mut frame = RECT::default();
    let found = unsafe { GetWindowRect(handle(window), &mut frame) } != 0;
    found.then_some(Point { x: frame.left, y: frame.top })
}

pub fn move_each_async(moves: &[Move]) {
    for step in moves {
        let Point { x, y } = step.to;
        unsafe {
            SetWindowPos(handle(step.window), null_mut(), x, y, 0, 0, POSITION_ONLY | SWP_ASYNCWINDOWPOS)
        };
    }
}

pub fn move_as_one_batch(moves: &[Move]) -> bool {
    let mut batch = unsafe { BeginDeferWindowPos(moves.len() as i32) };
    for step in moves {
        if batch.is_null() {
            return false;
        }
        let Point { x, y } = step.to;
        batch = unsafe { DeferWindowPos(batch, handle(step.window), null_mut(), x, y, 0, 0, POSITION_ONLY) };
    }
    !batch.is_null() && unsafe { EndDeferWindowPos(batch) } != 0
}

pub fn set_border(window: Window, colour: Option<Rgb>) -> Result<(), i32> {
    let colour = colour.map_or(DWMWA_COLOR_DEFAULT, colorref);
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOMOVE, SWP_SHOWWINDOW,
    };

    use super::*;

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
