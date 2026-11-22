pub const SPAN: i32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seat {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub const NAMED: [(&str, Seat); 22] = [
    ("full", seat(0, 0, 12, 12)),
    ("left-half", seat(0, 0, 6, 12)),
    ("right-half", seat(6, 0, 6, 12)),
    ("top-half", seat(0, 0, 12, 6)),
    ("bottom-half", seat(0, 6, 12, 6)),
    ("left-third", seat(0, 0, 4, 12)),
    ("middle-third", seat(4, 0, 4, 12)),
    ("right-third", seat(8, 0, 4, 12)),
    ("left-two-thirds", seat(0, 0, 8, 12)),
    ("right-two-thirds", seat(4, 0, 8, 12)),
    ("top-left", seat(0, 0, 6, 6)),
    ("top-right", seat(6, 0, 6, 6)),
    ("bottom-left", seat(0, 6, 6, 6)),
    ("bottom-right", seat(6, 6, 6, 6)),
    ("center", seat(3, 3, 6, 6)),
    ("top-third", seat(0, 0, 12, 4)),
    ("bottom-third", seat(0, 8, 12, 4)),
    ("left-quarter", seat(0, 0, 3, 12)),
    ("right-quarter", seat(9, 0, 3, 12)),
    ("left-three-quarters", seat(0, 0, 9, 12)),
    ("right-three-quarters", seat(3, 0, 9, 12)),
    ("middle-half", seat(3, 0, 6, 12)),
];

const ALIASES: [(&str, &str); 10] = [
    ("left", "left-half"),
    ("right", "right-half"),
    ("top", "top-half"),
    ("bottom", "bottom-half"),
    ("middle", "center"),
    ("top left quarter", "top-left"),
    ("top right quarter", "top-right"),
    ("bottom left quarter", "bottom-left"),
    ("bottom right quarter", "bottom-right"),
    ("middle column", "middle-half"),
];

const SYNONYMS: [(&str, &str); 17] = [
    ("upper", "top"),
    ("lower", "bottom"),
    ("center", "middle"),
    ("centre", "middle"),
    ("central", "middle"),
    ("whole", "full"),
    ("entire", "full"),
    ("everything", "full"),
    ("fullscreen", "full"),
    ("maximized", "full"),
    ("maximised", "full"),
    ("thirds", "third"),
    ("quarters", "quarter"),
    ("2/3", "two third"),
    ("1/3", "third"),
    ("1/2", "half"),
    ("3/4", "three quarter"),
];

const FILLER: [&str; 12] =
    ["the", "a", "of", "on", "in", "at", "side", "hand", "corner", "screen", "stage", "one"];

const fn seat(x: i32, y: i32, w: i32, h: i32) -> Seat {
    Seat { x, y, w, h }
}

impl Seat {
    pub fn named(name: &str) -> Option<Seat> {
        NAMED.iter().find(|(known, _)| *known == name).map(|(_, seat)| *seat)
    }

    pub fn name(self) -> Option<&'static str> {
        NAMED.iter().find(|(_, seat)| *seat == self).map(|(name, _)| *name)
    }

    pub fn read(said: &str) -> Result<Seat, String> {
        let key = key_of(said);
        let by_name = NAMED.iter().find(|(name, _)| key_of(name) == key).map(|(_, seat)| *seat);
        let by_alias =
            ALIASES.iter().find(|(alias, _)| key_of(alias) == key).and_then(|(_, name)| Seat::named(name));
        if let Some(seat) = by_name.or(by_alias) {
            return Ok(seat);
        }
        match on_lines(said) {
            Some(seat) => seat.fitted(),
            None => Err(format!(
                "no seat is called {said:?}; name one such as left-half or center, or give lattice lines such as \"columns 4 to 8, rows 0 to 6\""
            )),
        }
    }

    pub fn fitted(self) -> Result<Seat, String> {
        let Seat { x, y, w, h } = self;
        if x < 0 || y < 0 {
            return Err("x and y start at 0, the left and top edge".into());
        }
        if w < 1 || h < 1 {
            return Err("a seat is at least 1 column wide and 1 row tall".into());
        }
        if x + w > SPAN {
            return Err(format!("x {x} + w {w} is {}, past the stage's {SPAN} columns", x + w));
        }
        if y + h > SPAN {
            return Err(format!("y {y} + h {h} is {}, past the stage's {SPAN} rows", y + h));
        }
        Ok(self)
    }

    pub fn words(self) -> String {
        match self.name() {
            Some("full") => "the whole stage".into(),
            Some(name) => format!("the {}", name.replace('-', " ")),
            None => {
                format!("columns {} to {}, rows {} to {}", self.x, self.x + self.w, self.y, self.y + self.h)
            }
        }
    }

    pub fn on(self, room: Rect, gap: i32) -> Rect {
        let across = |at: i32| room.x + (i64::from(room.w) * i64::from(at) / i64::from(SPAN)) as i32;
        let down = |at: i32| room.y + (i64::from(room.h) * i64::from(at) / i64::from(SPAN)) as i32;
        let (left, right) = (across(self.x), across(self.x + self.w));
        let (top, bottom) = (down(self.y), down(self.y + self.h));
        let half = gap / 2;
        Rect { x: left + half, y: top + half, w: right - left - gap, h: bottom - top - gap }
    }

    pub fn overlaps(self, other: Seat) -> bool {
        let wide = (self.x + self.w).min(other.x + other.w) - self.x.max(other.x);
        let tall = (self.y + self.h).min(other.y + other.h) - self.y.max(other.y);
        wide > 0 && tall > 0
    }
}

