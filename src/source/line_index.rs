//! Line and column indexing compatible with the existing LSL lexer.
//!
//! The lexer treats LF, lone CR, and CRLF as line breaks. CRLF is one logical
//! newline that consumes both bytes. Columns count Unicode scalar values and are
//! one-based; byte offsets remain zero-based UTF-8 offsets.

use std::fmt;
use std::sync::Arc;

/// A resolved line/column position inside one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    /// Zero-based UTF-8 byte offset.
    pub offset: usize,
    /// One-based line.
    pub line: usize,
    /// One-based Unicode scalar column.
    pub column: usize,
}

/// Failure to resolve a line/column position or byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineIndexError {
    /// The byte offset is past the end of the source.
    OutOfBounds,
    /// The byte offset splits a UTF-8 scalar value.
    NotCharBoundary,
    /// The requested line is not present.
    LineOutOfBounds,
    /// The requested column is not present on the line.
    ColumnOutOfBounds,
}

impl fmt::Display for LineIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::OutOfBounds => "byte offset is outside the source",
            Self::NotCharBoundary => "byte offset is not on a UTF-8 boundary",
            Self::LineOutOfBounds => "line is outside the source",
            Self::ColumnOutOfBounds => "column is outside the line",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LineIndexError {}

/// Per-file line index.
///
/// Construction is linear in the source size. Lookups use binary search over
/// line starts, so resolving many diagnostics does not rescan the whole file.
#[derive(Debug)]
pub struct LineIndex {
    text: Arc<str>,
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build a line index for one immutable source text.
    pub fn new(text: Arc<str>) -> Self {
        let mut line_starts = Vec::with_capacity(text.len() / 32 + 1);
        line_starts.push(0);
        let bytes = text.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            match bytes[offset] {
                b'\r' => {
                    if offset + 1 < bytes.len() && bytes[offset + 1] == b'\n' {
                        offset += 2;
                    } else {
                        offset += 1;
                    }
                    line_starts.push(offset);
                }
                b'\n' => {
                    offset += 1;
                    line_starts.push(offset);
                }
                _ => {
                    // Advance one UTF-8 scalar. Non-newline bytes are only
                    // inspected for ASCII CR/LF above.
                    let remainder = &text[offset..];
                    offset += remainder
                        .chars()
                        .next()
                        .map_or(1, |character| character.len_utf8());
                }
            }
        }
        Self { text, line_starts }
    }

    /// Source text this index was built for.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of lines. An empty source has one line; a trailing newline opens
    /// one additional empty line.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Resolve a zero-based UTF-8 byte offset to a one-based line/column.
    pub fn location(&self, offset: usize) -> Result<SourcePosition, LineIndexError> {
        if offset > self.text.len() {
            return Err(LineIndexError::OutOfBounds);
        }
        if !self.text.is_char_boundary(offset) {
            return Err(LineIndexError::NotCharBoundary);
        }
        let line_index = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line_index];
        let column = self.text[line_start..offset].chars().count() + 1;
        Ok(SourcePosition {
            offset,
            line: line_index + 1,
            column,
        })
    }

    /// Resolve a one-based line/column to a zero-based UTF-8 byte offset.
    pub fn offset(&self, line: usize, column: usize) -> Result<usize, LineIndexError> {
        if line == 0 || line > self.line_starts.len() {
            return Err(LineIndexError::LineOutOfBounds);
        }
        if column == 0 {
            return Err(LineIndexError::ColumnOutOfBounds);
        }
        let start = self.line_starts[line - 1];
        let mut end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        if end > start && self.text.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        if end > start && self.text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }

        let mut current_column = 1;
        for (relative, _) in self.text[start..end].char_indices() {
            if current_column == column {
                return Ok(start + relative);
            }
            current_column += 1;
        }
        if current_column == column {
            Ok(end)
        } else {
            Err(LineIndexError::ColumnOutOfBounds)
        }
    }

    /// Position of the end of the source.
    pub fn eof_position(&self) -> SourcePosition {
        self.location(self.text.len()).unwrap_or(SourcePosition {
            offset: self.text.len(),
            line: self.line_starts.len(),
            column: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex;

    fn index(text: &str) -> LineIndex {
        LineIndex::new(Arc::from(text))
    }

    fn assert_matches_lexer(text: &str) {
        let line_index = index(text);
        let (tokens, diagnostics) = lex(text);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for token in tokens {
            for offset in [token.span.start, token.span.end] {
                let position = line_index.location(offset).unwrap();
                let expected_line = if offset == token.span.end {
                    token.span.end_line
                } else {
                    token.span.line
                };
                let expected_column = if offset == token.span.end {
                    token.span.end_column
                } else {
                    token.span.column
                };
                assert_eq!(
                    (position.line, position.column),
                    (expected_line, expected_column),
                    "offset {offset} in {text:?}"
                );
            }
        }
    }

    #[test]
    fn matches_lexer_for_ascii_and_newline_forms() {
        assert_matches_lexer("");
        assert_matches_lexer("integer a;\nfloat b;\n");
        assert_matches_lexer("integer a;\rfloat b;\r");
        assert_matches_lexer("integer a;\r\nfloat b;\r\n");
        assert_matches_lexer("default {\r\n  state_entry() { }\r\n}\r\n");
    }

    #[test]
    fn matches_lexer_for_multibyte_text() {
        assert_matches_lexer("// héllo\nstring s = \"héllo\";\n");
        assert_matches_lexer("string s = \"日本語\";\n");
    }

    #[test]
    fn empty_source_has_one_line_and_eof_at_one_one() {
        let line_index = index("");
        assert_eq!(line_index.line_count(), 1);
        assert_eq!(
            line_index.location(0).unwrap(),
            SourcePosition {
                offset: 0,
                line: 1,
                column: 1
            }
        );
        assert_eq!(line_index.eof_position().offset, 0);
    }

    #[test]
    fn trailing_newline_opens_an_empty_line() {
        let line_index = index("a\n");
        assert_eq!(line_index.line_count(), 2);
        assert_eq!(line_index.location(2).unwrap().line, 2);
        assert_eq!(line_index.location(2).unwrap().column, 1);
    }

    #[test]
    fn columns_count_scalar_values_not_bytes() {
        let line_index = index("éé");
        assert_eq!(line_index.location(0).unwrap().column, 1);
        assert_eq!(line_index.location(2).unwrap().column, 2);
        assert_eq!(line_index.location(4).unwrap().column, 3);
        assert_eq!(line_index.offset(1, 3).unwrap(), 4);
    }

    #[test]
    fn rejects_offsets_inside_utf8_scalars() {
        let line_index = index("é");
        assert_eq!(line_index.location(1), Err(LineIndexError::NotCharBoundary));
        assert_eq!(line_index.location(3), Err(LineIndexError::OutOfBounds));
    }

    #[test]
    fn every_char_boundary_resolves() {
        let text = "aé\r\nb";
        let line_index = index(text);
        for offset in 0..=text.len() {
            if text.is_char_boundary(offset) {
                assert!(line_index.location(offset).is_ok(), "offset {offset}");
            } else {
                assert_eq!(
                    line_index.location(offset),
                    Err(LineIndexError::NotCharBoundary),
                    "offset {offset}"
                );
            }
        }
    }

    #[test]
    fn line_column_lookup_round_trips() {
        let text = "one\ntwo\r\nthree\rfour";
        let line_index = index(text);
        for offset in 0..=text.len() {
            if !text.is_char_boundary(offset) {
                continue;
            }
            // Skip the byte between CR and LF: it is a boundary but not a
            // position the lexer ever produces.
            if offset > 0
                && offset < text.len()
                && text.as_bytes()[offset - 1] == b'\r'
                && text.as_bytes()[offset] == b'\n'
            {
                continue;
            }
            let position = line_index.location(offset).unwrap();
            assert_eq!(
                line_index.offset(position.line, position.column).unwrap(),
                offset
            );
        }
    }
}
