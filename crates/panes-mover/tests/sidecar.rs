use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::AsRawHandle;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, Once, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{FILETIME, HWND, RECT};
use windows_sys::Win32::System::Threading::GetProcessTimes;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GW_HWNDNEXT, GetDesktopWindow, GetMessageW, GetWindow, GetWindowRect,
    IsWindowVisible, MSG, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows_sys::w;

const HELPER: &str = env!("CARGO_BIN_EXE_kw-panes-mover");
const PATIENCE: Duration = Duration::from_secs(5);
const BOTH_ISSUES: [&str; 2] = ["async", "defer"];

#[test]
fn announces_its_protocol_version_and_its_parent() {
    let helper = Sidecar::start("async");
    assert_eq!(helper.hears(), format!("ready 1 {}", std::process::id()));
}

#[test]
fn glides_a_window_of_its_parent_to_the_target() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 7 150 {window:x}@120,140>420,260"));
        assert_eq!(helper.hears(), "settled 7", "{issue}");
        assert_eq!(resting_place(window, (420, 260)), (420, 260), "{issue}");
    }
}

#[test]
fn jumps_when_the_duration_is_zero() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 3 0 {window:x}@120,140>-40,90"));
        assert_eq!(helper.hears(), "settled 3", "{issue}");
        assert_eq!(resting_place(window, (-40, 90)), (-40, 90), "{issue}");
    }
}

#[test]
fn moves_position_only() {
    for issue in BOTH_ISSUES {
        let (lower, upper) = (hidden_window(100, 100), hidden_window(300, 100));
        assert!(is_above(upper, lower), "{issue}");
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 1 80 {upper:x}@300,100>360,220 {lower:x}@100,100>160,220"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        assert_eq!(resting_place(lower, (160, 220)), (160, 220), "{issue}");

        assert_eq!(size(upper), WINDOW_SIZE, "{issue}");
        assert_eq!(size(lower), WINDOW_SIZE, "{issue}");
        assert!(is_above(upper, lower), "{issue}");
        assert!(!is_visible(upper) && !is_visible(lower), "{issue}");
    }
}

#[test]
fn settles_at_once_when_the_window_is_already_there() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("async");
    helper.tell(&format!("glide 9 500 {window:x}@700,700>700,700"));
    assert_eq!(helper.hears(), "settled 9");
    assert_eq!(place(window), (120, 140));
}

#[test]
fn a_newer_glide_takes_the_window_and_settles_the_older_one() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("async");
    helper.tell(&format!("glide 1 60000 {window:x}@120,140>900,140"));
    helper.tell(&format!("glide 2 100 {window:x}@120,140>120,400"));
    assert_eq!(helper.hears(), "settled 1");
    assert_eq!(helper.hears(), "settled 2");
    assert_eq!(resting_place(window, (120, 400)), (120, 400));
}

#[test]
fn release_stops_a_glide_where_it_is() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("defer");
    helper.tell(&format!("glide 1 60000 {window:x}@120,140>2000,140"));
    thread::sleep(Duration::from_millis(200));
    helper.tell(&format!("release {window:x}"));
    assert_eq!(helper.hears(), "settled 1");

    let released_at = place(window);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(place(window), released_at);
    assert!(released_at.0 > 120 && released_at.0 < 2000);
}

#[test]
fn refuses_a_window_its_parent_does_not_own() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::behind_a_shell();
    assert_eq!(helper.hears(), format!("ready 1 {}", helper.process.id()));

    helper.tell(&format!("glide 5 0 {window:x}@120,140>500,500"));
    assert_eq!(helper.hears(), format!("refused {window:x} not-parent-window"));
    assert_eq!(helper.hears(), "settled 5");
    helper.tell(&format!("border {window:x} ff8800"));
    assert_eq!(helper.hears(), format!("refused {window:x} not-parent-window"));

    thread::sleep(Duration::from_millis(100));
    assert_eq!(place(window), (120, 140));
}

#[test]
fn refuses_the_desktop_and_handles_that_are_no_window() {
    let desktop = unsafe { GetDesktopWindow() } as usize;
    let mut helper = Sidecar::ready("async");
    helper.tell(&format!("glide 6 0 {desktop:x}@0,0>0,0 1@0,0>5,5"));
    assert_eq!(helper.hears(), format!("refused {desktop:x} not-parent-window"));
    assert_eq!(helper.hears(), "refused 1 no-such-window");
    assert_eq!(helper.hears(), "settled 6");
}

