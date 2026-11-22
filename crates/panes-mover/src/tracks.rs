use crate::easing::{eased_to_rest, point_along, size_along};
use crate::screen::{Point, Size, Sizing, Standing, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leg {
    pub window: Window,
    pub from: Point,
    pub to: Point,
    pub window_offset: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resize {
    pub from: Size,
    pub to: Size,
    pub window_offset: Size,
    /// The window takes its new size on the first frame of its turn, then only travels.
    pub at_once: bool,
}

/// One window's role in a play: its leg, a change of size when it has one, how long after the
/// play starts it sets off, and how long it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    pub leg: Leg,
    pub resize: Option<Resize>,
    pub delay_ms: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub window: Window,
    pub to: Point,
    pub size: Option<Size>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub moves: Vec<Move>,
    pub settled: Vec<u32>,
}

#[derive(Default)]
pub struct Tracks {
    runs: Vec<Run>,
}

impl Tracks {
    /// A glide: every window sets off at once, takes the same time and keeps its size.
    pub fn start(&mut self, id: u32, now_us: u64, duration_ms: u32, legs: &[Leg]) -> Vec<u32> {
        let roles: Vec<Role> =
            legs.iter().map(|leg| Role { leg: *leg, resize: None, delay_ms: 0, duration_ms }).collect();
        self.play(id, now_us, &roles)
    }

    pub fn play(&mut self, id: u32, now_us: u64, roles: &[Role]) -> Vec<u32> {
        let mut settled: Vec<u32> = roles.iter().flat_map(|role| self.release(role.leg.window)).collect();
        let steps: Vec<Step> =
            roles.iter().filter(|role| role.changes()).map(|role| Step::waiting(role, now_us)).collect();
        if steps.is_empty() {
            settled.push(id);
        } else {
            self.runs.push(Run { id, steps });
        }
        settled
    }

    pub fn release(&mut self, window: Window) -> Vec<u32> {
        for run in &mut self.runs {
            run.steps.retain(|step| step.role.leg.window != window);
        }
        self.take_finished(|run| run.steps.is_empty())
    }

    pub fn frame(&mut self, now_us: u64) -> Frame {
        let moves = self.runs.iter_mut().flat_map(|run| run.advance(now_us)).collect();
        let settled = self.take_finished(|run| run.is_over(now_us));
        Frame { moves, settled }
    }

    pub fn standing(&self, window: Window) -> Option<Standing> {
        let step = self.step_of(window)?;
        Some(Standing { at: step.at, window_offset: step.role.leg.window_offset })
    }

    pub fn sizing(&self, window: Window) -> Option<Sizing> {
        let step = self.step_of(window)?;
        let resize = step.role.resize?;
        Some(Sizing { size: step.sized.unwrap_or(resize.from), window_offset: resize.window_offset })
    }

    pub fn is_idle(&self) -> bool {
        self.runs.is_empty()
    }

    fn step_of(&self, window: Window) -> Option<&Step> {
        self.runs.iter().flat_map(|run| &run.steps).find(|step| step.role.leg.window == window)
    }

    fn take_finished(&mut self, finished: impl Fn(&Run) -> bool) -> Vec<u32> {
        self.runs.extract_if(.., |run| finished(run)).map(|run| run.id).collect()
    }
}

impl Role {
    fn changes(&self) -> bool {
        self.leg.from != self.leg.to || self.resize.is_some_and(|resize| resize.from != resize.to)
    }
}

struct Run {
    id: u32,
    steps: Vec<Step>,
}

struct Step {
    role: Role,
    starts_us: u64,
    duration_us: u64,
    at: Point,
    sized: Option<Size>,
}

impl Run {
    fn advance(&mut self, now_us: u64) -> Vec<Move> {
        self.steps.iter_mut().filter_map(|step| step.advance(now_us)).collect()
    }

    fn is_over(&self, now_us: u64) -> bool {
        self.steps.iter().all(|step| step.progress(now_us).is_some_and(|progress| progress >= 1.0))
    }
}

impl Step {
    fn waiting(role: &Role, now_us: u64) -> Step {
        Step {
            role: *role,
            starts_us: now_us.saturating_add(u64::from(role.delay_ms) * 1000),
            duration_us: u64::from(role.duration_ms) * 1000,
            at: role.leg.from,
            sized: None,
        }
    }

    // None until the step's turn comes: a window is left alone while it waits.
    fn progress(&self, now_us: u64) -> Option<f64> {
        let elapsed_us = now_us.checked_sub(self.starts_us)?;
        Some(if self.duration_us == 0 { 1.0 } else { elapsed_us as f64 / self.duration_us as f64 })
    }

    fn advance(&mut self, now_us: u64) -> Option<Move> {
        let eased = eased_to_rest(self.progress(now_us)?);
        let Leg { window, from, to, window_offset } = self.role.leg;
        let point = point_along(from, to, eased);
        let grown = |resize: Resize| {
            if resize.at_once { resize.to } else { size_along(resize.from, resize.to, eased) }
        };
        let size = self.role.resize.map(grown);
        let resized =
            size.filter(|size| Some(*size) != self.sized.or(self.role.resize.map(|resize| resize.from)));
        if point == self.at && resized.is_none() {
            return None;
        }
        self.at = point;
        self.sized = size;
        let outer = |size: Size| {
            size + self.role.resize.map_or(Size { width: 0, height: 0 }, |resize| resize.window_offset)
        };
        Some(Move { window, to: point + window_offset, size: resized.map(outer) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Window = Window(0xa);
    const B: Window = Window(0xb);
    const ORIGIN: Point = Point { x: 0, y: 0 };

    fn leg(window: Window, to: (i32, i32)) -> Leg {
        Leg { window, from: ORIGIN, to: Point { x: to.0, y: to.1 }, window_offset: ORIGIN }
    }

    fn framed_leg(window: Window, to: (i32, i32), window_offset: (i32, i32)) -> Leg {
        Leg { window_offset: Point { x: window_offset.0, y: window_offset.1 }, ..leg(window, to) }
    }

    fn moved(window: Window, to: (i32, i32)) -> Move {
        Move { window, to: Point { x: to.0, y: to.1 }, size: None }
    }

    fn sized(window: Window, to: (i32, i32), size: (i32, i32)) -> Move {
        Move { size: Some(Size { width: size.0, height: size.1 }), ..moved(window, to) }
    }

    fn role(window: Window, to: (i32, i32), delay_ms: u32, duration_ms: u32) -> Role {
        Role { leg: leg(window, to), resize: None, delay_ms, duration_ms }
    }

    fn growing(window: Window, to: (i32, i32), from: (i32, i32), until: (i32, i32)) -> Role {
        let size = |(width, height): (i32, i32)| Size { width, height };
        let resize =
            Resize { from: size(from), to: size(until), window_offset: size((0, 0)), at_once: false };
        Role { resize: Some(resize), ..role(window, to, 0, 100) }
    }

    #[test]
    fn glides_along_the_easing_curve_and_settles_on_the_target() {
        let mut tracks = Tracks::default();
        assert!(tracks.start(1, 1_000, 100, &[leg(A, (1000, -200))]).is_empty());

        let halfway = tracks.frame(51_000);
        assert_eq!(halfway, Frame { moves: vec![moved(A, (969, -194))], settled: vec![] });

        let landed = tracks.frame(101_000);
        assert_eq!(landed, Frame { moves: vec![moved(A, (1000, -200))], settled: vec![1] });
        assert!(tracks.is_idle());
    }

    #[test]
    fn issues_no_move_when_a_frame_lands_on_the_same_pixel() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 1_000, &[leg(A, (10, 0))]);
        assert_eq!(tracks.frame(900_000).moves, vec![moved(A, (10, 0))]);
        assert_eq!(tracks.frame(950_000), Frame::default());
        assert_eq!(tracks.frame(1_000_000), Frame { moves: vec![], settled: vec![1] });
    }

    #[test]
    fn jumps_on_the_next_frame_when_the_duration_is_zero() {
        let mut tracks = Tracks::default();
        assert!(tracks.start(4, 500, 0, &[leg(A, (30, 40)), leg(B, (50, 60))]).is_empty());
        let jump = tracks.frame(500);
        assert_eq!(jump, Frame { moves: vec![moved(A, (30, 40)), moved(B, (50, 60))], settled: vec![4] });
    }

    #[test]
    fn settles_at_once_when_nothing_has_to_move() {
        let mut tracks = Tracks::default();
        assert_eq!(tracks.start(2, 0, 100, &[leg(A, (0, 0))]), vec![2]);
        assert_eq!(tracks.start(3, 0, 100, &[]), vec![3]);
        assert!(tracks.is_idle());
    }

    #[test]
    fn keeps_only_the_windows_that_have_to_move() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 0, &[leg(A, (0, 0)), leg(B, (5, 5))]);
        assert_eq!(tracks.frame(0).moves, vec![moved(B, (5, 5))]);
    }

    #[test]
    fn a_newer_glide_takes_a_window_out_of_the_older_one() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0)), leg(B, (100, 0))]);
        assert!(tracks.start(2, 0, 100, &[leg(A, (0, 300))]).is_empty());

        let landed = tracks.frame(100_000);
        assert_eq!(landed.moves, vec![moved(B, (100, 0)), moved(A, (0, 300))]);
        assert_eq!(landed.settled, vec![1, 2]);
    }

    #[test]
    fn an_older_glide_settles_when_a_newer_one_takes_its_last_window() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0))]);
        assert_eq!(tracks.start(2, 0, 100, &[leg(A, (0, 300))]), vec![1]);
        assert_eq!(tracks.frame(100_000).settled, vec![2]);
    }

    #[test]
    fn a_superseding_glide_that_moves_nothing_settles_both() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0))]);
        assert_eq!(tracks.start(2, 0, 100, &[leg(A, (0, 0))]), vec![1, 2]);
        assert!(tracks.is_idle());
    }

    #[test]
    fn release_drops_a_window_and_leaves_the_rest_gliding() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0)), leg(B, (100, 0))]);
        assert!(tracks.release(A).is_empty());
        assert_eq!(tracks.frame(100_000), Frame { moves: vec![moved(B, (100, 0))], settled: vec![1] });
    }

    #[test]
    fn release_of_the_last_window_settles_the_glide() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0))]);
        assert_eq!(tracks.release(A), vec![1]);
        assert_eq!(tracks.frame(50_000), Frame::default());
        assert!(tracks.is_idle());
    }

    #[test]
    fn release_of_a_window_that_is_not_gliding_changes_nothing() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0))]);
        assert!(tracks.release(B).is_empty());
        assert!(!tracks.is_idle());
    }

    #[test]
    fn settles_each_glide_exactly_once() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[leg(A, (100, 0))]);
        assert_eq!(tracks.frame(200_000).settled, vec![1]);
        assert!(tracks.frame(300_000).settled.is_empty());
        assert!(tracks.release(A).is_empty());
    }

    #[test]
    fn remembers_where_a_gliding_window_was_last_put() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[framed_leg(A, (1000, 0), (-12, 0))]);
        let window_offset = Point { x: -12, y: 0 };
        assert_eq!(tracks.standing(A), Some(Standing { at: ORIGIN, window_offset }));
        tracks.frame(50_000);
        assert_eq!(tracks.standing(A), Some(Standing { at: Point { x: 969, y: 0 }, window_offset }));
        assert_eq!(tracks.standing(B), None);
    }

    #[test]
    fn moves_the_frame_so_that_the_client_area_follows_the_glide() {
        let mut tracks = Tracks::default();
        tracks.start(1, 0, 100, &[framed_leg(A, (1000, 40), (-12, -3)), leg(B, (1000, 40))]);
        assert_eq!(tracks.frame(50_000).moves, vec![moved(A, (957, 36)), moved(B, (969, 39))]);
        assert_eq!(tracks.frame(100_000).moves, vec![moved(A, (988, 37)), moved(B, (1000, 40))]);
    }

    #[test]
    fn a_clock_that_reads_before_the_start_holds_the_window_still() {
        let mut tracks = Tracks::default();
        tracks.start(1, 5_000, 100, &[leg(A, (100, 0))]);
        assert_eq!(tracks.frame(4_000), Frame::default());
    }

    #[test]
    fn each_window_sets_off_after_its_own_delay_and_takes_its_own_time() {
        let mut tracks = Tracks::default();
        assert!(tracks.play(1, 0, &[role(A, (1000, 0), 0, 100), role(B, (1000, 0), 60, 40)]).is_empty());

        assert_eq!(tracks.frame(50_000).moves, vec![moved(A, (969, 0))]);
        assert_eq!(tracks.frame(80_000).moves, vec![moved(A, (1000, 0)), moved(B, (969, 0))]);
        let landed = tracks.frame(100_000);
        assert_eq!(landed, Frame { moves: vec![moved(B, (1000, 0))], settled: vec![1] });
        assert!(tracks.is_idle());
    }

    #[test]
    fn a_play_settles_only_when_its_last_window_has_landed() {
        let mut tracks = Tracks::default();
        tracks.play(1, 0, &[role(A, (10, 0), 0, 10), role(B, (10, 0), 500, 0)]);
        assert_eq!(tracks.frame(10_000), Frame { moves: vec![moved(A, (10, 0))], settled: vec![] });
        assert_eq!(tracks.frame(499_000), Frame::default());
        assert_eq!(tracks.frame(500_000), Frame { moves: vec![moved(B, (10, 0))], settled: vec![1] });
    }

    #[test]
    fn a_window_grows_along_the_same_curve_as_it_travels() {
        let mut tracks = Tracks::default();
        tracks.play(1, 0, &[growing(A, (1000, 0), (400, 300), (800, 100))]);
        assert_eq!(tracks.frame(50_000).moves, vec![sized(A, (969, 0), (788, 106))]);
        assert_eq!(
            tracks.frame(100_000),
            Frame { moves: vec![sized(A, (1000, 0), (800, 100))], settled: vec![1] }
        );
    }

    #[test]
    fn a_window_told_to_takes_its_size_on_the_first_frame_of_its_turn_and_then_only_travels() {
        let mut tracks = Tracks::default();
        let mut at_once = growing(A, (1000, 0), (400, 300), (800, 100));
        at_once.resize = at_once.resize.map(|resize| Resize { at_once: true, ..resize });
        at_once.delay_ms = 20;
        tracks.play(1, 0, &[at_once]);
        assert_eq!(tracks.frame(10_000), Frame::default());
        assert_eq!(tracks.frame(21_000).moves, vec![sized(A, (49, 0), (800, 100))]);
        assert_eq!(tracks.frame(70_000).moves, vec![moved(A, (969, 0))]);
        assert_eq!(tracks.frame(120_000), Frame { moves: vec![moved(A, (1000, 0))], settled: vec![1] });
    }

    #[test]
    fn a_window_that_only_changes_size_still_plays() {
        let mut tracks = Tracks::default();
        assert!(tracks.play(1, 0, &[growing(A, (0, 0), (400, 300), (800, 300))]).is_empty());
        assert_eq!(tracks.frame(100_000).moves, vec![sized(A, (0, 0), (800, 300))]);
    }

    #[test]
    fn says_a_size_only_on_the_frames_that_change_it() {
        let mut tracks = Tracks::default();
        tracks.play(1, 0, &[growing(A, (1000, 0), (400, 300), (401, 300))]);
        assert_eq!(tracks.frame(1_000).moves, vec![moved(A, (49, 0))]);
        assert_eq!(tracks.frame(50_000).moves, vec![sized(A, (969, 0), (401, 300))]);
        assert_eq!(tracks.frame(100_000).moves, vec![moved(A, (1000, 0))]);
    }

    #[test]
    fn adds_the_frame_the_owner_leaves_out_of_a_size() {
        let mut tracks = Tracks::default();
        let mut framed = growing(A, (0, 0), (400, 300), (800, 300));
        framed.resize =
            framed.resize.map(|resize| Resize { window_offset: Size { width: 14, height: 7 }, ..resize });
        tracks.play(1, 0, &[framed]);
        assert_eq!(tracks.frame(100_000).moves, vec![sized(A, (0, 0), (814, 307))]);
    }

    #[test]
    fn remembers_the_size_a_playing_window_was_last_given() {
        let mut tracks = Tracks::default();
        tracks.play(1, 0, &[growing(A, (1000, 0), (400, 300), (800, 100)), role(B, (5, 5), 0, 100)]);
        let none = Size { width: 0, height: 0 };
        assert_eq!(
            tracks.sizing(A),
            Some(Sizing { size: Size { width: 400, height: 300 }, window_offset: none })
        );
        tracks.frame(50_000);
        assert_eq!(
            tracks.sizing(A),
            Some(Sizing { size: Size { width: 788, height: 106 }, window_offset: none })
        );
        assert_eq!(tracks.sizing(B), None);
    }

    #[test]
    fn a_window_waiting_for_its_turn_is_left_alone_and_can_be_taken() {
        let mut tracks = Tracks::default();
        tracks.play(1, 0, &[role(A, (100, 0), 1_000, 100)]);
        assert_eq!(tracks.frame(500_000), Frame::default());
        assert_eq!(tracks.standing(A), Some(Standing { at: ORIGIN, window_offset: ORIGIN }));
        assert_eq!(tracks.play(2, 500_000, &[role(A, (0, 50), 0, 0)]), vec![1]);
        assert_eq!(tracks.frame(500_000), Frame { moves: vec![moved(A, (0, 50))], settled: vec![2] });
    }
}
