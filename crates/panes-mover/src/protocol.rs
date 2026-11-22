use std::fmt;
use std::ops::RangeInclusive;
use std::str::{FromStr, SplitAsciiWhitespace};

use crate::screen::{Point, Window};

pub const VERSION: u32 = 1;
pub const MAX_LINE_BYTES: usize = 4096;
pub const MAX_WINDOWS: usize = 64;
pub const MAX_COORDINATE: i32 = 1_000_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Glide { id: u32, duration_ms: u32, targets: Vec<Target> },
    Release { window: Window },
    Border { window: Window, colour: Option<Rgb> },
    Stats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub window: Window,
    pub at: Point,
    pub to: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Ready { parent_pid: u32 },
    Settled { id: u32 },
    Refused { window: Option<Window>, reason: Reason },
    Stats(Report),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub frames: u64,
    pub missed: u64,
    pub mean_us: u64,
    pub worst_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    UnknownCommand,
    MissingArgument,
    UnexpectedArgument,
    BadId,
    BadDuration,
    BadWindow,
    BadPoint,
    BadColour,
    TooManyWindows,
    DuplicateWindow,
    LineTooLong,
    NotText,
    NoSuchWindow,
    NotParentWindow,
    NotTopLevel,
    BorderFailed(i32),
}

pub fn parse(line: &str) -> Result<Option<Command>, Reason> {
    let mut words = line.split_ascii_whitespace();
    let Some(verb) = words.next() else {
        return Ok(None);
    };
    let command = match verb {
        "glide" => glide(&mut words)?,
        "release" => Command::Release { window: window(next(&mut words)?)? },
        "border" => border(&mut words)?,
        "stats" => Command::Stats,
        _ => return Err(Reason::UnknownCommand),
    };
    match words.next() {
        Some(_) => Err(Reason::UnexpectedArgument),
        None => Ok(Some(command)),
    }
}

impl fmt::Display for Event {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Ready { parent_pid } => write!(out, "ready {VERSION} {parent_pid}"),
            Event::Settled { id } => write!(out, "settled {id}"),
            Event::Refused { window: Some(window), reason } => write!(out, "refused {window} {reason}"),
            Event::Refused { window: None, reason } => write!(out, "refused - {reason}"),
            Event::Stats(report) => write!(out, "stats {report}"),
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Report { frames, missed, mean_us, worst_us } = self;
        write!(out, "frames={frames} missed={missed} mean_us={mean_us} worst_us={worst_us}")
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Reason::UnknownCommand => "unknown-command",
            Reason::MissingArgument => "missing-argument",
            Reason::UnexpectedArgument => "unexpected-argument",
            Reason::BadId => "bad-id",
            Reason::BadDuration => "bad-duration",
            Reason::BadWindow => "bad-window",
            Reason::BadPoint => "bad-point",
            Reason::BadColour => "bad-colour",
            Reason::TooManyWindows => "too-many-windows",
            Reason::DuplicateWindow => "duplicate-window",
            Reason::LineTooLong => "line-too-long",
            Reason::NotText => "not-text",
            Reason::NoSuchWindow => "no-such-window",
            Reason::NotParentWindow => "not-parent-window",
            Reason::NotTopLevel => "not-top-level",
            Reason::BorderFailed(result) => return write!(out, "border-failed:{result:08x}"),
        };
        out.write_str(text)
    }
}

type Words<'line> = SplitAsciiWhitespace<'line>;

fn glide(words: &mut Words) -> Result<Command, Reason> {
    let id = number(next(words)?).ok_or(Reason::BadId)?;
    let duration_ms = number(next(words)?).ok_or(Reason::BadDuration)?;
    let mut targets: Vec<Target> = Vec::new();
    for word in words {
        let target = target(word)?;
        if targets.len() == MAX_WINDOWS {
            return Err(Reason::TooManyWindows);
        }
        if targets.iter().any(|earlier| earlier.window == target.window) {
            return Err(Reason::DuplicateWindow);
        }
        targets.push(target);
    }
    if targets.is_empty() {
        return Err(Reason::MissingArgument);
    }
    Ok(Command::Glide { id, duration_ms, targets })
}

fn border(words: &mut Words) -> Result<Command, Reason> {
    let window = window(next(words)?)?;
    let colour = match next(words)? {
        "clear" => None,
        text => Some(colour(text)?),
    };
    Ok(Command::Border { window, colour })
}

fn next<'line>(words: &mut Words<'line>) -> Result<&'line str, Reason> {
    words.next().ok_or(Reason::MissingArgument)
}

fn target(word: &str) -> Result<Target, Reason> {
    let (handle, journey) = word.split_once('@').ok_or(Reason::BadWindow)?;
    let (at, to) = journey.split_once('>').ok_or(Reason::BadPoint)?;
    Ok(Target { window: window(handle)?, at: point(at)?, to: point(to)? })
}

