use crate::protocol::Report;

#[derive(Default)]
pub struct FrameStats {
    frames: u64,
    missed: u64,
    issuing_us: u64,
    worst_us: u64,
}

impl FrameStats {
    pub fn issued(&mut self, took_us: u64) {
        self.frames += 1;
        self.issuing_us += took_us;
        self.worst_us = self.worst_us.max(took_us);
    }

    pub fn waited(&mut self, interval_us: u64, refresh_us: u64) {
        self.missed += missed_refreshes(interval_us, refresh_us);
    }

    pub fn take(&mut self) -> Report {
        let taken = std::mem::take(self);
        Report {
            frames: taken.frames,
            missed: taken.missed,
            mean_us: taken.issuing_us.checked_div(taken.frames).unwrap_or(0),
            worst_us: taken.worst_us,
        }
    }
}

pub fn missed_refreshes(interval_us: u64, refresh_us: u64) -> u64 {
    let refreshes = (interval_us + refresh_us / 2).checked_div(refresh_us).unwrap_or(1);
    refreshes.saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIXTY_HERTZ_US: u64 = 16_667;

    #[test]
    fn a_frame_on_every_refresh_misses_none() {
        assert_eq!(missed_refreshes(16_667, SIXTY_HERTZ_US), 0);
        assert_eq!(missed_refreshes(15_900, SIXTY_HERTZ_US), 0);
        assert_eq!(missed_refreshes(17_800, SIXTY_HERTZ_US), 0);
        assert_eq!(missed_refreshes(400, SIXTY_HERTZ_US), 0);
    }

    #[test]
    fn a_late_frame_misses_the_refreshes_it_slept_through() {
        assert_eq!(missed_refreshes(33_400, SIXTY_HERTZ_US), 1);
        assert_eq!(missed_refreshes(50_100, SIXTY_HERTZ_US), 2);
        assert_eq!(missed_refreshes(26_000, SIXTY_HERTZ_US), 1);
    }

    #[test]
    fn an_unknown_refresh_period_misses_none() {
        assert_eq!(missed_refreshes(50_000, 0), 0);
    }

    #[test]
    fn reports_mean_and_worst_issue_time_then_starts_again() {
        let mut stats = FrameStats::default();
        stats.issued(100);
        stats.issued(300);
        stats.issued(80);
        stats.waited(33_400, SIXTY_HERTZ_US);

        assert_eq!(stats.take(), Report { frames: 3, missed: 1, mean_us: 160, worst_us: 300 });
        assert_eq!(stats.take(), Report::default());
    }
}