#[test]
fn refuses_a_window_that_is_not_top_level() {
    let child = hidden_child_window();
    let mut helper = Sidecar::ready("async");
    helper.tell(&format!("glide 8 0 {child:x}@0,0>10,10"));
    assert_eq!(helper.hears(), format!("refused {child:x} not-top-level"));
    assert_eq!(helper.hears(), "settled 8");
}

#[test]
fn moves_the_windows_it_may_and_refuses_the_rest_of_a_glide() {
    let desktop = unsafe { GetDesktopWindow() } as usize;
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("defer");
    helper.tell(&format!("glide 4 50 {desktop:x}@0,0>0,0 {window:x}@120,140>200,200"));
    assert_eq!(helper.hears(), format!("refused {desktop:x} not-parent-window"));
    assert_eq!(helper.hears(), "settled 4");
    assert_eq!(resting_place(window, (200, 200)), (200, 200));
}

#[test]
fn answers_what_it_cannot_read_and_keeps_serving() {
    let mut helper = Sidecar::ready("async");
    helper.tell("resize 1f 10,10");
    assert_eq!(helper.hears(), "refused - unknown-command");
    helper.tell(&"x".repeat(5000));
    assert_eq!(helper.hears(), "refused - line-too-long");
    helper.tell("stats");
    assert_eq!(helper.hears(), "stats frames=0 missed=0 mean_us=0 worst_us=0");
}

#[test]
fn counts_the_frames_of_a_glide_and_starts_again_after_reporting() {
    for issue in BOTH_ISSUES {
        let (left, right) = (hidden_window(100, 100), hidden_window(300, 100));
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 1 200 {left:x}@100,100>700,500 {right:x}@300,100>900,500"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        helper.tell("stats");

        let stats = helper.hears();
        assert!((2..=60).contains(&counted(&stats, "frames")), "{issue}: {stats}");
        assert!(counted(&stats, "worst_us") >= counted(&stats, "mean_us"), "{issue}: {stats}");
        helper.tell("stats");
        assert_eq!(helper.hears(), "stats frames=0 missed=0 mean_us=0 worst_us=0", "{issue}");
    }
}

#[test]
fn moves_a_window_as_far_as_its_owner_says_whatever_frame_the_owner_leaves_out() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 1 100 {window:x}@132,140>512,400"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        assert_eq!(resting_place(window, (500, 400)), (500, 400), "{issue}");
    }
}

#[test]
fn a_newer_glide_keeps_the_frame_the_older_one_measured() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("glide 1 60000 {window:x}@132,140>912,140"));
        thread::sleep(Duration::from_millis(100));
        helper.tell(&format!("glide 2 100 {window:x}@-7,-7>512,400"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        assert_eq!(helper.hears(), "settled 2", "{issue}");
        assert_eq!(resting_place(window, (500, 400)), (500, 400), "{issue}");
    }
}

#[test]
fn uses_no_processor_time_while_idle() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("async");
    helper.tell(&format!("glide 1 50 {window:x}@120,140>300,300"));
    assert_eq!(helper.hears(), "settled 1");
    thread::sleep(Duration::from_millis(200));

    let before = helper.processor_time();
    thread::sleep(Duration::from_millis(600));
    assert_eq!(helper.processor_time(), before);
}

#[test]
fn exits_when_its_input_closes() {
    let mut helper = Sidecar::ready("async");
    helper.hang_up();
    assert_eq!(helper.exit_code(), Some(0));
}

#[test]
fn exits_when_its_input_closes_in_the_middle_of_a_glide() {
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("defer");
    helper.tell(&format!("glide 1 60000 {window:x}@120,140>900,140"));
    thread::sleep(Duration::from_millis(100));
    helper.hang_up();
    assert_eq!(helper.exit_code(), Some(0));
}

#[test]
fn exits_when_its_parent_dies_though_its_input_stays_open() {
    let mut helper = Sidecar::behind_a_shell();
    assert!(helper.hears().starts_with("ready 1 "));
    helper.process.kill().expect("a shell to kill");
    assert_eq!(helper.events.recv_timeout(PATIENCE), Err(RecvTimeoutError::Disconnected));
    assert!(helper.input.is_some());
}

