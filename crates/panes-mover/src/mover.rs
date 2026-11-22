use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{Command, Event, Reason, Rgb, Target};
use crate::screen::{Standing, Window};
use crate::stats::FrameStats;
use crate::tracks::{Frame, Leg, Move, Tracks};
use crate::win32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issue {
    Async,
    Defer,
}

pub struct Mover {
    desk: Arc<Desk>,
}

impl Mover {
    pub fn start(parent_pid: u32, issue: Issue) -> Mover {
        let desk = Arc::new(Desk::new(parent_pid, issue));
        let pacer = Arc::clone(&desk);
        thread::spawn(move || pacer.keep_pace());
        Mover { desk }
    }

    pub fn obey(&self, command: Command) {
        match command {
            Command::Glide { id, duration_ms, targets } => self.desk.glide(id, duration_ms, &targets),
            Command::Release { window } => self.desk.release(window),
            Command::Border { window, colour } => self.desk.border(window, colour),
            Command::Stats => self.desk.report_stats(),
        }
    }

    pub fn refuse(&self, reason: Reason) {
        emit(&Event::Refused { window: None, reason });
    }
}

impl Issue {
    pub fn named(name: &str) -> Option<Issue> {
        match name {
            "async" => Some(Issue::Async),
            "defer" => Some(Issue::Defer),
            _ => None,
        }
    }

    fn apply(self, moves: &[Move]) {
        if self == Issue::Async || !win32::move_as_one_batch(moves) {
            win32::move_each_async(moves);
        }
    }
}

pub fn emit(event: &Event) {
    let mut out = io::stdout().lock();
    if writeln!(out, "{event}").and_then(|()| out.flush()).is_err() {
        std::process::exit(0);
    }
}

const FALLBACK_REFRESH: Duration = Duration::from_millis(16);
const FALLBACK_REFRESH_US: u64 = 16_667;
const FASTEST_REFRESH_US: u64 = 1_000;

struct Desk {
    parent_pid: u32,
    issue: Issue,
    started: Instant,
    state: Mutex<State>,
    gliding: Condvar,
}

#[derive(Default)]
struct State {
    tracks: Tracks,
    stats: FrameStats,
}

type Refusals = Vec<(Window, Reason)>;

impl Desk {
    fn new(parent_pid: u32, issue: Issue) -> Desk {
        Desk { parent_pid, issue, started: Instant::now(), state: Mutex::default(), gliding: Condvar::new() }
    }

    fn glide(&self, id: u32, duration_ms: u32, targets: &[Target]) {
        let mut refused = Refusals::new();
        let settled = {
            let mut state = self.state();
            let legs = self.legs(&state.tracks, targets, &mut refused);
            state.tracks.start(id, self.now_us(), duration_ms, &legs)
        };
        self.gliding.notify_one();
        announce(refused, settled);
    }

    fn release(&self, window: Window) {
        let settled = self.state().tracks.release(window);
        announce(Refusals::new(), settled);
    }

    fn border(&self, window: Window, colour: Option<Rgb>) {
        let painted =
            self.admit(window).and_then(|()| win32::set_border(window, colour).map_err(Reason::BorderFailed));
        if let Err(reason) = painted {
            emit(&Event::Refused { window: Some(window), reason });
        }
    }

    fn report_stats(&self) {
        let report = self.state().stats.take();
        emit(&Event::Stats(report));
    }

    fn keep_pace(&self) {
        loop {
            self.park_until_gliding();
            let refresh_us = win32::refresh_period_us().unwrap_or(FALLBACK_REFRESH_US);
            let mut previous_us = None;
            while self.is_gliding() {
                self.wait_for_refresh(previous_us);
                let now_us = self.now_us();
                self.frame(now_us, previous_us.map(|previous| (now_us - previous, refresh_us)));
                previous_us = Some(now_us);
            }
        }
    }

