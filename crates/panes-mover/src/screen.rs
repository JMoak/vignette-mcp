use std::fmt;
use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Window(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub at: Point,
    pub window_offset: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizing {
    pub size: Size,
    pub window_offset: Size,
}

impl fmt::Display for Window {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{:x}", self.0)
    }
}

impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point {
        Point { x: self.x.saturating_add(other.x), y: self.y.saturating_add(other.y) }
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point {
        Point { x: self.x.saturating_sub(other.x), y: self.y.saturating_sub(other.y) }
    }
}

impl Add for Size {
    type Output = Size;

    fn add(self, other: Size) -> Size {
        Size {
            width: self.width.saturating_add(other.width),
            height: self.height.saturating_add(other.height),
        }
    }
}

impl Sub for Size {
    type Output = Size;

    fn sub(self, other: Size) -> Size {
        Size {
            width: self.width.saturating_sub(other.width),
            height: self.height.saturating_sub(other.height),
        }
    }
}
