use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::ptr::null_mut;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use windows_sys::Win32::Foundation::{HANDLE, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_TIMING_INFO, DWMWA_BORDER_COLOR, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetCompositionTimingInfo,
    DwmGetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{CLR_INVALID, GetDC, GetPixel, ReleaseDC};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetWindowInfo, GetWindowRect, WINDOWINFO, WindowFromPoint,
};

const USAGE: &str = "usage: bench_probe watch <milliseconds> <pid,pid|-> <hwnd>...\n       bench_probe look <hwnd>...\n       bench_probe border <hwnd>...";

fn main() -> ExitCode {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let report = match words.as_slice() {
        ["watch", milliseconds, processes, windows @ ..] => watch_report(milliseconds, processes, windows),
        ["look", windows @ ..] => report_each(windows, look_line),
        ["border", windows @ ..] => report_each(windows, border_line),
        _ => None,
    };
    match report {
        Some(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn watch_report(milliseconds: &str, processes: &str, windows: &[&str]) -> Option<String> {
    let windows = handles(windows)?;
    let marks = marks_on_each_input_line(open_processes(processes)?);
    let watched = watch(&windows, milliseconds.parse().ok()?);
    let mut report = format!("clock {}\n", ticks_per_second());
    report += &format!("cycles_per_second {}\n", watched.cycles_per_second);
    for (refresh, at, period) in [watched.composed_before, watched.composed_after] {
        report += &format!("vblank {refresh} {at} {period}\n");
    }
    for (index, origin) in watched.origins.iter().enumerate() {
        report += &format!("origin {index} {} {}\n", origin.0, origin.1);
    }
    for landing in &watched.landings {
        let (x, y) = landing.place;
        report += &format!("move {} {} {x} {y}\n", landing.at, landing.index);
    }
    for (at, cycles) in marks.try_iter() {
        let cycles: Vec<String> = cycles.iter().map(u64::to_string).collect();
        report += &format!("mark {at} {}\n", cycles.join(" "));
    }
    report += &format!("polls {} {}\n", watched.polls, watched.longest_gap);
    Some(report)
}

fn report_each(windows: &[&str], line: fn(usize) -> String) -> Option<String> {
    Some(handles(windows)?.into_iter().map(line).collect())
}

type Place = (i32, i32);
type Composition = (u64, u64, u64);
type Mark = (i64, Vec<u64>);

struct Landing {
    at: i64,
    index: usize,
    place: Place,
}

struct Watched {
    composed_before: Composition,
    composed_after: Composition,
    origins: Vec<Place>,
    landings: Vec<Landing>,
    polls: u64,
    longest_gap: i64,
    cycles_per_second: u64,
}

fn watch(windows: &[usize], milliseconds: i64) -> Watched {
    let composed_before = last_composition();
    let origins: Vec<Place> = windows.iter().map(|window| place(*window)).collect();
    let mut places = origins.clone();
    let mut landings = Vec::new();
    let (mut polls, mut longest_gap) = (0, 0);
    let (started, cycles_before) = (now(), cycle_counter());
    let mut polled = started;
    let until = started + milliseconds * ticks_per_second() / 1000;
    announce_watching();
    while polled < until {
        let at = now();
        longest_gap = longest_gap.max(at - polled);
        polled = at;
        polls += 1;
        for (index, window) in windows.iter().enumerate() {
            let place = place(*window);
            if place != places[index] {
                places[index] = place;
                landings.push(Landing { at, index, place });
            }
        }
        std::hint::spin_loop();
    }
    let spun = (cycle_counter() - cycles_before) as f64 / (polled - started) as f64;
    Watched {
        composed_before,
        composed_after: last_composition(),
        origins,
        landings,
        polls,
        longest_gap,
        cycles_per_second: (spun * ticks_per_second() as f64) as u64,
    }
}

fn announce_watching() {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "watching {}", now());
    let _ = out.flush();
}

fn marks_on_each_input_line(processes: Vec<usize>) -> Receiver<Mark> {
    let (marked, marks) = mpsc::channel();
    thread::spawn(move || {
        for _ in io::stdin().lock().lines().map_while(Result::ok) {
            let cycles = processes.iter().map(|process| process_cycles(*process));
            let _ = marked.send((now(), cycles.collect()));
        }
    });
    marks
}

fn look_line(window: usize) -> String {
    let (rect, client, frame) = (window_rect(window), client_rect(window), visible_frame(window));
    format!(
        "look {window:x} rect={} client={} frame={} exposed={}\n",
        corners(rect),
        corners(client),
        corners(frame),
        u8::from(is_exposed_at(window, inside_left_edge(frame))),
    )
}

fn border_line(window: usize) -> String {
    let (result, colour) = border_attribute(window);
    let frame = visible_frame(window);
    let inside = inside_left_edge(frame);
    let edge = [0, 1, 2].map(|inset| screen_pixel(frame.left + inset, inside.1));
    format!(
        "border {window:x} get={result:08x} value={colour:08x} edge={} exposed={}\n",
        edge.join(","),
        u8::from(is_exposed_at(window, inside)),
    )
}

fn inside_left_edge(frame: RECT) -> Place {
    (frame.left + 24, (frame.top + frame.bottom) / 2)
}

fn corners(rect: RECT) -> String {
    format!("{},{},{},{}", rect.left, rect.top, rect.right, rect.bottom)
}

fn handles(words: &[&str]) -> Option<Vec<usize>> {
    let handles: Option<Vec<usize>> = words.iter().map(|word| usize::from_str_radix(word, 16).ok()).collect();
    handles.filter(|handles| !handles.is_empty())
}

fn open_processes(list: &str) -> Option<Vec<usize>> {
    let pids = list.split(',').filter(|pid| *pid != "-");
    pids.map(|pid| open_process(pid.parse().ok()?)).collect()
}

fn open_process(pid: u32) -> Option<usize> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    (!process.is_null()).then_some(process as usize)
}

fn process_cycles(process: usize) -> u64 {
    let mut cycles = 0;
    unsafe { QueryProcessCycleTime(process as HANDLE, &mut cycles) };
    cycles
}

// Process cycle times count ticks of the processor's timestamp counter, so its rate turns them
// into seconds. Elsewhere the best stand-in is this thread's own cycle time while it spins.
#[cfg(target_arch = "x86_64")]
fn cycle_counter() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(not(target_arch = "x86_64"))]
fn cycle_counter() -> u64 {
    use windows_sys::Win32::System::Threading::GetCurrentThread;
    use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;
    let mut cycles = 0;
    unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    cycles
}