#[test]
fn refuses_to_start_with_arguments_it_does_not_know() {
    let mut helper = Sidecar::spawn(Command::new(HELPER).arg("--parent=4"));
    assert_eq!(helper.exit_code(), Some(1));
    assert!(helper.events.recv_timeout(PATIENCE).is_err());
}

#[test]
fn plays_a_window_to_a_new_place_and_size() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("play 7 {window:x}@120,140,320,200>420,260,480,300~150"));
        assert_eq!(helper.hears(), "settled 7", "{issue}");
        assert_eq!(resting_frame(window, (420, 260, 480, 300)), (420, 260, 480, 300), "{issue}");
        assert!(!is_visible(window), "{issue}");
    }
}

#[test]
fn a_play_that_keeps_a_windows_size_never_sizes_it() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("play 2 {window:x}@120,140,999,999>420,260,999,999~80"));
        assert_eq!(helper.hears(), "settled 2", "{issue}");
        assert_eq!(resting_frame(window, (420, 260, 320, 200)), (420, 260, 320, 200), "{issue}");
    }
}

#[test]
fn a_part_told_to_takes_its_size_at_once_and_then_only_travels() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("play 1 {window:x}@120,140,320,200>2120,140,480,300~60000!"));
        let whole = || (place(window), size(window));
        let deadline = Instant::now() + PATIENCE;
        while size(window) != (480, 300) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let (at, sized) = whole();
        assert_eq!(sized, (480, 300), "{issue}");
        assert!(at.0 < 2120, "{issue}: {at:?}");
        helper.tell(&format!("release {window:x}"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
    }
}

#[test]
fn a_part_waits_out_its_delay_while_the_others_move() {
    let (first, second) = (hidden_window(100, 100), hidden_window(300, 100));
    let mut helper = Sidecar::ready("defer");
    let started = Instant::now();
    let later = format!("{second:x}@300,100,320,200>360,220,320,200+1500");
    helper.tell(&format!("play 1 {first:x}@100,100,320,200>160,220,320,200 {later}"));

    assert_eq!(resting_place(first, (160, 220)), (160, 220));
    assert_eq!(place(second), (300, 100));
    assert_eq!(helper.hears(), "settled 1");
    assert!(started.elapsed() >= Duration::from_millis(1400));
    assert_eq!(resting_place(second, (360, 220)), (360, 220));
}

#[test]
fn sizes_a_window_as_its_owner_says_whatever_frame_the_owner_leaves_out() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("play 1 {window:x}@132,140,300,180>512,400,500,280~100"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        assert_eq!(resting_frame(window, (500, 400, 520, 300)), (500, 400, 520, 300), "{issue}");
    }
}

#[test]
fn a_newer_play_carries_on_from_the_size_the_older_one_reached() {
    for issue in BOTH_ISSUES {
        let window = hidden_window(120, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("play 1 {window:x}@120,140,320,200>120,140,2320,200~60000"));
        thread::sleep(Duration::from_millis(150));
        helper.tell(&format!("play 2 {window:x}@120,140,2320,200>120,140,400,240~100"));
        assert_eq!(helper.hears(), "settled 1", "{issue}");
        assert_eq!(helper.hears(), "settled 2", "{issue}");
        assert_eq!(resting_frame(window, (120, 140, 400, 240)), (120, 140, 400, 240), "{issue}");
    }
}

#[test]
fn refuses_a_part_whose_window_its_parent_may_not_move() {
    let desktop = unsafe { GetDesktopWindow() } as usize;
    let window = hidden_window(120, 140);
    let mut helper = Sidecar::ready("defer");
    let ours = format!("{window:x}@120,140,320,200>200,200,400,240~50");
    helper.tell(&format!("play 5 {desktop:x}@0,0,10,10>5,5,10,10 1@0,0,10,10>5,5,10,10 {ours}"));
    assert_eq!(helper.hears(), format!("refused {desktop:x} not-parent-window"));
    assert_eq!(helper.hears(), "refused 1 no-such-window");
    assert_eq!(helper.hears(), "settled 5");
    assert_eq!(resting_frame(window, (200, 200, 400, 240)), (200, 200, 400, 240));
}