fn point(text: &str) -> Result<Point, Reason> {
    let (x, y) = text.split_once(',').ok_or(Reason::BadPoint)?;
    Ok(Point { x: coordinate(x)?, y: coordinate(y)? })
}

fn window(text: &str) -> Result<Window, Reason> {
    let handle = hexadecimal(text, 1..=16).ok_or(Reason::BadWindow)?;
    usize::try_from(handle).ok().filter(|handle| *handle != 0).map(Window).ok_or(Reason::BadWindow)
}

fn colour(text: &str) -> Result<Rgb, Reason> {
    let value = hexadecimal(text, 6..=6).ok_or(Reason::BadColour)?;
    let [.., red, green, blue] = value.to_be_bytes();
    Ok(Rgb { red, green, blue })
}

fn coordinate(text: &str) -> Result<i32, Reason> {
    let (sign, digits) = match text.strip_prefix('-') {
        Some(digits) => (-1, digits),
        None => (1, text),
    };
    match number::<i32>(digits) {
        Some(distance) if distance <= MAX_COORDINATE => Ok(sign * distance),
        _ => Err(Reason::BadPoint),
    }
}

fn number<Number: FromStr>(text: &str) -> Option<Number> {
    let plain = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    plain.then(|| text.parse().ok()).flatten()
}

