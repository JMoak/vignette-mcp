//! Where a remote keeper's pointer lands: finding the captured display among this machine's
//! displays, and turning a point normalized to it into the system's own coordinates.

use crate::protocol::{Display, Rect};

/// One of this machine's displays as the backend sees it, in the coordinates it injects in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub rect: Rect,
    /// The backend's units per unit of the console's display bounds, if the backend knows it
    /// (Windows: the display's DPI over 96; macOS: 1, both counting in points). `None` takes the
    /// console's scale as the scale of every display (X11 has one scale for the whole screen).
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub x: f64,
    pub y: f64,
}

const SAME_SCALE: f64 = 0.01;

/// The captured display's rectangle in the backend's coordinates, or `None` when this machine
/// has no display that is surely the one the console captured.
///
/// Electron on Windows lays displays of different scales out in its own units in a way the
/// system's pixels cannot be turned back into, so with mixed scales a display is found by its size
/// and scale alone and must be the only one of its kind. Where every display shares the console's
/// scale, positions scale too, and the display must also stand where the console says.
pub fn locate(display: &Display, monitors: &[Monitor]) -> Option<Rect> {
    if !has_area(&display.bounds) || display.scale_factor <= 0.0 {
        return None;
    }
    if let Some(physical) = display.physical {
        return monitors.iter().map(|monitor| monitor.rect).find(|rect| same_rect(rect, &physical));
    }
    let scale_of = |monitor: &Monitor| monitor.scale.unwrap_or(display.scale_factor);
    let shares_scale = |monitor: &Monitor| (scale_of(monitor) - display.scale_factor).abs() <= SAME_SCALE;
    let bounds = display.bounds;
    let scale = display.scale_factor;
    let same_size = |monitor: &&Monitor| {
        shares_scale(monitor)
            && (monitor.rect.width - bounds.width * scale).abs() <= scale.max(1.0)
            && (monitor.rect.height - bounds.height * scale).abs() <= scale.max(1.0)
    };
    let candidates: Vec<&Monitor> = monitors.iter().filter(same_size).collect();
    if monitors.iter().all(shares_scale) {
        let stands_there = |monitor: &&Monitor| {
            (monitor.rect.x / scale - bounds.x).abs() <= 1.0
                && (monitor.rect.y / scale - bounds.y).abs() <= 1.0
        };
        return candidates.into_iter().find(stands_there).map(|monitor| monitor.rect);
    }
    match candidates.as_slice() {
        [only] => Some(only.rect),
        _ => None,
    }
}

/// The spot on `area` that a point normalized to it names, never past its last row or column.
pub fn spot_on(area: &Rect, x: f64, y: f64) -> Spot {
    let along =
        |unit: f64, start: f64, length: f64| start + (unit.clamp(0.0, 1.0) * length).min(length - 1.0);
    Spot { x: along(x, area.x, area.width), y: along(y, area.y, area.height) }
}

/// Windows' absolute input coordinate (0 to 65535 across the virtual desktop) for one pixel.
/// The smallest value that maps onto the pixel: it lands there whether Windows rounds or floors,
/// and whether it divides by 65536 or 65535, on any virtual desktop narrower than 16384 pixels.
pub fn virtual_desk(pixel: i32, origin: i32, extent: i32) -> i32 {
    if extent <= 0 {
        return 0;
    }
    let offset = i64::from(pixel) - i64::from(origin);
    let extent = i64::from(extent);
    let scaled = (offset * 65_536 + extent - 1).div_euclid(extent);
    scaled.clamp(0, 65_535) as i32
}

fn has_area(rect: &Rect) -> bool {
    rect.width >= 1.0 && rect.height >= 1.0
}

