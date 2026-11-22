use std::io::BufRead;

use crate::protocol::Reason;

pub struct Lines<Input> {
    input: Input,
    limit: usize,
}

impl<Input: BufRead> Lines<Input> {
    pub fn new(input: Input, limit: usize) -> Self {
        Lines { input, limit }
    }
}

impl<Input: BufRead> Iterator for Lines<Input> {
    type Item = Result<String, Reason>;

    fn next(&mut self) -> Option<Self::Item> {
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
                return Some(text(line, fits));
            }
        }
    }
}

fn text(line: Vec<u8>, fits: bool) -> Result<String, Reason> {
    if !fits {
        return Err(Reason::LineTooLong);
    }
    String::from_utf8(line).map_err(|_| Reason::NotText)
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    fn lines(input: &[u8], limit: usize) -> Vec<Result<String, Reason>> {
        Lines::new(BufReader::with_capacity(3, input), limit).collect()
    }

    #[test]
    fn yields_each_finished_line_without_its_newline() {
        assert_eq!(
            lines(b"stats\nrelease 1f\n", 64),
            vec![Ok("stats".to_owned()), Ok("release 1f".to_owned())]
        );
    }

    #[test]
    fn drops_a_line_cut_short_by_the_end_of_input() {
        assert_eq!(lines(b"stats\nglide 1 10", 64), vec![Ok("stats".to_owned())]);
        assert!(lines(b"", 64).is_empty());
    }

    #[test]
    fn refuses_an_overlong_line_and_carries_on_with_the_next() {
        assert_eq!(lines(b"0123456789\nstats\n", 8), vec![Err(Reason::LineTooLong), Ok("stats".to_owned())]);
        assert_eq!(lines(b"01234567\n", 8), vec![Ok("01234567".to_owned())]);
    }

    #[test]
    fn refuses_bytes_that_are_not_text() {
        assert_eq!(lines(b"\xff\xfe\nstats\n", 64), vec![Err(Reason::NotText), Ok("stats".to_owned())]);
    }
}
