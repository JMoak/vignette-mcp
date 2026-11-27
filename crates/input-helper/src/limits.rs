//! How much input the helper takes in a moment. Generous for a person at a remote keyboard and
//! mouse, a stop for a console that has gone wrong. A release is never limited.

/// Pointer moves and wheel turns: a pointer reports at most a few hundred times a second.
pub const MOTION_PER_SECOND: f64 = 500.0;
pub const MOTION_BURST: f64 = 100.0;
/// Key and button presses: fast typing and key repeat stay well inside this.
pub const PRESSES_PER_SECOND: f64 = 50.0;
pub const PRESSES_BURST: f64 = 25.0;

#[derive(Debug)]
pub struct Bucket {
    capacity: f64,
    per_ms: f64,
    tokens: f64,
    at: Option<f64>,
}

impl Bucket {
    pub fn new(capacity: f64, per_second: f64) -> Bucket {
        Bucket { capacity, per_ms: per_second / 1000.0, tokens: capacity, at: None }
    }

    pub fn take(&mut self, now_ms: f64) -> bool {
        let elapsed = self.at.map_or(0.0, |at| (now_ms - at).max(0.0));
        self.at = Some(now_ms);
        self.tokens = (self.tokens + elapsed * self.per_ms).min(self.capacity);
        let taken = self.tokens >= 1.0;
        if taken {
            self.tokens -= 1.0;
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_a_burst_then_its_rate_and_refills_only_to_its_burst() {
        let mut presses = Bucket::new(PRESSES_BURST, PRESSES_PER_SECOND);
        let burst = (0..40).filter(|_| presses.take(1000.0)).count();
        assert_eq!(burst, 25);
        let next_second = (0..100).filter(|step| presses.take(1000.0 + f64::from(*step) * 10.0)).count();
        assert_eq!(next_second, 49);
        assert_eq!((0..40).filter(|_| presses.take(1_000_000.0)).count(), 25);
    }

    #[test]
    fn a_clock_that_steps_back_refills_nothing() {
        let mut motion = Bucket::new(2.0, MOTION_PER_SECOND);
        assert!(motion.take(5000.0) && motion.take(5000.0));
        assert!(!motion.take(4000.0));
    }
}
