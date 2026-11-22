//! Internal preprocessor text primitives and tokenization.
//!
//! This layer is independent of the LSL lexer: it preserves original byte
//! offsets, understands comments and strings, and supports logical directive
//! lines joined by backslash-newline.

use crate::source::file::SourceId;

/// A range in an original source file, before map validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RawSpan {
    pub(crate) source: SourceId,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl RawSpan {
    pub(crate) fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    pub(crate) fn point(source: SourceId, offset: usize) -> Self {
        Self::new(source, offset, offset)
    }
}

/// Coarse token category used by macro expansion and separator insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PieceKind {
    Whitespace,
    Comment,
    String,
    Identifier,
    Number,
    Punct,
    Other,
}

/// One exact text fragment with its original span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) text: String,
    pub(crate) kind: PieceKind,
    pub(crate) span: RawSpan,
}

impl Piece {
    pub(crate) fn new(text: String, kind: PieceKind, span: RawSpan) -> Self {
        Self { text, kind, span }
    }
}

#[derive(Debug, Clone)]
struct MappedChunk {
    out_start: usize,
    out_end: usize,
    orig_start: usize,
}

/// Logical directive text with byte mapping back to the original source.
#[derive(Debug, Clone)]
pub(crate) struct MappedText {
    source: SourceId,
    text: String,
    chunks: Vec<MappedChunk>,
    fallback_start: usize,
}