fn hexadecimal(text: &str, digits: RangeInclusive<usize>) -> Option<u64> {
    let plain = digits.contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_hexdigit());
    plain.then(|| u64::from_str_radix(text, 16).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journey(handle: usize, at: (i32, i32), to: (i32, i32)) -> Target {
        Target { window: Window(handle), at: Point { x: at.0, y: at.1 }, to: Point { x: to.0, y: to.1 } }
    }

    fn refusal(line: &str) -> Reason {
        parse(line).expect_err(line)
    }

    #[test]
    fn reads_a_glide_with_where_each_window_is_and_where_it_goes() {
        let command = parse("glide 7 180 1a2b3c@80,80>1200,340 ABCDEF@-8,0>-1520,-4").unwrap();
        let targets = vec![journey(0x1a2b3c, (80, 80), (1200, 340)), journey(0xabcdef, (-8, 0), (-1520, -4))];
        assert_eq!(command, Some(Command::Glide { id: 7, duration_ms: 180, targets }));
    }

    #[test]
    fn reads_a_jump_as_a_glide_of_no_duration() {
        let command = parse("glide 0 0 f@0,0>9,9").unwrap();
        let targets = vec![journey(0xf, (0, 0), (9, 9))];
        assert_eq!(command, Some(Command::Glide { id: 0, duration_ms: 0, targets }));
    }

    #[test]
    fn reads_release_border_and_stats() {
        let orange = Rgb { red: 0xff, green: 0x88, blue: 0x00 };
        assert_eq!(parse("release 2f0a").unwrap(), Some(Command::Release { window: Window(0x2f0a) }));
        assert_eq!(
            parse("border 2f0a ff8800").unwrap(),
            Some(Command::Border { window: Window(0x2f0a), colour: Some(orange) })
        );
        assert_eq!(
            parse("border 2f0a clear").unwrap(),
            Some(Command::Border { window: Window(0x2f0a), colour: None })
        );
        assert_eq!(parse("stats").unwrap(), Some(Command::Stats));
    }

    #[test]
    fn tolerates_blank_lines_carriage_returns_and_extra_spaces() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("  \r").unwrap(), None);
        assert_eq!(parse("  stats \r").unwrap(), Some(Command::Stats));
        assert_eq!(parse("release\t2f0a\r").unwrap(), Some(Command::Release { window: Window(0x2f0a) }));
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert_eq!(refusal("resize 2f0a 10,10"), Reason::UnknownCommand);
        assert_eq!(refusal("GLIDE 1 1 f@0,0>1,1"), Reason::UnknownCommand);
        assert_eq!(refusal("stats now"), Reason::UnexpectedArgument);
        assert_eq!(refusal("release 2f0a 2f0b"), Reason::UnexpectedArgument);
        assert_eq!(refusal("border 2f0a ff8800 thick"), Reason::UnexpectedArgument);
    }

    #[test]
    fn refuses_a_glide_that_is_incomplete() {
        assert_eq!(refusal("glide"), Reason::MissingArgument);
        assert_eq!(refusal("glide 1"), Reason::MissingArgument);
        assert_eq!(refusal("glide 1 100"), Reason::MissingArgument);
        assert_eq!(refusal("release"), Reason::MissingArgument);
        assert_eq!(refusal("border 2f0a"), Reason::MissingArgument);
    }

    #[test]
    fn refuses_numbers_that_are_not_plain_digits() {
        assert_eq!(refusal("glide +1 100 f@0,0>1,1"), Reason::BadId);
        assert_eq!(refusal("glide -1 100 f@0,0>1,1"), Reason::BadId);
        assert_eq!(refusal("glide 4294967296 100 f@0,0>1,1"), Reason::BadId);
        assert_eq!(refusal("glide 1 1.5 f@0,0>1,1"), Reason::BadDuration);
        assert_eq!(refusal("glide 1 fast f@0,0>1,1"), Reason::BadDuration);
    }

    #[test]
    fn refuses_windows_that_are_not_handles() {
        assert_eq!(refusal("glide 1 100 0@0,0>1,1"), Reason::BadWindow);
        assert_eq!(refusal("glide 1 100 0x1f@0,0>1,1"), Reason::BadWindow);
        assert_eq!(refusal("glide 1 100 +1f@0,0>1,1"), Reason::BadWindow);
        assert_eq!(refusal("glide 1 100 12345678901234567@0,0>1,1"), Reason::BadWindow);
        assert_eq!(refusal("glide 1 100 1f"), Reason::BadWindow);
        assert_eq!(refusal("release window"), Reason::BadWindow);
    }

    #[test]
    fn refuses_a_target_without_both_of_its_points() {
        assert_eq!(refusal("glide 1 100 1f@10,20"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@10,20>"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@>10,20"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@10,20>30,40>50,60"), Reason::BadPoint);
    }

    #[test]
    fn refuses_points_that_are_not_two_whole_coordinates() {
        assert_eq!(refusal("glide 1 100 1f@0,0>10"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@0,0>10,"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@0,0>10,20,30"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@0,0>+10,20"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@--10,20>0,0"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@1.5,20>0,0"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@0,0>1000001,0"), Reason::BadPoint);
        assert_eq!(refusal("glide 1 100 1f@0,-1000001>0,0"), Reason::BadPoint);
    }

    #[test]
    fn accepts_the_furthest_coordinates() {
        let command = parse("glide 1 100 1f@-1000000,1000000>1000000,-1000000").unwrap();
        let far = MAX_COORDINATE;
        let targets = vec![journey(0x1f, (-far, far), (far, -far))];
        assert_eq!(command, Some(Command::Glide { id: 1, duration_ms: 100, targets }));
    }

    #[test]
    fn refuses_a_window_named_twice_in_one_glide() {
        assert_eq!(refusal("glide 1 100 1f@0,0>1,1 2f@0,0>1,1 1F@0,0>5,5"), Reason::DuplicateWindow);
    }

    #[test]
    fn refuses_more_windows_than_one_glide_may_carry() {
        let windows =
            |count: usize| (1..=count).map(|handle| format!(" {handle:x}@0,0>1,1")).collect::<String>();
        assert!(parse(&format!("glide 1 100{}", windows(MAX_WINDOWS))).is_ok());
        assert_eq!(refusal(&format!("glide 1 100{}", windows(MAX_WINDOWS + 1))), Reason::TooManyWindows);
    }

    #[test]
    fn the_largest_glide_fits_in_one_line() {
        let furthest = format!("-{MAX_COORDINATE},-{MAX_COORDINATE}");
        let widest = |handle: u64| format!(" {handle:x}@{furthest}>{furthest}");
        let windows: String = (0..MAX_WINDOWS as u64).map(|index| widest(u64::MAX - index)).collect();
        let line = format!("glide {} {}{windows}", u32::MAX, u32::MAX);
        assert!(line.len() <= MAX_LINE_BYTES);
        assert!(parse(&line).is_ok());
    }

    #[test]
    fn refuses_colours_that_are_not_six_hex_digits() {
        assert_eq!(refusal("border 2f0a f80"), Reason::BadColour);
        assert_eq!(refusal("border 2f0a #ff8800"), Reason::BadColour);
        assert_eq!(refusal("border 2f0a ff8800ff"), Reason::BadColour);
        assert_eq!(refusal("border 2f0a orange"), Reason::BadColour);
        assert_eq!(refusal("border 2f0a none"), Reason::BadColour);
    }

    #[test]
    fn writes_each_event_as_one_line_of_text() {
        let report = Report { frames: 90, missed: 1, mean_us: 85, worst_us: 240 };
        let refused = |window, reason| Event::Refused { window, reason }.to_string();
        assert_eq!(Event::Ready { parent_pid: 4242 }.to_string(), "ready 1 4242");
        assert_eq!(Event::Settled { id: 7 }.to_string(), "settled 7");
        assert_eq!(refused(Some(Window(0x2F0A)), Reason::NotParentWindow), "refused 2f0a not-parent-window");
        assert_eq!(refused(None, Reason::UnknownCommand), "refused - unknown-command");
        assert_eq!(
            refused(Some(Window(0x2f0a)), Reason::BorderFailed(-2147024891)),
            "refused 2f0a border-failed:80070005"
        );
        assert_eq!(Event::Stats(report).to_string(), "stats frames=90 missed=1 mean_us=85 worst_us=240");
    }
}