fn tokens(said: &str) -> Vec<String> {
    said.to_lowercase()
        .split(|letter: char| !(letter.is_ascii_alphanumeric() || letter == '/'))
        .flat_map(|token| {
            let known = SYNONYMS.iter().find(|(from, _)| *from == token).map_or(token, |(_, to)| *to);
            known.split(' ').map(str::to_owned).collect::<Vec<_>>()
        })
        .filter(|token| !token.is_empty() && !FILLER.contains(&token.as_str()))
        .collect()
}

fn key_of(said: &str) -> String {
    let mut words = tokens(said);
    words.sort();
    words.join(" ")
}

fn on_lines(said: &str) -> Option<Seat> {
    let lowered = said.to_lowercase();
    let columns = lines_named(&lowered, "col");
    let rows = lines_named(&lowered, "row");
    if columns.is_none() && rows.is_none() {
        return None;
    }
    let (x, right) = columns.unwrap_or((0, SPAN));
    let (y, bottom) = rows.unwrap_or((0, SPAN));
    Some(Seat { x, y, w: right - x, h: bottom - y })
}

fn lines_named(said: &str, axis: &str) -> Option<(i32, i32)> {
    let after = &said[said.find(axis)? + axis.len()..];
    let mut numbers = after
        .split(|letter: char| !letter.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .filter_map(|run| run.parse::<i32>().ok());
    Some((numbers.next()?, numbers.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_seat_fits_the_lattice() {
        for (name, seat) in NAMED {
            assert_eq!(seat.fitted(), Ok(seat), "{name}");
        }
    }

    #[test]
    fn a_seat_is_read_in_the_words_a_model_reaches_for() {
        let read = |said: &str| Seat::read(said).unwrap();
        assert_eq!(read("left-half"), Seat::named("left-half").unwrap());
        assert_eq!(read("Upper Right"), Seat::named("top-right").unwrap());
        assert_eq!(read("the left 3/4"), Seat::named("left-three-quarters").unwrap());
        assert_eq!(read("right quarter"), Seat::named("right-quarter").unwrap());
        assert_eq!(read("centre"), Seat::named("center").unwrap());
        assert_eq!(read("top-left quarter"), Seat::named("top-left").unwrap());
        assert_eq!(read("columns 4 to 8, rows 0 to 6"), Seat { x: 4, y: 0, w: 4, h: 6 });
        assert_eq!(read("cols 0-6"), Seat::named("left-half").unwrap());
    }

    #[test]
    fn a_seat_that_cannot_be_placed_says_what_would_work() {
        assert!(Seat::read("the attic").unwrap_err().starts_with("no seat is called \"the attic\""));
        assert_eq!(
            Seat::read("columns 8 to 14").unwrap_err(),
            "x 8 + w 6 is 14, past the stage's 12 columns"
        );
    }

    #[test]
    fn seats_tile_a_room_with_one_gap_between_neighbours() {
        let room = Rect { x: 100, y: 0, w: 1200, h: 600 };
        let left = Seat::named("left-half").unwrap().on(room, 12);
        let right = Seat::named("right-half").unwrap().on(room, 12);
        assert_eq!(left, Rect { x: 106, y: 6, w: 588, h: 588 });
        assert_eq!(right.x - (left.x + left.w), 12);
        assert_eq!(Seat::named("full").unwrap().words(), "the whole stage");
        assert_eq!(Seat { x: 0, y: 0, w: 4, h: 6 }.words(), "columns 0 to 4, rows 0 to 6");
    }
}