impl MappedText {
    /// Build logical text from `start` until the physical line terminator,
    /// applying backslash-newline splicing. Returns the text and the offset just
    /// after the consumed terminator (or EOF).
    pub(crate) fn logical_line(source_text: &str, source: SourceId, start: usize) -> (Self, usize) {
        let mut mapped = Self {
            source,
            text: String::new(),
            chunks: Vec::new(),
            fallback_start: start,
        };
        let mut run_start = start;
        let mut index = start;
        let mut in_block_comment = false;
        let mut in_line_comment = false;
        let mut in_string = false;
        while index < source_text.len() {
            if source_text[index..].starts_with('\\') {
                let after_backslash = index + 1;
                let newline_len = newline_len_at(source_text, after_backslash);
                if newline_len > 0 {
                    mapped.push_run(source_text, run_start, index);
                    index = after_backslash + newline_len;
                    run_start = index;
                    continue;
                }
            }
            let ch = source_text[index..].chars().next().unwrap_or('\0');
            if ch == '\n' || ch == '\r' {
                if !in_block_comment {
                    mapped.push_run(source_text, run_start, index);
                    let len = if source_text[index..].starts_with("\r\n") {
                        2
                    } else {
                        1
                    };
                    return (mapped, index + len);
                }
                index += ch.len_utf8();
                continue;
            }
            if in_block_comment {
                if source_text[index..].starts_with("*/") {
                    in_block_comment = false;
                    index += 2;
                } else {
                    index += ch.len_utf8();
                }
                continue;
            }
            if in_line_comment {
                index += ch.len_utf8();
                continue;
            }
            if in_string {
                if ch == '\\' {
                    index += 1;
                    if index < source_text.len() {
                        let escaped = source_text[index..].chars().next().unwrap_or('\0');
                        index += escaped.len_utf8();
                    }
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                index += ch.len_utf8();
                continue;
            }
            if source_text[index..].starts_with("//") {
                in_line_comment = true;
                index += 2;
                continue;
            }
            if source_text[index..].starts_with("/*") {
                in_block_comment = true;
                index += 2;
                continue;
            }
            if ch == '"' {
                in_string = true;
            }
            index += ch.len_utf8();
        }
        mapped.push_run(source_text, run_start, source_text.len());
        (mapped, source_text.len())
    }

    /// Build a range with all backslash-newline splices removed. This is used
    /// for ordinary active code: line splicing happens before tokenization.
    pub(crate) fn spliced_range(
        source_text: &str,
        source: SourceId,
        start: usize,
        end: usize,
    ) -> Self {
        let mut mapped = Self {
            source,
            text: String::new(),
            chunks: Vec::new(),
            fallback_start: start,
        };
        let mut run_start = start;
        let mut index = start;
        while index < end {
            if source_text[index..].starts_with('\\') {
                let after_backslash = index + 1;
                let newline_len = newline_len_at(source_text, after_backslash);
                if newline_len > 0 && after_backslash + newline_len <= end {
                    mapped.push_run(source_text, run_start, index);
                    index = after_backslash + newline_len;
                    run_start = index;
                    continue;
                }
            }
            let ch = source_text[index..].chars().next().unwrap_or('\0');
            index += ch.len_utf8();
        }
        mapped.push_run(source_text, run_start, end);
        mapped
    }

    fn push_run(&mut self, source_text: &str, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let out_start = self.text.len();
        self.text.push_str(&source_text[start..end]);
        self.chunks.push(MappedChunk {
            out_start,
            out_end: self.text.len(),
            orig_start: start,
        });
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Map a logical-text range back to an original source span.
    pub(crate) fn raw_span(&self, start: usize, end: usize) -> RawSpan {
        if self.chunks.is_empty() || (start == 0 && end == 0) {
            return RawSpan::point(self.source, self.fallback_start);
        }
        let start = start.min(self.text.len());
        let end = end.min(self.text.len()).max(start);
        let first = self.chunks.partition_point(|chunk| chunk.out_end <= start);
        let first = first.min(self.chunks.len() - 1);
        let first_chunk = &self.chunks[first];
        let start_orig = first_chunk.orig_start + start.saturating_sub(first_chunk.out_start);

        if end == start {
            return RawSpan::point(self.source, start_orig);
        }
        let last_index = self
            .chunks
            .partition_point(|chunk| chunk.out_end < end)
            .min(self.chunks.len() - 1);
        let last_chunk = &self.chunks[last_index];
        let end_orig = last_chunk.orig_start + end.saturating_sub(last_chunk.out_start);
        RawSpan::new(self.source, start_orig, end_orig)
    }

    /// Return a sub-slice of this mapped text with adjusted chunk offsets.
    pub(crate) fn slice(&self, start: usize, end: usize) -> Self {
        let start = start.min(self.text.len());
        let end = end.min(self.text.len()).max(start);
        let text = self.text[start..end].to_owned();
        let mut chunks = Vec::new();
        for chunk in &self.chunks {
            let out_start = chunk.out_start.max(start);
            let out_end = chunk.out_end.min(end);
            if out_end <= out_start {
                continue;
            }
            chunks.push(MappedChunk {
                out_start: out_start - start,
                out_end: out_end - start,
                orig_start: chunk.orig_start + (out_start - chunk.out_start),
            });
        }
        let fallback_start = self.raw_span(start, start).start;
        Self {
            source: self.source,
            text,
            chunks,
            fallback_start,
        }
    }

    /// Trim ASCII whitespace from both ends, returning a sub-slice and the
    /// corresponding original span.
    pub(crate) fn trimmed(&self) -> (&str, RawSpan) {
        let trimmed = self.text.trim_matches(|ch: char| ch.is_ascii_whitespace());
        if trimmed.is_empty() {
            return ("", RawSpan::point(self.source, self.fallback_start));
        }
        let start = self.text.find(trimmed).unwrap_or(0);
        let end = start + trimmed.len();
        (trimmed, self.raw_span(start, end))
    }
}

fn newline_len_at(text: &str, index: usize) -> usize {
    if text[index..].starts_with('\n') {
        1
    } else if text[index..].starts_with("\r\n") {
        2
    } else if text[index..].starts_with('\r') {
        1
    } else {
        0
    }
}

/// Local provenance-node identifier used during preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ProvId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkOrigin {
    Direct,
    Node(ProvId),
}

/// One output fragment during preprocessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkItem {
    pub(crate) text: String,
    pub(crate) kind: PieceKind,
    pub(crate) span: RawSpan,
    pub(crate) origin: WorkOrigin,
}

impl WorkItem {
    pub(crate) fn from_piece(piece: Piece) -> Self {
        Self {
            text: piece.text,
            kind: piece.kind,
            span: piece.span,
            origin: WorkOrigin::Direct,
        }
    }
}

/// Tokenize generated text and assign every piece the same origin span.
pub(crate) fn tokenize_generated(text: &str, span: RawSpan) -> Vec<Piece> {
    tokenize_with(text, 0, text.len(), |_, _| span)
}

/// Tokenize original source text in `[start, end)`. Test helper.
#[cfg(test)]
pub(crate) fn tokenize_source(
    source_text: &str,
    source: SourceId,
    start: usize,
    end: usize,
) -> Vec<Piece> {
    tokenize_with(source_text, start, end, |from, to| {
        RawSpan::new(source, from, to)
    })
}

/// Tokenize logical directive text.
pub(crate) fn tokenize_mapped(mapped: &MappedText) -> Vec<Piece> {
    tokenize_with(mapped.text(), 0, mapped.text().len(), |from, to| {
        mapped.raw_span(from, to)
    })
}

