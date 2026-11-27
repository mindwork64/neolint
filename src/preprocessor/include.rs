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

fn include_candidates(
    spelling: &str,
    angle: bool,
    current_directory: Option<&Path>,
    include_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    if is_remote_spelling(spelling) {
        return Vec::new();
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
    candidates
}

/// Resolve an include and return every candidate that can affect the result.
///
/// Candidates are in search order and stop at the first existing regular file.
/// If no candidate exists, all searched paths are returned so an editor can
/// invalidate the including root when one of them is later created.
#[cfg(feature = "lsp")]
pub(crate) fn resolve_include_with_dependencies(
    spelling: &str,
    angle: bool,
    current_directory: Option<&Path>,
    include_dirs: &[PathBuf],
) -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut dependencies = Vec::new();
    let candidates = include_candidates(spelling, angle, current_directory, include_dirs);
    for candidate in candidates {
        let found = candidate.is_file();
        dependencies.push(candidate.clone());
        if found {
            return (Some(candidate), dependencies);
        }
    }
    (None, dependencies)
}

/// Resolve an include using the original CLI search behavior.
#[cfg(not(feature = "lsp"))]
pub(crate) fn resolve_include(
    spelling: &str,
    angle: bool,
    current_directory: Option<&Path>,
    include_dirs: &[PathBuf],
) -> Option<PathBuf> {
    include_candidates(spelling, angle, current_directory, include_dirs)
        .into_iter()
        .find(|path| path.is_file())
}

pub(crate) fn display_include_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(all(test, feature = "lsp"))]
mod resolution_tests {
    use super::resolve_include_with_dependencies;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("neolint-include-{label}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_include_returns_every_search_candidate_in_order() {
        let root = temp_dir("missing-candidates");
        let source_dir = root.join("scripts");
        let first_dir = root.join("first");
        let second_dir = root.join("second");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();

        let (resolved, dependencies) = resolve_include_with_dependencies(
            "shared.inc",
            false,
            Some(&source_dir),
            &[first_dir.clone(), second_dir.clone()],
        );

        assert!(resolved.is_none());
        assert_eq!(
            dependencies,
            vec![
                source_dir.join("shared.inc"),
                first_dir.join("shared.inc"),
                second_dir.join("shared.inc"),
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolved_include_tracks_only_higher_priority_candidates_and_the_hit() {
        let root = temp_dir("precedence-candidates");
        let source_dir = root.join("scripts");
        let first_dir = root.join("first");
        let second_dir = root.join("second");
        let third_dir = root.join("third");
        for directory in [&source_dir, &first_dir, &second_dir, &third_dir] {
            fs::create_dir_all(directory).unwrap();
        }
        let selected = second_dir.join("shared.inc");
        fs::write(&selected, "integer shared = 1;").unwrap();

        let (resolved, dependencies) = resolve_include_with_dependencies(
            "shared.inc",
            false,
            Some(&source_dir),
            &[first_dir.clone(), second_dir.clone(), third_dir.clone()],
        );

        assert_eq!(resolved, Some(selected.clone()));
        assert_eq!(
            dependencies,
            vec![
                source_dir.join("shared.inc"),
                first_dir.join("shared.inc"),
                selected,
            ]
        );
        assert!(!dependencies.contains(&third_dir.join("shared.inc")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn angle_include_does_not_watch_the_including_file_directory() {
        let root = temp_dir("angle-candidates");
        let source_dir = root.join("scripts");
        let include_dir = root.join("include");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&include_dir).unwrap();

        let (resolved, dependencies) = resolve_include_with_dependencies(
            "shared.inc",
            true,
            Some(&source_dir),
            std::slice::from_ref(&include_dir),
        );

        assert!(resolved.is_none());
        assert_eq!(dependencies, vec![include_dir.join("shared.inc")]);
        fs::remove_dir_all(root).unwrap();
    }
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
