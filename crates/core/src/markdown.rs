#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(String),
    Paragraph(String),
    Bullet(String),
    Check { done: bool, text: String },
    Row { cells: Vec<String>, head: bool },
    Gap,
}

pub fn blocks(source: &str) -> Vec<Block> {
    let lines: Vec<&str> = source.lines().collect();
    let mut read = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !matches!(read.last(), Some(Block::Gap) | None) {
                read.push(Block::Gap);
            }
        } else if let Some(text) = heading(trimmed) {
            read.push(Block::Heading(plain(text)));
        } else if trimmed.starts_with('|') {
            if !ruled(trimmed) {
                let head = lines.get(at + 1).is_some_and(|next| ruled(next.trim()));
                read.push(Block::Row { cells: cells(trimmed), head });
            }
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            read.push(listed(item));
        } else if let Some(Block::Paragraph(before)) = read.last_mut() {
            before.push(' ');
            before.push_str(&plain(trimmed));
        } else {
            read.push(Block::Paragraph(plain(trimmed)));
        }
    }
    while matches!(read.last(), Some(Block::Gap)) {
        read.pop();
    }
    read
}

fn heading(line: &str) -> Option<&str> {
    let marks = line.chars().take_while(|letter| *letter == '#').count();
    ((1..=6).contains(&marks) && line[marks..].starts_with(' ')).then(|| line[marks..].trim())
}

fn listed(item: &str) -> Block {
    for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
        if let Some(text) = item.strip_prefix(mark) {
            return Block::Check { done, text: plain(text) };
        }
    }
    Block::Bullet(plain(item))
}

fn ruled(line: &str) -> bool {
    line.starts_with('|') && line.contains('-') && line.chars().all(|letter| "|-: ".contains(letter))
}

fn cells(line: &str) -> Vec<String> {
    line.trim_matches('|').split('|').map(|cell| plain(cell.trim())).collect()
}

fn plain(text: &str) -> String {
    let unmarked: String = text.chars().filter(|letter| !matches!(letter, '*' | '`')).collect();
    unmarked.replace("__", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_reads_as_headings_lists_checks_and_rows() {
        let read = blocks(
            "## Ship the fix\n\n- [x] patch written\n- [ ] rollback rehearsed\n- plain item\n\n| step | days |\n|---|---|\n| patch | 1 |\n\nLow **risk**,\nhigh payoff.\n",
        );
        assert_eq!(
            read,
            vec![
                Block::Heading("Ship the fix".into()),
                Block::Gap,
                Block::Check { done: true, text: "patch written".into() },
                Block::Check { done: false, text: "rollback rehearsed".into() },
                Block::Bullet("plain item".into()),
                Block::Gap,
                Block::Row { cells: vec!["step".into(), "days".into()], head: true },
                Block::Row { cells: vec!["patch".into(), "1".into()], head: false },
                Block::Gap,
                Block::Paragraph("Low risk, high payoff.".into()),
            ]
        );
    }
}
