//! Include graph and deterministic local include resolution.

use std::path::{Path, PathBuf};

use crate::preprocessor::diagnostics::{DiagSink, PRE_INVALID_INCLUDE};
use crate::preprocessor::token::{MappedText, RawSpan};
use crate::source::file::SourceId;
use crate::source::map::SourceSpan;

/// One original source file participating in an include graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeNode {
    source: SourceId,
    display_name: String,
    canonical_path: Option<PathBuf>,
}

impl IncludeNode {
    pub(crate) fn new(
        source: SourceId,
        display_name: String,
        canonical_path: Option<PathBuf>,
    ) -> Self {
        Self {
            source,
            display_name,
            canonical_path,
        }
    }

    /// Source identity.
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// User-facing display spelling.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Canonical local path, when the node came from the filesystem.
    pub fn canonical_path(&self) -> Option<&Path> {
        self.canonical_path.as_deref()
    }
}

/// One textual include edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeEdge {
    from: SourceId,
    to: SourceId,
    path: SourceSpan,
    directive: SourceSpan,
    angle: bool,
}

impl IncludeEdge {
    pub(crate) fn new(
        from: SourceId,
        to: SourceId,
        path: SourceSpan,
        directive: SourceSpan,
        angle: bool,
    ) -> Self {
        Self {
            from,
            to,
            path,
            directive,
            angle,
        }
    }

    /// Including file.
    pub fn from(&self) -> SourceId {
        self.from
    }

    /// Included file.
    pub fn to(&self) -> SourceId {
        self.to
    }

    /// Exact spelling range of the path.
    pub fn path(&self) -> SourceSpan {
        self.path
    }

    /// Full `#include` directive range.
    pub fn directive(&self) -> SourceSpan {
        self.directive
    }

    /// Whether angle brackets were used.
    pub fn is_angle(&self) -> bool {
        self.angle
    }
}

/// Deterministic include graph for one AnalysisUnit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncludeGraph {
    nodes: Vec<IncludeNode>,
    edges: Vec<IncludeEdge>,
}

impl IncludeGraph {
    pub(crate) fn new(nodes: Vec<IncludeNode>, edges: Vec<IncludeEdge>) -> Self {
        Self { nodes, edges }
    }

    /// Files in SourceId load order.
    pub fn nodes(&self) -> &[IncludeNode] {
        &self.nodes
    }

    /// Include edges in textual expansion order.
    pub fn edges(&self) -> &[IncludeEdge] {
        &self.edges
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IncludeDirective {
    pub(crate) spelling: String,
    pub(crate) path_span: RawSpan,
    pub(crate) angle: bool,
}

pub(crate) fn parse_include(rest: &MappedText, diags: &mut DiagSink) -> Option<IncludeDirective> {
    let text = rest.text();
    let mut position = 0usize;
    loop {
        while position < text.len() {
            let ch = text[position..].chars().next().unwrap_or('\0');
            if ch.is_ascii_whitespace() {
                position += ch.len_utf8();
            } else {
                break;
            }
        }
        if text[position..].starts_with("/*") {
            if let Some(close) = text[position + 2..].find("*/") {
                position += 2 + close + 2;
                continue;
            }
        }
        if text[position..].starts_with("//") {
            return None;
        }
        break;
    }
    if position >= text.len() {
        diags.push(
            PRE_INVALID_INCLUDE,
            "#include requires a quoted or angle-bracket path",
            rest.raw_span(position, position),
        );
        return None;
    }
    let open = text[position..].chars().next().unwrap_or('\0');
    let (close, angle) = match open {
        '"' => ('"', false),
        '<' => ('>', true),
        _ => {
            diags.push(
                PRE_INVALID_INCLUDE,
                "#include path must use \"...\" or <...>",
                rest.raw_span(position, (position + open.len_utf8()).min(text.len())),
            );
            return None;
        }
    };
    position += open.len_utf8();
    let path_start = position;
    while position < text.len() {
        let ch = text[position..].chars().next().unwrap_or('\0');
        if ch == close || ch == '\n' || ch == '\r' {
            break;
        }
        position += ch.len_utf8();
    }
    if position >= text.len() || !text[position..].starts_with(close) {
        diags.push(
            PRE_INVALID_INCLUDE,
            "unterminated #include path",
            rest.raw_span(path_start.saturating_sub(1), text.len()),
        );
        return None;
    }
    let spelling = text[path_start..position].to_owned();
    if spelling.is_empty() {
        diags.push(
            PRE_INVALID_INCLUDE,
            "#include path is empty",
            rest.raw_span(path_start, position),
        );
        return None;
    }
    let path_span = rest.raw_span(path_start, position);
    position += close.len_utf8();
    let trailing = rest.slice(position, text.len());
    if !trailing.text().is_empty() && tokenize_for_trivia(trailing.text()).is_none() {
        diags.push(
            PRE_INVALID_INCLUDE,
            "unexpected tokens after #include path",
            trailing.raw_span(0, trailing.text().len()),
        );
        return None;
    }
    Some(IncludeDirective {
        spelling,
        path_span,
        angle,
    })
}

pub(crate) fn is_remote_spelling(spelling: &str) -> bool {
    let lower = spelling.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || spelling.starts_with("//")
        || spelling.starts_with("\\\\")
}

pub(crate) fn resolve_include(
    spelling: &str,
    angle: bool,
    current_directory: Option<&Path>,
    include_dirs: &[PathBuf],
) -> Option<PathBuf> {
    if is_remote_spelling(spelling) {
        return None;
    }
    let requested = Path::new(spelling);
    let mut candidates = Vec::new();
    if requested.is_absolute() {
        candidates.push(requested.to_path_buf());
    } else {
        if !angle {
            if let Some(directory) = current_directory {
                candidates.push(directory.join(requested));
            }
        }
        for directory in include_dirs {
            candidates.push(directory.join(requested));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub(crate) fn display_include_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn tokenize_for_trivia(text: &str) -> Option<()> {
    let mut index = 0usize;
    while index < text.len() {
        let remainder = &text[index..];
        if remainder.starts_with("//") {
            return Some(());
        }
        if let Some(comment) = remainder.strip_prefix("/*") {
            let close = comment.find("*/")?;
            index += 2 + close + 2;
            continue;
        }
        let ch = remainder.chars().next()?;
        if ch.is_ascii_whitespace() {
            index += ch.len_utf8();
            continue;
        }
        return None;
    }
    Some(())
}