#[test]
fn answers_a_play_it_cannot_read_and_keeps_serving() {
    let mut helper = Sidecar::ready("async");
    helper.tell("play 1 1f@0,0>1,1");
    assert_eq!(helper.hears(), "refused - bad-rect");
    helper.tell("stack 1f");
    assert_eq!(helper.hears(), "refused - missing-argument");
    helper.tell("stats");
    assert_eq!(helper.hears(), "stats frames=0 missed=0 mean_us=0 worst_us=0");
}

#[test]
fn stacks_windows_in_the_order_given_directly_above_their_base() {
    let _alone = stacking_alone();
    for issue in BOTH_ISSUES {
        let base = hidden_window(100, 100);
        let (a, b, c) = (hidden_window(110, 110), hidden_window(120, 120), hidden_window(130, 130));
        let bystander = hidden_window(140, 140);
        let mut helper = Sidecar::ready(issue);
        helper.tell(&format!("stack {base:x} {c:x} {a:x} {b:x}"));
        helper.tell("stats");
        assert!(helper.hears().starts_with("stats "), "{issue}");

        assert!(directly_above(b, a) && directly_above(a, c) && directly_above(c, base), "{issue}");
        assert!(is_above(bystander, b), "{issue}");
        for window in [base, a, b, c, bystander] {
            assert_eq!(size(window), WINDOW_SIZE, "{issue}");
            assert!(!is_visible(window), "{issue}");
        }
        assert_eq!((place(a), place(b), place(c)), ((110, 110), (120, 120), (130, 130)), "{issue}");
    }
}

#[test]
fn stacks_again_whatever_order_the_windows_were_left_in() {
    let _alone = stacking_alone();
    let base = hidden_window(100, 100);
    let (a, b, c) = (hidden_window(110, 110), hidden_window(120, 120), hidden_window(130, 130));
    let mut helper = Sidecar::ready("defer");
    helper.tell(&format!("stack {base:x} {a:x} {b:x} {c:x}"));
    helper.tell(&format!("stack {base:x} {b:x} {c:x} {a:x}"));
    helper.tell("stats");
    assert!(helper.hears().starts_with("stats "));
    assert!(directly_above(a, c) && directly_above(c, b) && directly_above(b, base));
}

#[test]
fn refuses_to_stack_windows_its_parent_does_not_own() {
    let _alone = stacking_alone();
    let desktop = unsafe { GetDesktopWindow() } as usize;
    let (base, a, b) = (hidden_window(100, 100), hidden_window(110, 110), hidden_window(120, 120));
    let mut helper = Sidecar::ready("defer");
    helper.tell(&format!("stack {desktop:x} {a:x} {b:x}"));
    assert_eq!(helper.hears(), format!("refused {desktop:x} not-parent-window"));
    helper.tell(&format!("stack {base:x} {b:x} 1 {a:x}"));
    assert_eq!(helper.hears(), "refused 1 no-such-window");
    assert!(directly_above(a, b) && directly_above(b, base));

    let mut stranger = Sidecar::behind_a_shell();
    assert!(stranger.hears().starts_with("ready 1 "));
    stranger.tell(&format!("stack {base:x} {b:x} {a:x}"));
    assert_eq!(stranger.hears(), format!("refused {base:x} not-parent-window"));
    thread::sleep(Duration::from_millis(100));
    assert!(directly_above(a, b) && directly_above(b, base));
}

const WINDOW_SIZE: (i32, i32) = (320, 200);

struct Sidecar {
    process: Child,
    input: Option<ChildStdin>,
    events: Receiver<String>,
}

impl Sidecar {
    fn ready(issue: &str) -> Sidecar {
        let helper = Sidecar::start(issue);
        assert!(helper.hears().starts_with("ready 1 "));
        helper
    }

    fn start(issue: &str) -> Sidecar {
        Sidecar::spawn(Command::new(HELPER).arg(format!("--issue={issue}")))
    }

    fn behind_a_shell() -> Sidecar {
        Sidecar::spawn(Command::new("cmd").args(["/d", "/c", HELPER]))
    }

    fn spawn(command: &mut Command) -> Sidecar {
        let command = command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut process = command.spawn().expect("the helper starts");
        let output = BufReader::new(process.stdout.take().expect("piped output"));
        let (heard, events) = mpsc::channel();
        thread::spawn(move || output.lines().map_while(Result::ok).all(|line| heard.send(line).is_ok()));
        Sidecar { input: process.stdin.take(), process, events }
    }