fn tokenize_with<F>(text: &str, start: usize, end: usize, span: F) -> Vec<Piece>
where
    F: Fn(usize, usize) -> RawSpan,
{
    let mut pieces = Vec::new();
    let mut index = start;
    while index < end {
        let ch = text[index..].chars().next().unwrap_or('\0');
        let begin = index;
        if ch.is_whitespace() {
            index += ch.len_utf8();
            while index < end {
                let next = text[index..].chars().next().unwrap_or('\0');
                if !next.is_whitespace() {
                    break;
                }
                index += next.len_utf8();
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Whitespace,
                span(begin, index),
            ));
            continue;
        }
        if ch == '/' && text[index..].starts_with("//") {
            index += 2;
            while index < end {
                let next = text[index..].chars().next().unwrap_or('\0');
                if next == '\n' || next == '\r' {
                    break;
                }
                index += next.len_utf8();
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Comment,
                span(begin, index),
            ));
            continue;
        }
        if ch == '/' && text[index..].starts_with("/*") {
            index += 2;
            while index < end {
                if text[index..].starts_with("*/") {
                    index += 2;
                    break;
                }
                let next = text[index..].chars().next().unwrap_or('\0');
                index += next.len_utf8();
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Comment,
                span(begin, index),
            ));
            continue;
        }
        if ch == '"' {
            index += 1;
            while index < end {
                let next = text[index..].chars().next().unwrap_or('\0');
                if next == '\\' {
                    index += 1;
                    if index < end {
                        let escaped = text[index..].chars().next().unwrap_or('\0');
                        index += escaped.len_utf8();
                    }
                } else if next == '"' {
                    index += 1;
                    break;
                } else if next == '\n' || next == '\r' {
                    break;
                } else {
                    index += next.len_utf8();
                }
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::String,
                span(begin, index),
            ));
            continue;
        }
        if is_identifier_start(ch) {
            index += ch.len_utf8();
            while index < end {
                let next = text[index..].chars().next().unwrap_or('\0');
                if !is_identifier_continue(next) {
                    break;
                }
                index += next.len_utf8();
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Identifier,
                span(begin, index),
            ));
            continue;
        }
        if ch.is_ascii_digit() {
            index += ch.len_utf8();
            while index < end {
                let next = text[index..].chars().next().unwrap_or('\0');
                if next.is_ascii_alphanumeric() || next == '_' || next == '.' {
                    index += next.len_utf8();
                } else {
                    break;
                }
            }
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Number,
                span(begin, index),
            ));
            continue;
        }
        if let Some(operator) = longest_operator(&text[index..end]) {
            index += operator.len();
            pieces.push(Piece::new(
                text[begin..index].to_owned(),
                PieceKind::Punct,
                span(begin, index),
            ));
            continue;
        }
        index += ch.len_utf8();
        let kind = if is_punctuation(ch) {
            PieceKind::Punct
        } else {
            PieceKind::Other
        };
        pieces.push(Piece::new(
            text[begin..index].to_owned(),
            kind,
            span(begin, index),
        ));
    }
    pieces
}

const OPERATORS: &[&str] = &[
    "<<=", ">>=", "##", "++", "--", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "&&",
    "||", "<<", ">>", "&=", "|=", "^=", "->", "::",
];

fn longest_operator(text: &str) -> Option<&'static str> {
    OPERATORS
        .iter()
        .copied()
        .find(|operator| text.starts_with(operator))
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | ','
            | ';'
            | ':'
            | '?'
            | '.'
            | '#'
            | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '='
            | '!'
            | '<'
            | '>'
            | '&'
            | '|'
            | '^'
            | '~'
            | '@'
    )
}

pub(crate) fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

pub(crate) fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// Whether inserting these two fragments directly would form a different token,
/// comment, or operator.
pub(crate) fn needs_separator(left: &str, right: &str) -> bool {
    let Some(left_ch) = left.chars().next_back() else {
        return false;
    };
    let Some(right_ch) = right.chars().next() else {
        return false;
    };
    if left_ch.is_whitespace() || right_ch.is_whitespace() {
        return false;
    }
    if (is_identifier_continue(left_ch) || left_ch == '.')
        && (is_identifier_continue(right_ch) || right_ch == '.')
    {
        return true;
    }
    if left_ch.is_ascii_digit() && right_ch == '.' {
        return true;
    }
    let pair = [left_ch, right_ch];
    let compound = [
        "++", "--", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
        "&=", "|=", "^=", "##",
    ];
    if compound.iter().any(|op| op.chars().eq(pair)) {
        return true;
    }
    matches!(
        (left_ch, right_ch),
        ('/', '/') | ('/', '*') | ('<', '<') | ('>', '>')
    )
}
