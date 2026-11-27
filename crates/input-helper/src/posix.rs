//! On Linux and macOS, a polite stop (hang-up, interrupt, terminate) ends the helper the way a
//! closed input does, letting go of everything first. Nothing can be done about `SIGKILL`.

use std::ffi::c_int;
use std::ptr::null_mut;
use std::sync::mpsc::Sender;
use std::thread;

use crate::backend::Event;

const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;

#[cfg(target_os = "macos")]
type SigSet = u32;
#[cfg(target_os = "macos")]
const SIG_BLOCK: c_int = 1;

// glibc's `sigset_t`: 1024 bits, signal n at bit n - 1 of the first word.
#[cfg(not(target_os = "macos"))]
type SigSet = [u64; 16];
#[cfg(not(target_os = "macos"))]
const SIG_BLOCK: c_int = 0;

unsafe extern "C" {
    fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn sigwait(set: *const SigSet, signal: *mut c_int) -> c_int;
}

/// Blocks the stop signals in every thread and waits for them on one of its own. Must be called
/// before any other thread starts, so they all inherit the mask.
pub fn forward_stops(events: Sender<Event>) {
    let set = stop_signals();
    if unsafe { pthread_sigmask(SIG_BLOCK, &set, null_mut()) } != 0 {
        return;
    }
    thread::spawn(move || {
        loop {
            let mut signal = 0;
            let caught = unsafe { sigwait(&set, &mut signal) } == 0;
            if caught && events.send(Event::Stop).is_err() {
                return;
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn stop_signals() -> SigSet {
    [SIGHUP, SIGINT, SIGTERM].iter().fold(0, |set, signal| set | 1 << (signal - 1))
}

#[cfg(not(target_os = "macos"))]
fn stop_signals() -> SigSet {
    let mut set = [0; 16];
    set[0] = [SIGHUP, SIGINT, SIGTERM].iter().fold(0, |word, signal| word | 1 << (signal - 1));
    set
}
