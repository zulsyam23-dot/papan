use serde::{Deserialize, Serialize};

/// Byte range in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    pub fn empty(at: usize) -> Self {
        Self { start: at, end: at }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns `(1-based line, 1-based column, byte offset of line start)`.
    pub fn line_col(self, text: &str) -> (usize, usize, usize) {
        let offset = self.start.min(text.len());
        let mut line = 1usize;
        let mut line_start = 0usize;
        for (idx, ch) in text.char_indices() {
            if idx >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                line_start = idx + ch.len_utf8();
            }
        }
        (line, offset.saturating_sub(line_start) + 1, line_start)
    }

    /// The full source line containing the span, without the line ending.
    pub fn source_line<'a>(self, text: &'a str) -> &'a str {
        let (_, _, line_start) = self.line_col(text);
        let rest = text.get(line_start..).unwrap_or("");
        match rest.find('\n') {
            Some(end) => &rest[..end],
            None => rest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_line_and_column() {
        let text = "first\nsecond\nthird";
        let span = Span::new(8, 12);
        assert_eq!(span.line_col(text), (2, 3, 6));
    }

    #[test]
    fn finds_source_line() {
        let text = "one\ntwo\nthree";
        assert_eq!(Span::new(5, 7).source_line(text), "two");
    }
}
