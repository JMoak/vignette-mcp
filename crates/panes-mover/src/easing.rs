use crate::screen::{Point, Size};

pub fn eased_to_rest(progress: f64) -> f64 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(5)
}

pub fn point_along(from: Point, to: Point, eased: f64) -> Point {
    Point { x: coordinate_along(from.x, to.x, eased), y: coordinate_along(from.y, to.y, eased) }
}

pub fn size_along(from: Size, to: Size, eased: f64) -> Size {
    Size {
        width: coordinate_along(from.width, to.width, eased),
        height: coordinate_along(from.height, to.height, eased),
    }
}

fn coordinate_along(from: i32, to: i32, eased: f64) -> i32 {
    let travelled = (f64::from(to) - f64::from(from)) * eased;
    (f64::from(from) + rounded_like_javascript(travelled)) as i32
}

// `Math.round` sends a half towards positive infinity; `f64::round` sends it away from zero.
fn rounded_like_javascript(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 { floor + 1.0 } else { floor }
}

#[cfg(test)]
mod tests {
    use super::*;

    // V8's `**` and `powi` round a fifth power differently in the last bit, never by more than this.
    const V8_ROUNDING_SLACK: f64 = 2.5e-16;

    const EASED_BY_GLIDE_TS: [(f64, f64); 19] = [
        (-0.5, 0.0),
        (0.0, 0.0),
        (0.01, 0.04900995010000009),
        (0.1, 0.40950999999999993),
        (0.2, 0.6723199999999999),
        (0.25, 0.7626953125),
        (1.0 / 3.0, 0.868312757201646),
        (0.4, 0.9222400000000001),
        (0.5, 0.96875),
        (0.6, 0.98976),
        (2.0 / 3.0, 0.9958847736625515),
        (0.75, 0.9990234375),
        (0.8, 0.99968),
        (0.9, 0.99999),
        (0.95, 0.9999996875),
        (0.99, 0.9999999999),
        (0.999, 0.999999999999999),
        (1.0, 1.0),
        (1.5, 1.0),
    ];

    const PROGRESS_STEPS: [f64; 7] = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0];

    const COORDINATES_BY_GLIDE_TS: [(i32, i32, [i32; 7]); 6] = [
        (100, 1060, [100, 493, 832, 1030, 1059, 1060, 1060]),
        (200, 440, [200, 298, 383, 433, 440, 440, 440]),
        (0, -5, [0, -2, -4, -5, -5, -5, -5]),
        (0, 5, [0, 2, 4, 5, 5, 5, 5]),
        (-7, -1000, [-7, -414, -764, -969, -999, -1000, -1000]),
        (3, -999, [3, -407, -761, -968, -998, -999, -999]),
    ];

    const HALFWAY_BY_GLIDE_TS: [(i32, i32, i32); 6] =
        [(100, 1060, 580), (200, 440, 320), (0, -5, -2), (0, 5, 3), (-7, -1000, -503), (3, -999, -498)];

    #[test]
    fn eases_like_glide_ts() {
        for (progress, expected) in EASED_BY_GLIDE_TS {
            let eased = eased_to_rest(progress);
            assert!(
                (eased - expected).abs() <= V8_ROUNDING_SLACK,
                "progress {progress}: {eased} is not {expected}"
            );
        }
    }

    #[test]
    fn matches_glide_ts_bit_for_bit_where_the_power_is_exact() {
        let exact: [(f64, f64); 5] =
            [(0.0, 0.0), (0.25, 0.7626953125), (0.5, 0.96875), (0.75, 0.9990234375), (1.0, 1.0)];
        for (progress, expected) in exact {
            assert_eq!(eased_to_rest(progress).to_bits(), expected.to_bits(), "progress {progress}");
        }
    }

    #[test]
    fn never_overshoots_or_runs_backwards() {
        let mut previous = 0.0;
        for step in 0..=10_000 {
            let eased = eased_to_rest(f64::from(step) / 10_000.0);
            assert!((previous..=1.0).contains(&eased));
            previous = eased;
        }
    }

    #[test]
    fn clamps_progress_outside_the_glide() {
        assert_eq!(eased_to_rest(-3.0), 0.0);
        assert_eq!(eased_to_rest(7.0), 1.0);
    }

    #[test]
    fn places_coordinates_like_glide_ts() {
        for (from, to, expected) in COORDINATES_BY_GLIDE_TS {
            let placed = PROGRESS_STEPS.map(|progress| coordinate_along(from, to, eased_to_rest(progress)));
            assert_eq!(placed, expected, "from {from} to {to}");
        }
    }

    #[test]
    fn rounds_a_half_pixel_like_math_round() {
        for (from, to, expected) in HALFWAY_BY_GLIDE_TS {
            assert_eq!(coordinate_along(from, to, 0.5), expected, "from {from} to {to}");
        }
    }

    #[test]
    fn moves_both_axes_together() {
        let from = Point { x: 100, y: 200 };
        let to = Point { x: 1060, y: 440 };
        assert_eq!(point_along(from, to, eased_to_rest(0.25)), Point { x: 832, y: 383 });
        assert_eq!(point_along(from, to, 1.0), to);
    }
}
