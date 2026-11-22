#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const BACKGROUND: Rgb = Rgb(0x1a, 0x1b, 0x26);
pub const HEADER: Rgb = Rgb(0x1f, 0x20, 0x30);
pub const TEXT: Rgb = Rgb(0xa9, 0xb1, 0xd6);
pub const BRIGHT: Rgb = Rgb(0xc0, 0xca, 0xf5);
pub const DIM: Rgb = Rgb(0x56, 0x5f, 0x89);
pub const RULE: Rgb = Rgb(0x2a, 0x2c, 0x41);
pub const HUE: Rgb = Rgb(0x7a, 0xa2, 0xf7);
pub const BACKDROP: Rgb = Rgb(0x0f, 0x10, 0x18);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentColor {
    Cyan,
    Magenta,
    Green,
    Yellow,
    Blue,
}

impl AgentColor {
    pub const ALL: [AgentColor; 5] =
        [AgentColor::Cyan, AgentColor::Magenta, AgentColor::Green, AgentColor::Yellow, AgentColor::Blue];

    pub fn rgb(self) -> Rgb {
        match self {
            AgentColor::Cyan => Rgb(0x7d, 0xcf, 0xff),
            AgentColor::Magenta => Rgb(0xbb, 0x9a, 0xf7),
            AgentColor::Green => Rgb(0x9e, 0xce, 0x6a),
            AgentColor::Yellow => Rgb(0xe0, 0xaf, 0x68),
            AgentColor::Blue => Rgb(0x7a, 0xa2, 0xf7),
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            AgentColor::Cyan => "cyan",
            AgentColor::Magenta => "magenta",
            AgentColor::Green => "green",
            AgentColor::Yellow => "yellow",
            AgentColor::Blue => "blue",
        }
    }

    pub fn kept_for(name: &str, taken: &[AgentColor]) -> AgentColor {
        let mut hash: u32 = 2_166_136_261;
        for letter in name.trim().to_lowercase().chars() {
            hash = (hash ^ letter as u32).wrapping_mul(16_777_619);
        }
        let first = hash as usize % Self::ALL.len();
        let in_turn = (0..Self::ALL.len()).map(|step| Self::ALL[(first + step) % Self::ALL.len()]);
        in_turn
            .clone()
            .find(|color| !taken.contains(color))
            .or_else(|| in_turn.clone().next())
            .unwrap_or(AgentColor::Cyan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_agent_gets_a_colour_of_its_own_while_one_is_free() {
        let mut taken = Vec::new();
        for name in ["claude-code", "cursor", "codex", "goose", "aider"] {
            let color = AgentColor::kept_for(name, &taken);
            assert!(!taken.contains(&color), "{name}");
            taken.push(color);
        }
        assert_eq!(AgentColor::kept_for("Claude-Code", &[]), AgentColor::kept_for("claude-code", &[]));
    }
}
