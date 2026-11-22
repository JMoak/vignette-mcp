use crate::lattice::Rect;

/// The same fifth-power ease the mover and the app's own glides use, so nothing moves differently.
pub fn ease_out(progress: f64) -> f64 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(5)
}

pub fn between(from: Rect, to: Rect, progress: f64) -> Rect {
    let eased = ease_out(progress);
    let mix = |start: i32, end: i32| start + (f64::from(end - start) * eased).round() as i32;
    Rect { x: mix(from.x, to.x), y: mix(from.y, to.y), w: mix(from.w, to.w), h: mix(from.h, to.h) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_starts_where_it_was_and_ends_where_it_is_going() {
        let from = Rect { x: 0, y: 0, w: 100, h: 100 };
        let to = Rect { x: 300, y: 40, w: 200, h: 100 };
        assert_eq!(between(from, to, 0.0), from);
        assert_eq!(between(from, to, 1.0), to);
        assert_eq!(between(from, to, 7.0), to);
        assert!(between(from, to, 0.5).x > 150, "it covers most of the way early");
    }
}
