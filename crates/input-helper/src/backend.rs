//! What the helper asks of the operating system, behind one trait: the displays, the input it
//! injects, and a watch on the input it did not inject. Everything above this is tested against a
//! fake; each platform's backend is the only code that touches the system.

use crate::keys::Key;
use crate::protocol::{Button, Rect};
use crate::screen::{Monitor, Spot};

/// A browser scrolls about this far for one notch of a mouse wheel.
pub const BROWSER_PIXELS_PER_NOTCH: f64 = 100.0;

/// What the helper's one loop waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A line from the console; `None` for one that could not be read as text.
    Line(Option<String>),
    /// The console closed the helper's input.
    Closed,
    /// Someone used this machine's own keyboard, mouse, touchpad or touch screen.
    Physical,
    /// The system asked the helper to stop.
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Move(Spot),
    Press(Button),
    Release(Button),
    /// Browser wheel deltas: positive `dy` scrolls down, positive `dx` scrolls right.
    Wheel {
        dx: f64,
        dy: f64,
    },
    KeyDown(&'static Key),
    KeyUp(&'static Key),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The system will not let the helper inject or watch input at all.
    Access,
    /// The system did not take this input.
    Blocked,
}

pub trait Backend {
    fn monitors(&mut self) -> Vec<Monitor>;
    /// Starts reporting input the helper did not inject. Called on every grant; starts once.
    fn watch(&mut self) -> Result<(), Fault>;
    /// Injects the acts in order, as one batch where the system allows.
    fn send(&mut self, acts: &[Act]) -> Result<(), Fault>;
}

/// Speaks the whole protocol and touches nothing: for proving the helper's protocol and
/// lifecycle as a process on a machine where injecting input is not allowed.
pub struct Dry;

impl Dry {
    pub const DISPLAY: Rect = Rect { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 };
}

impl Backend for Dry {
    fn monitors(&mut self) -> Vec<Monitor> {
        vec![Monitor { rect: Dry::DISPLAY, scale: Some(1.0) }]
    }

    fn watch(&mut self) -> Result<(), Fault> {
        Ok(())
    }

    fn send(&mut self, _acts: &[Act]) -> Result<(), Fault> {
        Ok(())
    }
}

/// Turns a stream of fractional wheel amounts into whole steps without losing the remainder, so
/// a trackpad's many small deltas still add up to the same scroll.
#[derive(Debug, Default)]
pub struct Notches {
    rest: f64,
}

impl Notches {
    pub fn take(&mut self, amount: f64) -> i32 {
        let total = self.rest + amount;
        let whole = total.trunc();
        self.rest = total - whole;
        whole as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_wheel_deltas_add_up_to_whole_steps_in_either_direction() {
        let mut notches = Notches::default();
        let amounts = [0.25, 0.25, 0.5, -0.75, -0.75, 2.5];
        let steps: Vec<i32> = amounts.into_iter().map(|amount| notches.take(amount)).collect();
        assert_eq!(steps, [0, 0, 1, 0, -1, 2]);
    }
}
