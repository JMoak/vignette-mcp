use std::time::{Duration, Instant};

use vignette_core::easing::between;
use vignette_core::lattice::Rect;

use crate::look::{BACKDROP, RULE};
use crate::pane::Pane;
use crate::win32::{self, Fonts, PaneFace, Window};

/// The panes that stand as windows, and the only thing that moves them.
pub struct Stage {
    fonts: Box<Fonts>,
    shown: Vec<Shown>,
    pub scale: f64,
    pub area: Rect,
}

struct Shown {
    window: Window,
    face: Box<PaneFace>,
    bounds: Rect,
}

#[derive(Clone, Copy, Debug)]
pub struct Track {
    pub pane: usize,
    pub to: Rect,
    pub start_ms: f64,
    pub glide_ms: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Glide {
    pub frames: u32,
    pub mean_us: u64,
    pub worst_us: u64,
    pub late: u32,
    pub period_us: u64,
    pub took_ms: f64,
}

impl Stage {
    pub fn open() -> Option<Stage> {
        if !win32::enable_per_monitor_dpi() || !win32::register_class() {
            return None;
        }
        let scale = win32::scale();
        Some(Stage {
            fonts: Box::new(win32::fonts(scale)),
            shown: Vec::new(),
            scale,
            area: win32::work_area(),
        })
    }

    /// Opens a pane where it is told, without taking focus, and says how long that took.
    pub fn show(&mut self, pane: Pane, bounds: Rect) -> Option<(usize, Duration)> {
        let began = Instant::now();
        let face = Box::new(PaneFace { fonts: &raw const *self.fonts, painted: Box::new(pane) });
        let window = win32::create_pane(bounds, &raw const *face)?;
        win32::set_border(window, RULE);
        win32::show_without_focus(window);
        self.shown.push(Shown { window, face, bounds });
        Some((self.shown.len() - 1, began.elapsed()))
    }

    pub fn count(&self) -> usize {
        self.shown.len()
    }

    pub fn bounds(&self, pane: usize) -> Rect {
        self.shown[pane].bounds
    }

    /// Moves the tracked panes together, one batch a frame, in step with the display.
    pub fn glide(&mut self, tracks: &[Track], sized: bool) -> Glide {
        let from: Vec<Rect> = tracks.iter().map(|track| self.shown[track.pane].bounds).collect();
        let ends = tracks.iter().map(|track| track.start_ms + track.glide_ms).fold(0.0, f64::max);
        let period_us = win32::refresh_period_us().unwrap_or(16_667);
        let began = Instant::now();
        let mut last = began;
        let mut spans: Vec<u64> = Vec::new();
        loop {
            win32::wait_for_composition();
            let now = Instant::now();
            let at_ms = now.duration_since(began).as_secs_f64() * 1_000.0;
            let moves: Vec<(Window, Rect)> = tracks
                .iter()
                .zip(&from)
                .filter(|(track, _)| at_ms >= track.start_ms)
                .map(|(track, from)| {
                    let to = between(*from, track.to, (at_ms - track.start_ms) / track.glide_ms);
                    self.shown[track.pane].bounds = to;
                    (self.shown[track.pane].window, to)
                })
                .collect();
            if !moves.is_empty() {
                win32::move_as_one_batch(&moves, sized);
            }
            win32::pump();
            if !spans.is_empty() || now != began {
                spans.push(now.duration_since(last).as_micros() as u64);
            }
            last = now;
            if at_ms >= ends {
                break;
            }
        }
        let frames = spans.len() as u32;
        let late = spans.iter().filter(|span| **span > period_us * 3 / 2).count() as u32;
        Glide {
            frames,
            mean_us: spans.iter().sum::<u64>() / u64::from(frames.max(1)),
            worst_us: spans.iter().copied().max().unwrap_or(0),
            late,
            period_us,
            took_ms: began.elapsed().as_secs_f64() * 1_000.0,
        }
    }

    /// Keeps the windows answering for a while, moving nothing.
    pub fn hold(&self, time: Duration) {
        let until = Instant::now() + time;
        while Instant::now() < until && win32::pump() {
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    /// One picture of the area with every pane painted where it stands, as a BMP file.
    pub fn picture(&self, area: Rect) -> Vec<u8> {
        let faces: Vec<(Rect, &PaneFace)> =
            self.shown.iter().map(|shown| (shown.bounds, &*shown.face)).collect();
        win32::picture_of(area, BACKDROP, &self.fonts, &faces)
    }

    pub fn withdraw_all(&mut self) {
        for shown in self.shown.drain(..) {
            win32::destroy(shown.window);
        }
        win32::pump();
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.withdraw_all();
    }
}
