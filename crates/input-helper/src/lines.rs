use std::io::BufRead;

/// The console's lines, read as its own `lineReader` reads them: split at each newline, trimmed,
/// blank ones skipped, an unfinished last line dropped. A line too long or not UTF-8 comes back as
/// `None` so it can be refused, and reading carries on with the next.
pub struct Lines<Input> {
    input: Input,
    limit: usize,
}

impl<Input: BufRead> Lines<Input> {
    pub fn new(input: Input, limit: usize) -> Self {
        Lines { input, limit }
    }

    fn next_raw(&mut self) -> Option<Option<String>> {
        let mut line = Vec::new();
        let mut fits = true;
        loop {
            let chunk = self.input.fill_buf().ok().filter(|chunk| !chunk.is_empty())?;
            let end = chunk.iter().position(|byte| *byte == b'\n');
            let taken = end.unwrap_or(chunk.len());
            fits = fits && line.len() + taken <= self.limit;
            if fits {
                line.extend_from_slice(&chunk[..taken]);
            }
            self.input.consume(taken + usize::from(end.is_some()));
            if end.is_some() {
                return Some(fits.then(|| String::from_utf8(line).ok()).flatten());
            }
        }
    }
}

impl<Input: BufRead> Iterator for Lines<Input> {
    type Item = Option<String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.next_raw()? {
                Some(text) if text.trim().is_empty() => continue,
                Some(text) => return Some(Some(text.trim().to_owned())),
                None => return Some(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    fn lines(input: &[u8], limit: usize) -> Vec<Option<String>> {
        Lines::new(BufReader::with_capacity(3, input), limit).collect()
    }

    #[test]
    fn reads_trimmed_lines_skipping_blank_ones_and_an_unfinished_last_one() {
        let read = lines(b" {\"a\":1}\r\n\n  \n{}\n{\"cut", 64);
        assert_eq!(read, [Some("{\"a\":1}".to_owned()), Some("{}".to_owned())]);
    }

    #[test]
    fn refuses_an_overlong_line_or_one_that_is_not_text_and_carries_on() {
        let read = lines(b"0123456789\n\xff\xfe\n01234567\n", 8);
        assert_eq!(read, [None, None, Some("01234567".to_owned())]);
    }
}
