//! The console's helper protocol, version 1: one JSON object a line each way. Requests are read
//! exactly as `parseHelperRequest` in Keywork-app's `screen/helper/protocol.ts` reads them, and
//! replies are written field for field as its stand-in writes them.

use crate::json::{self, Value};

pub const VERSION: u32 = 1;
pub const MAX_LINE_BYTES: usize = 4096;

const LONGEST_TEXT: usize = 128;
const LARGEST_SEQ: f64 = 9_007_199_254_740_991.0;
const LARGEST_DELTA: f64 = 10_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    pub fn name(self) -> &'static str {
        match self {
            Button::Left => "left",
            Button::Middle => "middle",
            Button::Right => "right",
        }
    }

    fn named(name: &str) -> Option<Button> {
        [Button::Left, Button::Middle, Button::Right].into_iter().find(|button| button.name() == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The captured display, as the console's Electron sees it: bounds in its own units, the scale it
/// draws at, and optionally the same rectangle in the system's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    pub bounds: Rect,
    pub scale_factor: f64,
    pub physical: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grant {
    pub id: String,
    pub expires_at: f64,
    pub display: Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Move,
    Down,
    Up,
    Wheel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pointer {
    pub grant_id: String,
    pub seq: u64,
    pub action: Action,
    pub x: f64,
    pub y: f64,
    pub button: Option<Button>,
    pub dx: f64,
    pub dy: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub grant_id: String,
    pub seq: u64,
    pub code: String,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Hello { versions: Vec<f64> },
    Grant(Grant),
    Pointer(Pointer),
    Key(Key),
    ReleaseAll,
    Bye,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoGrant,
    WrongGrant,
    Expired,
    Version,
    Malformed,
    /// The grant names a display this machine does not have.
    Display,
    /// The system will not let the helper inject or watch input.
    Access,
    /// More input than the helper takes in a moment.
    Rate,
    /// The system did not take this input.
    Blocked,
}

impl Refusal {
    fn name(self) -> &'static str {
        match self {
            Refusal::NoGrant => "no-grant",
            Refusal::WrongGrant => "wrong-grant",
            Refusal::Expired => "expired",
            Refusal::Version => "version",
            Refusal::Malformed => "malformed",
            Refusal::Display => "display",
            Refusal::Access => "access",
            Refusal::Rate => "rate",
            Refusal::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Hello { implementation: &'static str },
    Granted { grant_id: String, expires_at: f64 },
    Applied { seq: u64 },
    Refused { seq: Option<u64>, reason: Refusal },
    Released { keys: Vec<&'static str>, buttons: Vec<Button> },
    LocalInput,
    Bye,
}

impl Reply {
    pub fn line(&self) -> String {
        let mut out = format!("{{\"v\":{VERSION},\"kind\":");
        match self {
            Reply::Hello { implementation } => {
                out.push_str(&format!("\"hello\",\"version\":{VERSION},\"impl\":"));
                json::write_text(&mut out, implementation);
            }
            Reply::Granted { grant_id, expires_at } => {
                out.push_str("\"granted\",\"grantId\":");
                json::write_text(&mut out, grant_id);
                out.push_str(",\"expiresAt\":");
                json::write_number(&mut out, *expires_at);
            }
            Reply::Applied { seq } => out.push_str(&format!("\"applied\",\"seq\":{seq}")),
            Reply::Refused { seq, reason } => {
                out.push_str("\"refused\"");
                if let Some(seq) = seq {
                    out.push_str(&format!(",\"seq\":{seq}"));
                }
                out.push_str(&format!(",\"reason\":\"{}\"", reason.name()));
            }
            Reply::Released { keys, buttons } => {
                out.push_str("\"released\",\"keys\":");
                list(&mut out, keys.iter().copied());
                out.push_str(",\"buttons\":");
                list(&mut out, buttons.iter().map(|button| button.name()));
            }
            Reply::LocalInput => out.push_str("\"local-input\""),
            Reply::Bye => out.push_str("\"bye\""),
        }
        out.push_str("}\n");
        out
    }
}

fn list<'a>(out: &mut String, names: impl Iterator<Item = &'a str>) {
    out.push('[');
    for (index, name) in names.enumerate() {
        if index > 0 {
            out.push(',');
        }
        json::write_text(out, name);
    }
    out.push(']');
}

pub fn parse(line: &str) -> Option<Request> {
    let message = json::parse(line)?;
    if number(message.field("v")?)? != f64::from(VERSION) {
        return None;
    }
    match text_of(message.field("kind")?)? {
        "hello" => hello(&message),
        "grant" => grant(&message),
        "ptr" => pointer(&message),
        "key" => key(&message),
        "release-all" => Some(Request::ReleaseAll),
        "bye" => Some(Request::Bye),
        _ => None,
    }
}

fn hello(message: &Value) -> Option<Request> {
    let Value::List(items) = message.field("versions")? else { return None };
    let versions = items.iter().map(finite).collect::<Option<Vec<f64>>>()?;
    Some(Request::Hello { versions })
}

fn grant(message: &Value) -> Option<Request> {
    let display = message.field("display")?;
    let physical = match display.field("physical") {
        None => None,
        Some(rect) => Some(rect_of(rect)?),
    };
    Some(Request::Grant(Grant {
        id: short_text(message.field("grantId")?)?,
        expires_at: finite(message.field("expiresAt")?)?,
        display: Display {
            bounds: rect_of(display)?,
            scale_factor: finite(display.field("scaleFactor")?)?,
            physical,
        },
    }))
}

fn pointer(message: &Value) -> Option<Request> {
    let action = match text_of(message.field("action")?)? {
        "move" => Action::Move,
        "down" => Action::Down,
        "up" => Action::Up,
        "wheel" => Action::Wheel,
        _ => return None,
    };
    let button = match message.field("button") {
        None => None,
        Some(name) => Some(Button::named(text_of(name)?)?),
    };
    Some(Request::Pointer(Pointer {
        grant_id: short_text(message.field("grantId")?)?,
        seq: seq(message.field("seq")?)?,
        action,
        x: unit(message.field("x")?)?,
        y: unit(message.field("y")?)?,
        button,
        dx: delta(message.field("dx"))?,
        dy: delta(message.field("dy"))?,
    }))
}

fn key(message: &Value) -> Option<Request> {
    let Value::Bool(down) = message.field("down")? else { return None };
    Some(Request::Key(Key {
        grant_id: short_text(message.field("grantId")?)?,
        seq: seq(message.field("seq")?)?,
        code: short_text(message.field("code")?)?,
        down: *down,
    }))
}

fn rect_of(value: &Value) -> Option<Rect> {
    Some(Rect {
        x: finite(value.field("x")?)?,
        y: finite(value.field("y")?)?,
        width: finite(value.field("width")?)?,
        height: finite(value.field("height")?)?,
    })
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(*number),
        _ => None,
    }
}

fn finite(value: &Value) -> Option<f64> {
    number(value).filter(|number| number.is_finite())
}

fn unit(value: &Value) -> Option<f64> {
    finite(value).filter(|number| (0.0..=1.0).contains(number))
}

fn seq(value: &Value) -> Option<u64> {
    let number = finite(value)?;
    (number.fract() == 0.0 && number > 0.0 && number <= LARGEST_SEQ).then_some(number as u64)
}

// The console's own protocol leaves wheel deltas unchecked; the helper takes only numbers, and no
// more than the console's viewer ever sends.
fn delta(value: Option<&Value>) -> Option<f64> {
    match value {
        None => Some(0.0),
        Some(value) => finite(value).map(|number| number.clamp(-LARGEST_DELTA, LARGEST_DELTA)),
    }
}

fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::Text(text) => Some(text),
        _ => None,
    }
}

// The console counts a text's length in UTF-16 units, as JavaScript does.
fn short_text(value: &Value) -> Option<String> {
    let text = text_of(value)?;
    let length = text.encode_utf16().count();
    (1..=LONGEST_TEXT).contains(&length).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> Display {
        Display {
            bounds: Rect { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 },
            scale_factor: 1.0,
            physical: None,
        }
    }

    #[test]
    fn reads_every_request_the_console_sends() {
        let hello = parse(r#"{"v":1,"kind":"hello","versions":[1]}"#);
        assert_eq!(hello, Some(Request::Hello { versions: vec![1.0] }));
        let grant = r#"{"v":1,"kind":"grant","grantId":"g1","expiresAt":1764115200000,"display":{"x":0,"y":0,"width":1920,"height":1080,"scaleFactor":1}}"#;
        assert_eq!(
            parse(grant),
            Some(Request::Grant(Grant {
                id: "g1".into(),
                expires_at: 1_764_115_200_000.0,
                display: display()
            }))
        );
        let wheel = r#"{"v":1,"grantId":"g1","seq":7,"kind":"ptr","action":"wheel","x":0.5,"y":1,"dx":-3.5,"dy":120}"#;
        assert_eq!(
            parse(wheel),
            Some(Request::Pointer(Pointer {
                grant_id: "g1".into(),
                seq: 7,
                action: Action::Wheel,
                x: 0.5,
                y: 1.0,
                button: None,
                dx: -3.5,
                dy: 120.0
            }))
        );
        let down =
            r#"{"v":1,"kind":"ptr","grantId":"g1","seq":2,"action":"down","x":0,"y":0.25,"button":"right"}"#;
        assert!(matches!(parse(down), Some(Request::Pointer(Pointer { button: Some(Button::Right), .. }))));
        let key = r#"{"v":1,"kind":"key","grantId":"g1","seq":3,"code":"ShiftLeft","down":false}"#;
        assert_eq!(
            parse(key),
            Some(Request::Key(Key { grant_id: "g1".into(), seq: 3, code: "ShiftLeft".into(), down: false }))
        );
        assert_eq!(parse(r#"{"v":1,"kind":"release-all"}"#), Some(Request::ReleaseAll));
        assert_eq!(parse(r#"{"kind":"bye","v":1.0,"extra":[{}]}"#), Some(Request::Bye));
    }

    #[test]
    fn takes_the_display_in_system_pixels_when_the_console_gives_it() {
        let grant = r#"{"v":1,"kind":"grant","grantId":"g","expiresAt":1,"display":{"x":1280,"y":0,"width":1920,"height":1080,"scaleFactor":1,"physical":{"x":1920,"y":0,"width":1920,"height":1080}}}"#;
        let Some(Request::Grant(grant)) = parse(grant) else { panic!("a grant") };
        assert_eq!(grant.display.physical, Some(Rect { x: 1920.0, y: 0.0, width: 1920.0, height: 1080.0 }));
    }

    #[test]
    fn refuses_what_the_console_s_own_reader_refuses() {
        let refused = [
            r#"{"v":2,"kind":"bye"}"#,
            r#"{"v":"1","kind":"bye"}"#,
            r#"{"kind":"bye"}"#,
            r#"{"v":1,"kind":"wave"}"#,
            r#"{"v":1,"kind":"hello","versions":[1,"2"]}"#,
            r#"{"v":1,"kind":"hello"}"#,
            r#"{"v":1,"kind":"grant","grantId":"","expiresAt":1,"display":{"x":0,"y":0,"width":1,"height":1,"scaleFactor":1}}"#,
            r#"{"v":1,"kind":"grant","grantId":"g","expiresAt":1,"display":{"x":0,"y":0,"width":1,"height":1}}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":0,"action":"move","x":0,"y":0}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1.5,"action":"move","x":0,"y":0}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":9007199254740992,"action":"move","x":0,"y":0}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1,"action":"move","x":2,"y":0}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1,"action":"move","x":0,"y":-0.1}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1,"action":"drag","x":0,"y":0}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1,"action":"down","x":0,"y":0,"button":null}"#,
            r#"{"v":1,"kind":"ptr","grantId":"g","seq":1,"action":"down","x":0,"y":0,"button":"back"}"#,
            r#"{"v":1,"kind":"key","grantId":"g","seq":1,"code":"KeyA","down":1}"#,
            r#"{"v":1,"kind":"key","grantId":"g","seq":1,"code":"KeyA"}"#,
        ];
        for line in refused {
            assert_eq!(parse(line), None, "{line}");
        }
    }

    #[test]
    fn counts_a_grant_s_length_as_javascript_does() {
        let line = |id: &str| {
            format!(r#"{{"v":1,"kind":"key","grantId":"{id}","seq":1,"code":"KeyA","down":true}}"#)
        };
        assert!(parse(&line(&"g".repeat(128))).is_some());
        assert_eq!(parse(&line(&"g".repeat(129))), None);
        assert!(parse(&line(&"😀".repeat(64))).is_some());
        assert_eq!(parse(&line(&format!("{}g", "😀".repeat(64)))), None);
    }

    #[test]
    fn writes_replies_field_for_field_as_the_stand_in_does() {
        let lines: Vec<String> = [
            Reply::Hello { implementation: "kw-input-helper 0.1.0" },
            Reply::Granted { grant_id: "g\"1".into(), expires_at: 1_764_115_200_000.0 },
            Reply::Applied { seq: 3 },
            Reply::Refused { seq: Some(4), reason: Refusal::WrongGrant },
            Reply::Refused { seq: None, reason: Refusal::Malformed },
            Reply::Released { keys: vec!["KeyQ", "ShiftLeft"], buttons: vec![Button::Left, Button::Right] },
            Reply::Released { keys: vec![], buttons: vec![] },
            Reply::LocalInput,
            Reply::Bye,
        ]
        .iter()
        .map(Reply::line)
        .collect();
        assert_eq!(
            lines.concat(),
            [
                r#"{"v":1,"kind":"hello","version":1,"impl":"kw-input-helper 0.1.0"}"#,
                r#"{"v":1,"kind":"granted","grantId":"g\"1","expiresAt":1764115200000}"#,
                r#"{"v":1,"kind":"applied","seq":3}"#,
                r#"{"v":1,"kind":"refused","seq":4,"reason":"wrong-grant"}"#,
                r#"{"v":1,"kind":"refused","reason":"malformed"}"#,
                r#"{"v":1,"kind":"released","keys":["KeyQ","ShiftLeft"],"buttons":["left","right"]}"#,
                r#"{"v":1,"kind":"released","keys":[],"buttons":[]}"#,
                r#"{"v":1,"kind":"local-input"}"#,
                r#"{"v":1,"kind":"bye"}"#,
                "",
            ]
            .join("\n")
        );
    }
}
