#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mood {
    Calm,
    #[default]
    Brisk,
    Eager,
    Playful,
    Urgent,
    Decisive,
    Tentative,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    InLine,
    Varied,
    AtOnce,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    pub glide: f64,
    pub gaps: f64,
    pub order: Order,
}

pub const GLIDE_MS: f64 = 200.0;
pub const GAP_MS: f64 = 34.0;

const WORDS: [(&str, Mood); 7] = [
    ("calm", Mood::Calm),
    ("brisk", Mood::Brisk),
    ("eager", Mood::Eager),
    ("playful", Mood::Playful),
    ("urgent", Mood::Urgent),
    ("decisive", Mood::Decisive),
    ("tentative", Mood::Tentative),
];

const NEAREST: [(&str, Mood); 16] = [
    ("gentle", Mood::Calm),
    ("quiet", Mood::Calm),
    ("soft", Mood::Calm),
    ("quick", Mood::Eager),
    ("excited", Mood::Eager),
    ("curious", Mood::Eager),
    ("festive", Mood::Playful),
    ("happy", Mood::Playful),
    ("proud", Mood::Playful),
    ("critical", Mood::Urgent),
    ("decided", Mood::Decisive),
    ("resolved", Mood::Decisive),
    ("confident", Mood::Decisive),
    ("hesitant", Mood::Tentative),
    ("unsure", Mood::Tentative),
    ("sorry", Mood::Tentative),
];

impl Mood {
    pub fn read(said: &str) -> Option<Mood> {
        let word = said.trim().to_lowercase();
        WORDS.iter().chain(NEAREST.iter()).find(|(known, _)| *known == word).map(|(_, mood)| *mood)
    }

    pub fn word(self) -> &'static str {
        WORDS.iter().find(|(_, mood)| *mood == self).map_or("brisk", |(word, _)| word)
    }

    pub fn pace(self) -> Pace {
        match self {
            Mood::Calm => Pace { glide: 1.75, gaps: 1.6, order: Order::InLine },
            Mood::Brisk => Pace { glide: 1.0, gaps: 1.0, order: Order::InLine },
            Mood::Eager => Pace { glide: 0.7, gaps: 0.5, order: Order::InLine },
            Mood::Playful => Pace { glide: 1.0, gaps: 1.0, order: Order::Varied },
            Mood::Urgent => Pace { glide: 0.8, gaps: 0.0, order: Order::AtOnce },
            Mood::Decisive => Pace { glide: 0.9, gaps: 0.0, order: Order::AtOnce },
            Mood::Tentative => Pace { glide: 1.2, gaps: 1.8, order: Order::InLine },
        }
    }

    pub fn glide_ms(self) -> f64 {
        GLIDE_MS * self.pace().glide
    }

    pub fn starts_ms(self, count: usize) -> Vec<f64> {
        let pace = self.pace();
        let in_line = |turn: usize| turn as f64 * GAP_MS * pace.gaps;
        match pace.order {
            Order::AtOnce => vec![0.0; count],
            Order::InLine => (0..count).map(in_line).collect(),
            Order::Varied => {
                let middle = count.saturating_sub(1) / 2;
                let mut starts = vec![0.0; count];
                for turn in 0..count {
                    let step = turn.div_ceil(2);
                    let place = if turn % 2 == 1 { middle + step } else { middle.saturating_sub(step) };
                    starts[place.min(count - 1)] = in_line(turn);
                }
                starts
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mood_is_read_by_its_word_or_the_nearest_one() {
        assert_eq!(Mood::read("Playful"), Some(Mood::Playful));
        assert_eq!(Mood::read("resolved"), Some(Mood::Decisive));
        assert_eq!(Mood::read("angry"), None);
        assert_eq!(Mood::Tentative.word(), "tentative");
    }

    #[test]
    fn sixteen_panes_never_take_long_whatever_the_mood() {
        for (_, mood) in WORDS {
            let last = mood.starts_ms(16).into_iter().fold(0.0, f64::max);
            assert!(last + mood.glide_ms() <= 1_250.0, "{mood:?} takes {}", last + mood.glide_ms());
        }
    }

    #[test]
    fn urgent_moves_everything_at_once_and_playful_starts_from_the_middle() {
        assert_eq!(Mood::Urgent.starts_ms(3), vec![0.0, 0.0, 0.0]);
        let playful = Mood::Playful.starts_ms(5);
        assert_eq!(playful[2], 0.0);
        assert!(playful[0] > 0.0 && playful[4] > 0.0);
    }
}