fn place(window: usize) -> Place {
    let rect = window_rect(window);
    (rect.left, rect.top)
}

fn window_rect(window: usize) -> RECT {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(window as HWND, &mut rect) };
    rect
}

fn client_rect(window: usize) -> RECT {
    let mut info = WINDOWINFO { cbSize: size_of::<WINDOWINFO>() as u32, ..Default::default() };
    unsafe { GetWindowInfo(window as HWND, &mut info) };
    info.rcClient
}

fn now() -> i64 {
    let mut ticks = 0;
    unsafe { QueryPerformanceCounter(&mut ticks) };
    ticks
}

fn ticks_per_second() -> i64 {
    let mut frequency = 0;
    unsafe { QueryPerformanceFrequency(&mut frequency) };
    frequency
}

fn last_composition() -> Composition {
    let mut timing = DWM_TIMING_INFO { cbSize: size_of::<DWM_TIMING_INFO>() as u32, ..Default::default() };
    unsafe { DwmGetCompositionTimingInfo(null_mut(), &mut timing) };
    (timing.cRefresh, timing.qpcVBlank, timing.qpcRefreshPeriod)
}

fn border_attribute(window: usize) -> (i32, u32) {
    let mut colour = 0u32;
    let size = size_of_val(&colour) as u32;
    let result = unsafe {
        DwmGetWindowAttribute(window as HWND, DWMWA_BORDER_COLOR as u32, (&raw mut colour).cast(), size)
    };
    (result, colour)
}

fn visible_frame(window: usize) -> RECT {
    let mut frame = RECT::default();
    let size = size_of_val(&frame) as u32;
    unsafe {
        DwmGetWindowAttribute(
            window as HWND,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&raw mut frame).cast(),
            size,
        )
    };
    frame
}

fn screen_pixel(x: i32, y: i32) -> String {
    let screen = unsafe { GetDC(null_mut()) };
    let colorref = unsafe { GetPixel(screen, x, y) };
    unsafe { ReleaseDC(null_mut(), screen) };
    if colorref == CLR_INVALID {
        return "unread".to_owned();
    }
    let [red, green, blue, _] = colorref.to_le_bytes();
    format!("{red:02x}{green:02x}{blue:02x}")
}

fn is_exposed_at(window: usize, point: Place) -> bool {
    let under = unsafe { WindowFromPoint(POINT { x: point.0, y: point.1 }) };
    unsafe { GetAncestor(under, GA_ROOT) as usize == window }
}