    fn tell(&mut self, line: &str) {
        let input = self.input.as_mut().expect("open input");
        writeln!(input, "{line}").expect("the helper listens");
    }

    fn hears(&self) -> String {
        self.events.recv_timeout(PATIENCE).expect("the helper answers")
    }

    fn hang_up(&mut self) {
        self.input = None;
    }

    fn exit_code(&mut self) -> Option<i32> {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(status) = self.process.try_wait().expect("a child to wait for") {
                return status.code();
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    fn processor_time(&self) -> u64 {
        let (mut created, mut exited, mut kernel, mut user) =
            <(FILETIME, FILETIME, FILETIME, FILETIME)>::default();
        let process = self.process.as_raw_handle();
        let known = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
        assert_ne!(known, 0);
        ticks(kernel) + ticks(user)
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn hidden_window(x: i32, y: i32) -> usize {
    on_a_pumping_thread(move || top_level(WS_POPUP, x, y))
}

fn hidden_child_window() -> usize {
    on_a_pumping_thread(|| child_of(top_level(WS_POPUP, 100, 100)))
}

fn on_a_pumping_thread(create: impl FnOnce() -> HWND + Send + 'static) -> usize {
    static AWARE: Once = Once::new();
    AWARE.call_once(|| unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    });
    let (created, window) = mpsc::channel();
    thread::spawn(move || {
        created.send(create() as usize).expect("a test waiting for its window");
        pump_messages();
    });
    let window = window.recv().expect("a window");
    assert_ne!(window, 0);
    window
}

fn top_level(style: u32, x: i32, y: i32) -> HWND {
    let (width, height) = WINDOW_SIZE;
    let quiet = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    let (none, class) = (null_mut(), w!("STATIC"));
    unsafe { CreateWindowExW(quiet, class, null(), style, x, y, width, height, none, none, none, null()) }
}

fn child_of(parent: HWND) -> HWND {
    let (none, class) = (null_mut(), w!("STATIC"));
    unsafe { CreateWindowExW(0, class, null(), WS_CHILD, 0, 0, 40, 40, parent, none, none, null()) }
}

fn pump_messages() {
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe { DispatchMessageW(&message) };
    }
}

fn resting_place(window: usize, expected: (i32, i32)) -> (i32, i32) {
    let deadline = Instant::now() + PATIENCE;
    while place(window) != expected && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    place(window)
}

fn resting_frame(window: usize, expected: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let whole = || {
        let ((x, y), (width, height)) = (place(window), size(window));
        (x, y, width, height)
    };
    let deadline = Instant::now() + PATIENCE;
    while whole() != expected && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    whole()
}

// Every test shares one desktop, and a stack is placed under whatever stands above its base:
// two tests stacking at once could each see the other's windows move mid-check.
fn stacking_alone() -> MutexGuard<'static, ()> {
    static STACKING: Mutex<()> = Mutex::new(());
    STACKING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn directly_above(upper: usize, lower: usize) -> bool {
    unsafe { GetWindow(upper as HWND, GW_HWNDNEXT) as usize == lower }
}

fn place(window: usize) -> (i32, i32) {
    let frame = frame(window);
    (frame.left, frame.top)
}

fn size(window: usize) -> (i32, i32) {
    let frame = frame(window);
    (frame.right - frame.left, frame.bottom - frame.top)
}

fn frame(window: usize) -> RECT {
    let mut frame = RECT::default();
    assert_ne!(unsafe { GetWindowRect(window as HWND, &mut frame) }, 0);
    frame
}

fn is_visible(window: usize) -> bool {
    unsafe { IsWindowVisible(window as HWND) != 0 }
}

fn is_above(upper: usize, lower: usize) -> bool {
    let mut below = unsafe { GetWindow(upper as HWND, GW_HWNDNEXT) };
    while !below.is_null() && below as usize != lower {
        below = unsafe { GetWindow(below, GW_HWNDNEXT) };
    }
    !below.is_null()
}

fn counted(stats: &str, name: &str) -> u64 {
    let field = stats.split(' ').find_map(|field| field.strip_prefix(name)?.strip_prefix('='));
    field.and_then(|count| count.parse().ok()).expect("a counted field")
}

fn ticks(time: FILETIME) -> u64 {
    u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)
}