fn same_rect(a: &Rect, b: &Rect) -> bool {
    has_area(b)
        && (a.x - b.x).abs() <= 1.0
        && (a.y - b.y).abs() <= 1.0
        && (a.width - b.width).abs() <= 1.0
        && (a.height - b.height).abs() <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    fn monitor(x: f64, y: f64, width: f64, height: f64, scale: Option<f64>) -> Monitor {
        Monitor { rect: rect(x, y, width, height), scale }
    }

    fn display(bounds: Rect, scale_factor: f64) -> Display {
        Display { bounds, scale_factor, physical: None }
    }

    // Windows, a laptop at 200% with a 1440p display at 100% and a 4K display at 150% beside it.
    // Electron puts the outer two in its own units where the pixels alone would not.
    fn mixed() -> Vec<Monitor> {
        vec![
            monitor(0.0, 0.0, 2880.0, 1800.0, Some(2.0)),
            monitor(2880.0, 0.0, 2560.0, 1440.0, Some(1.0)),
            monitor(5440.0, -300.0, 3840.0, 2160.0, Some(1.5)),
        ]
    }

    #[test]
    fn finds_a_display_by_size_and_scale_when_scales_are_mixed() {
        let monitors = mixed();
        let laptop = display(rect(0.0, 0.0, 1440.0, 900.0), 2.0);
        let middle = display(rect(1440.0, 0.0, 2560.0, 1440.0), 1.0);
        let right = display(rect(4000.0, -300.0, 2560.0, 1440.0), 1.5);
        assert_eq!(locate(&laptop, &monitors), Some(monitors[0].rect));
        assert_eq!(locate(&middle, &monitors), Some(monitors[1].rect));
        assert_eq!(locate(&right, &monitors), Some(monitors[2].rect));
        assert_eq!(locate(&display(rect(0.0, 0.0, 1920.0, 1080.0), 1.0), &monitors), None);
    }

    #[test]
    fn refuses_twins_it_cannot_tell_apart_unless_given_their_pixels() {
        let mut monitors = mixed();
        monitors.push(monitor(-2560.0, 0.0, 2560.0, 1440.0, Some(1.0)));
        let twin = display(rect(1440.0, 0.0, 2560.0, 1440.0), 1.0);
        assert_eq!(locate(&twin, &monitors), None);
        let given = Display { physical: Some(rect(-2560.0, 0.0, 2560.0, 1440.0)), ..twin };
        assert_eq!(locate(&given, &monitors), Some(monitors[3].rect));
        let elsewhere = Display { physical: Some(rect(-2560.0, 10.0, 2560.0, 1440.0)), ..twin };
        assert_eq!(locate(&elsewhere, &monitors), None);
    }

    #[test]
    fn tells_twins_apart_by_place_when_every_display_shares_one_scale() {
        let windows =
            [monitor(0.0, 0.0, 2560.0, 1440.0, Some(1.25)), monitor(2560.0, 0.0, 2560.0, 1440.0, Some(1.25))];
        let right = display(rect(2048.0, 0.0, 2048.0, 1152.0), 1.25);
        assert_eq!(locate(&right, &windows), Some(windows[1].rect));
        let x11 = [monitor(0.0, 0.0, 3840.0, 2160.0, None), monitor(3840.0, 0.0, 3840.0, 2160.0, None)];
        let right = display(rect(1920.0, 0.0, 1920.0, 1080.0), 2.0);
        assert_eq!(locate(&right, &x11), Some(x11[1].rect));
        let gone = display(rect(3840.0, 0.0, 1920.0, 1080.0), 2.0);
        assert_eq!(locate(&gone, &x11), None);
    }

    #[test]
    fn takes_macos_points_as_they_are_and_refuses_a_display_with_no_area() {
        let mac =
            [monitor(0.0, 0.0, 1512.0, 982.0, Some(1.0)), monitor(-1920.0, -98.0, 1920.0, 1080.0, Some(1.0))];
        let left = display(rect(-1920.0, -98.0, 1920.0, 1080.0), 1.0);
        assert_eq!(locate(&left, &mac), Some(mac[1].rect));
        assert_eq!(locate(&display(rect(-1920.0, -98.0, 0.0, 1080.0), 1.0), &mac), None);
        assert_eq!(locate(&display(rect(-1920.0, -98.0, 1920.0, 1080.0), 0.0), &mac), None);
    }

    #[test]
    fn a_normalized_point_lands_on_the_display_and_never_past_its_edge() {
        let area = rect(-1920.0, 0.0, 1920.0, 1080.0);
        assert_eq!(spot_on(&area, 0.0, 0.0), Spot { x: -1920.0, y: 0.0 });
        assert_eq!(spot_on(&area, 0.5, 0.5), Spot { x: -960.0, y: 540.0 });
        assert_eq!(spot_on(&area, 1.0, 1.0), Spot { x: -1.0, y: 1079.0 });
        assert_eq!(spot_on(&area, 0.999_99, 0.0).x.floor(), -1.0);
    }

    #[test]
    fn an_absolute_coordinate_lands_on_its_own_pixel_however_windows_converts_it() {
        for (origin, extent) in
            [(0, 1920), (-2560, 2560 + 3840), (-1366, 1366 + 1920 + 2880), (0, 7), (0, 16383)]
        {
            for pixel in origin..origin + extent {
                let absolute = i64::from(virtual_desk(pixel, origin, extent));
                let extent = i64::from(extent);
                let expected = i64::from(pixel - origin);
                assert!((0..=65_535).contains(&absolute));
                assert_eq!(absolute * extent / 65_536, expected, "floor, pixel {pixel} of {extent}");
                assert_eq!(
                    (absolute * extent + 32_768) / 65_536,
                    expected,
                    "round, pixel {pixel} of {extent}"
                );
                assert_eq!(absolute * extent / 65_535, expected, "floor 65535, pixel {pixel} of {extent}");
                assert_eq!(
                    (2 * absolute * extent + 65_535) / 131_070,
                    expected,
                    "round 65535, pixel {pixel}"
                );
            }
        }
    }
}
