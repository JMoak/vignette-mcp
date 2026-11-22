use crate::easing::{eased_to_rest, point_along};
use crate::screen::{Point, Standing, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leg {
    pub window: Window,
    pub from: Point,
    pub to: Point,
    pub window_offset: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub window: Window,
    pub to: Point,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub moves: Vec<Move>,
    pub settled: Vec<u32>,
}

#[derive(Default)]
pub struct Tracks {
    glides: Vec<Glide>,
}

impl Tracks {
    pub fn start(&mut self, id: u32, now_us: u64, duration_ms: u32, legs: &[Leg]) -> Vec<u32> {
        let mut settled: Vec<u32> = legs.iter().flat_map(|leg| self.release(leg.window)).collect();
        let steps: Vec<Step> = legs.iter().filter(|leg| leg.from != leg.to).map(Step::at_start).collect();
        if steps.is_empty() {
            settled.push(id);
        } else {
            self.glides.push(Glide {
                id,
                started_us: now_us,
                duration_us: u64::from(duration_ms) * 1000,
                steps,
            });
        }
        settled
    }

    pub fn release(&mut self, window: Window) -> Vec<u32> {
        for glide in &mut self.glides {
            glide.steps.retain(|step| step.leg.window != window);
        }
        self.take_finished(|glide| glide.steps.is_empty())
    }

    pub fn frame(&mut self, now_us: u64) -> Frame {
        let moves = self.glides.iter_mut().flat_map(|glide| glide.advance(now_us)).collect();
        let settled = self.take_finished(|glide| glide.progress(now_us) >= 1.0);
        Frame { moves, settled }
    }

    pub fn standing(&self, window: Window) -> Option<Standing> {
        let mut steps = self.glides.iter().flat_map(|glide| &glide.steps);
        let step = steps.find(|step| step.leg.window == window)?;
        Some(Standing { at: step.at, window_offset: step.leg.window_offset })
    }

    pub fn is_idle(&self) -> bool {
        self.glides.is_empty()
    }

    fn take_finished(&mut self, finished: impl Fn(&Glide) -> bool) -> Vec<u32> {
        self.glides.extract_if(.., |glide| finished(glide)).map(|glide| glide.id).collect()
    }
}

struct Glide {
    id: u32,
    started_us: u64,
    duration_us: u64,
    steps: Vec<Step>,
}

struct Step {
    leg: Leg,
    at: Point,
}

impl Glide {
    fn progress(&self, now_us: u64) -> f64 {
        if self.duration_us == 0 {
            return 1.0;
        }
        now_us.saturating_sub(self.started_us) as f64 / self.duration_us as f64
    }

    fn advance(&mut self, now_us: u64) -> Vec<Move> {
        let eased = eased_to_rest(self.progress(now_us));
        self.steps.iter_mut().filter_map(|step| step.advance(eased)).collect()
    }
}

impl Step {
    fn at_start(leg: &Leg) -> Step {
        Step { leg: *leg, at: leg.from }
    }

    fn advance(&mut self, eased: f64) -> Option<Move> {
        let point = point_along(self.leg.from, self.leg.to, eased);
        if point == self.at {
            return None;
        }
        self.at = point;
        Some(Move { window: self.leg.window, to: point + self.leg.window_offset })
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
        Move { window, to: Point { x: to.0, y: to.1 } }
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
}