    fn park_until_gliding(&self) {
        let mut state = self.state();
        while state.tracks.is_idle() {
            state = self.gliding.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn is_gliding(&self) -> bool {
        !self.state().tracks.is_idle()
    }

    fn wait_for_refresh(&self, previous_us: Option<u64>) {
        let composed = win32::wait_for_composition();
        if !waited_a_refresh(composed, previous_us.map(|previous| self.now_us() - previous)) {
            thread::sleep(FALLBACK_REFRESH);
        }
    }

    fn frame(&self, now_us: u64, waited: Option<(u64, u64)>) {
        let mut refused = Refusals::new();
        let settled = {
            let mut state = self.state();
            let Frame { moves, mut settled } = state.tracks.frame(now_us);
            let moves = self.still_ours(moves, &mut state.tracks, &mut refused, &mut settled);
            if let Some((interval_us, refresh_us)) = waited {
                state.stats.waited(interval_us, refresh_us);
            }
            if !moves.is_empty() {
                let issuing = Instant::now();
                self.issue.apply(&moves);
                state.stats.issued(micros(issuing.elapsed()));
            }
            settled
        };
        announce(refused, settled);
    }

    fn legs(&self, tracks: &Tracks, targets: &[Target], refused: &mut Refusals) -> Vec<Leg> {
        let mut legs = Vec::with_capacity(targets.len());
        for target in targets {
            match self.leg(tracks, target) {
                Ok(leg) => legs.push(leg),
                Err(reason) => refused.push((target.window, reason)),
            }
        }
        legs
    }

    fn leg(&self, tracks: &Tracks, target: &Target) -> Result<Leg, Reason> {
        let window = target.window;
        self.admit(window)?;
        let standing = tracks.standing(window).or_else(|| standing_as_told(target));
        let Standing { at, window_offset } = standing.ok_or(Reason::NoSuchWindow)?;
        Ok(Leg { window, from: at, to: target.to, window_offset })
    }

    fn still_ours(
        &self,
        moves: Vec<Move>,
        tracks: &mut Tracks,
        refused: &mut Refusals,
        settled: &mut Vec<u32>,
    ) -> Vec<Move> {
        let mut ours = Vec::with_capacity(moves.len());
        for step in moves {
            match self.admit(step.window) {
                Ok(()) => ours.push(step),
                Err(reason) => {
                    refused.push((step.window, reason));
                    settled.extend(tracks.release(step.window));
                }
            }
        }
        ours
    }

    fn admit(&self, window: Window) -> Result<(), Reason> {
        match win32::owner_pid(window) {
            None => Err(Reason::NoSuchWindow),
            Some(owner) if owner != self.parent_pid => Err(Reason::NotParentWindow),
            Some(_) if !win32::is_top_level(window) => Err(Reason::NotTopLevel),
            Some(_) => Ok(()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now_us(&self) -> u64 {
        micros(self.started.elapsed())
    }
}

fn announce(refused: Refusals, settled: Vec<u32>) {
    for (window, reason) in refused {
        emit(&Event::Refused { window: Some(window), reason });
    }
    for id in settled {
        emit(&Event::Settled { id });
    }
}

fn standing_as_told(target: &Target) -> Option<Standing> {
    let corner = win32::corner(target.window)?;
    Some(Standing { at: target.at, window_offset: corner - target.at })
}

// Nothing guarantees that DwmFlush blocks: it fails without composition, and a call that
// returned without waiting would turn a glide into a busy loop.
fn waited_a_refresh(composed: bool, since_previous_us: Option<u64>) -> bool {
    composed && since_previous_us.is_none_or(|waited_us| waited_us >= FASTEST_REFRESH_US)
}

fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_both_ways_of_issuing_moves() {
        assert_eq!(Issue::named("async"), Some(Issue::Async));
        assert_eq!(Issue::named("defer"), Some(Issue::Defer));
        assert_eq!(Issue::named("sync"), None);
    }

    #[test]
    fn trusts_a_wait_that_lasted_about_a_refresh() {
        assert!(waited_a_refresh(true, None));
        assert!(waited_a_refresh(true, Some(16_600)));
        assert!(waited_a_refresh(true, Some(4_100)));
    }

    #[test]
    fn falls_back_to_sleeping_when_the_compositor_does_not_pace() {
        assert!(!waited_a_refresh(false, Some(16_600)));
        assert!(!waited_a_refresh(false, None));
        assert!(!waited_a_refresh(true, Some(40)));
    }
}
