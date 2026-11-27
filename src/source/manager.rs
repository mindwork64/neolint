//! Per-AnalysisUnit owner of original source files.
//!
//! The manager loads exactly one root per unit, assigns deterministic
//! [`SourceId`] values, enforces bounded reads and UTF-8, and rejects remote or
//! non-regular roots. It never tokenizes LSL and never expands includes.

#[cfg(feature = "lsp")]
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::source::file::{AnalysisUnitId, SourceFile, SourceFileKind, SourceId};

/// Maximum size of one root source in bytes.
pub const MAX_ROOT_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum size of one included source.
pub const MAX_INCLUDED_SOURCE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum original source files per AnalysisUnit.
pub const MAX_SOURCE_FILES_PER_UNIT: usize = 256;

/// Maximum distinct local include paths retained by one LSP analysis.
#[cfg(feature = "lsp")]
pub(crate) const MAX_TRACKED_INCLUDE_DEPENDENCIES: usize = 4096;
/// Maximum root AnalysisUnits in one invocation.
pub const MAX_ANALYSIS_UNITS_PER_INVOCATION: usize = 65_536;

/// Normalize a local path for dependency identity, including paths that do
/// not exist yet. Existing ancestors are canonicalized so a missing leaf below
/// a symlinked directory matches the URI used after the file is created.
#[cfg(feature = "lsp")]
pub(crate) fn normalize_local_path(path: &Path) -> PathBuf {
    use std::ffi::OsString;
    use std::path::Component;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    let mut lexical = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => lexical.push(prefix.as_os_str()),
            Component::RootDir => lexical.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if lexical.file_name().is_some() {
                    lexical.pop();
                }
            }
            Component::Normal(part) => lexical.push(part),
        }
    }

    let mut ancestor = lexical.clone();
    let mut suffix = Vec::<OsString>::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            break;
        };
        suffix.push(name.to_os_string());
        if !ancestor.pop() {
            break;
        }
    }

    if let Ok(mut canonical) = fs::canonicalize(&ancestor) {
        for component in suffix.iter().rev() {
            canonical.push(component);
        }
        canonical
    } else {
        lexical
    }
}

/// Identity of a path when its final component is missing, while resolving
/// existing parent directories (including symlinks). This preserves the
/// spelling used for a symlinked include after that file is unlinked.
#[cfg(feature = "lsp")]
pub(crate) fn normalize_missing_leaf_path(path: &Path) -> PathBuf {
    let Some(file_name) = path.file_name() else {
        return normalize_local_path(path);
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut normalized = normalize_local_path(parent);
    normalized.push(file_name);
    normalized
}

/// Failure to load an original source file.
#[derive(Debug)]
pub enum SourceLoadError {
    /// Filesystem or reader error.
    Io(io::Error),
    /// The bytes are not valid UTF-8.
    InvalidUtf8,
    /// The source exceeds its byte limit.
    TooLarge {
        /// Configured limit in bytes.
        limit: usize,
    },
    /// The path is not a regular file.
    NotRegularFile {
        /// Display spelling of the path.
        path: String,
    },
    /// A URL or network/UNC path is not a local source.
    RemotePath {
        /// Rejected spelling.
        spelling: String,
    },
    /// The unit already has a root.
    RootAlreadyLoaded,
    /// The per-unit source count limit was reached.
    TooManyFiles {
        /// Configured limit.
        limit: usize,
    },
    /// The unit-local file index space is exhausted.
    FileIndexExhausted,
}

impl fmt::Display for SourceLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot read source: {error}"),
            Self::InvalidUtf8 => formatter.write_str("source is not valid UTF-8"),
            Self::TooLarge { limit } => write!(formatter, "source exceeds {limit} bytes"),
            Self::NotRegularFile { path } => {
                write!(formatter, "'{path}' is not a regular file")
            }
            Self::RemotePath { spelling } => {
                write!(
                    formatter,
                    "remote or network source is not supported: {spelling}"
                )
            }
            Self::RootAlreadyLoaded => formatter.write_str("unit already has a root source"),
            Self::TooManyFiles { limit } => {
                write!(formatter, "unit exceeds {limit} source files")
            }
            Self::FileIndexExhausted => formatter.write_str("source file index space exhausted"),
        }
    }
}

impl std::error::Error for SourceLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for SourceLoadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Owner of the original sources for one AnalysisUnit.
#[derive(Debug, Clone)]
pub struct SourceManager {
    unit: AnalysisUnitId,
    files: Vec<Arc<SourceFile>>,
    root: Option<SourceId>,
    #[cfg(feature = "lsp")]
    include_dependencies: HashSet<PathBuf>,
    #[cfg(feature = "lsp")]
    include_dependencies_complete: bool,
}

impl SourceManager {
    /// Create an empty manager for one unit.
    pub fn new(unit: AnalysisUnitId) -> Self {
        Self {
            unit,
            files: Vec::new(),
            root: None,
            #[cfg(feature = "lsp")]
            include_dependencies: HashSet::new(),
            #[cfg(feature = "lsp")]
            include_dependencies_complete: true,
        }
    }

    /// Unit that owns these sources.
    pub fn unit(&self) -> AnalysisUnitId {
        self.unit
    }

    /// All loaded files, in deterministic file-index order.
    pub fn files(&self) -> &[Arc<SourceFile>] {
        &self.files
    }

    /// Paths that can affect include resolution, including missing candidates.
    #[cfg(feature = "lsp")]
    pub(crate) fn include_dependencies(&self) -> &HashSet<PathBuf> {
        &self.include_dependencies
    }

    /// Whether every dependency candidate could be retained within the budget.
    #[cfg(feature = "lsp")]
    pub(crate) fn include_dependencies_complete(&self) -> bool {
        self.include_dependencies_complete
    }

    /// Record a path that can affect include resolution.
    ///
    /// Missing candidates are retained as absolute normalized paths. Once the
    /// bounded set is full, mark the snapshot incomplete so the LSP can fall
    /// back to conservative invalidation instead of silently missing an edge.
    #[cfg(feature = "lsp")]
    pub(crate) fn record_include_dependency(&mut self, path: &Path) -> bool {
        if !self.include_dependencies_complete {
            return false;
        }
        let normalized = [
            normalize_local_path(path),
            normalize_missing_leaf_path(path),
        ];
        for dependency in normalized {
            if self.include_dependencies.contains(&dependency) {
                continue;
            }
            if self.include_dependencies.len() >= MAX_TRACKED_INCLUDE_DEPENDENCIES {
                self.include_dependencies_complete = false;
                return false;
            }
            self.include_dependencies.insert(dependency);
        }
        true
    }

    /// Root source, if one was loaded.
    pub fn root(&self) -> Option<SourceId> {
        self.root
    }

    /// File registered for `id` in this unit.
    pub fn file(&self, id: SourceId) -> Option<&SourceFile> {
        if id.unit() != self.unit {
            return None;
        }
        self.files
            .get(id.file() as usize)
            .map(|file| file.as_ref())
            .filter(|file| file.id() == id)
    }

    /// Text of a registered file.
    pub fn text(&self, id: SourceId) -> Option<&str> {
        self.file(id).map(SourceFile::text)
    }

    /// Load a root source from a regular local file.
    pub fn load_root_file(&mut self, path: impl AsRef<Path>) -> Result<SourceId, SourceLoadError> {
        if self.root.is_some() {
            return Err(SourceLoadError::RootAlreadyLoaded);
        }
        let path = path.as_ref();
        let display = path.to_string_lossy().into_owned();
        reject_remote(&display)?;
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(SourceLoadError::NotRegularFile { path: display });
        }
        self.check_capacity()?;
        let file = fs::File::open(path)?;
        let bytes = read_bounded(file, MAX_ROOT_SOURCE_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|_| SourceLoadError::InvalidUtf8)?;
        let canonical_path = fs::canonicalize(path).ok();
        self.register(
            display,
            canonical_path,
            SourceFileKind::Root,
            Arc::from(text),
        )
    }

    /// Load a root source from a supplied reader (for example stdin).
    ///
    /// The library never reads the process stdin itself; the caller supplies
    /// the reader or already-read text.
    pub fn load_root_reader(
        &mut self,
        display_name: &str,
        reader: impl Read,
    ) -> Result<SourceId, SourceLoadError> {
        if self.root.is_some() {
            return Err(SourceLoadError::RootAlreadyLoaded);
        }
        reject_remote(display_name)?;
        self.check_capacity()?;
        let bytes = read_bounded(reader, MAX_ROOT_SOURCE_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|_| SourceLoadError::InvalidUtf8)?;
        self.register(
            display_name.to_owned(),
            None,
            SourceFileKind::Stdin,
            Arc::from(text),
        )
    }

    /// Load a root source from already-read text.
    pub fn load_root_text(
        &mut self,
        display_name: &str,
        text: &str,
    ) -> Result<SourceId, SourceLoadError> {
        if self.root.is_some() {
            return Err(SourceLoadError::RootAlreadyLoaded);
        }
        reject_remote(display_name)?;
        if text.len() > MAX_ROOT_SOURCE_BYTES {
            return Err(SourceLoadError::TooLarge {
                limit: MAX_ROOT_SOURCE_BYTES,
            });
        }
        self.check_capacity()?;
        self.register(
            display_name.to_owned(),
            None,
            SourceFileKind::Stdin,
            Arc::from(text),
        )
    }

    /// Load an included local source, reusing an existing `SourceId` for the
    /// same canonical path. The caller supplies the display spelling so the
    /// original relative name is preserved.
    pub fn load_included_file(
        &mut self,
        path: impl AsRef<Path>,
        display_name: &str,
    ) -> Result<SourceId, SourceLoadError> {
        let path = path.as_ref();
        reject_remote(display_name)?;
        let display = path
            .to_str()
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        reject_remote(&display)?;
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(SourceLoadError::NotRegularFile {
                path: display_name.to_owned(),
            });
        }
        let canonical = fs::canonicalize(path).ok();
        if let Some(canonical) = &canonical {
            if let Some(file) = self
                .files
                .iter()
                .find(|file| file.canonical_path() == Some(canonical))
            {
                return Ok(file.id());
            }
        }
        self.check_capacity()?;
        let file = fs::File::open(path)?;
        let bytes = read_bounded(file, MAX_INCLUDED_SOURCE_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|_| SourceLoadError::InvalidUtf8)?;
        self.register(
            display_name.to_owned(),
            canonical,
            SourceFileKind::Included,
            Arc::from(text),
        )
    }

    /// Register included text supplied by an immutable editor-buffer snapshot.
    /// The path must still identify an existing regular local file.
    pub(crate) fn load_included_text(
        &mut self,
        path: impl AsRef<Path>,
        display_name: &str,
        text: &str,
    ) -> Result<SourceId, SourceLoadError> {
        let path = path.as_ref();
        reject_remote(display_name)?;
        let display = path
            .to_str()
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        reject_remote(&display)?;
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(SourceLoadError::NotRegularFile {
                path: display_name.to_owned(),
            });
        }
        let canonical = fs::canonicalize(path).ok();
        if let Some(canonical) = &canonical {
            if let Some(file) = self
                .files
                .iter()
                .find(|file| file.canonical_path() == Some(canonical))
            {
                return Ok(file.id());
            }
        }
        if text.len() > MAX_INCLUDED_SOURCE_BYTES {
            return Err(SourceLoadError::TooLarge {
                limit: MAX_INCLUDED_SOURCE_BYTES,
            });
        }
        self.check_capacity()?;
        self.register(
            display_name.to_owned(),
            canonical,
            SourceFileKind::Included,
            Arc::from(text),
        )
    }

    fn check_capacity(&self) -> Result<(), SourceLoadError> {
        if self.files.len() >= MAX_SOURCE_FILES_PER_UNIT {
            return Err(SourceLoadError::TooManyFiles {
                limit: MAX_SOURCE_FILES_PER_UNIT,
            });
        }
        if self.files.len() > u32::MAX as usize {
            return Err(SourceLoadError::FileIndexExhausted);
        }
        Ok(())
    }

    fn register(
        &mut self,
        display_name: String,
        canonical_path: Option<PathBuf>,
        kind: SourceFileKind,
        text: Arc<str>,
    ) -> Result<SourceId, SourceLoadError> {
        let file_index =
            u32::try_from(self.files.len()).map_err(|_| SourceLoadError::FileIndexExhausted)?;
        let id = SourceId::new(self.unit, file_index);
        let file = Arc::new(SourceFile::new(
            id,
            display_name,
            canonical_path,
            kind,
            text,
        ));
        self.files.push(file);
        if self.root.is_none() {
            self.root = Some(id);
        }
        Ok(id)
    }
}

fn reject_remote(spelling: &str) -> Result<(), SourceLoadError> {
    let lower = spelling.to_ascii_lowercase();
    let remote = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || spelling.starts_with("//")
        || spelling.starts_with("\\\\");
    if remote {
        Err(SourceLoadError::RemotePath {
            spelling: spelling.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, SourceLoadError> {
    let mut bytes = Vec::new();
    let mut limited = reader.take(limit as u64 + 1);
    limited.read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(SourceLoadError::TooLarge { limit });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::file::RunId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn unit(index: u32) -> AnalysisUnitId {
        AnalysisUnitId::new(RunId::new(1), index)
    }

    fn temp_dir() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "neolint-phase1-manager-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn loads_file_root_and_preserves_display_spelling() {
        let directory = temp_dir();
        let path = directory.join("script.lsl");
        fs::write(&path, "default {}").unwrap();
        let mut manager = SourceManager::new(unit(0));
        let id = manager.load_root_file(&path).unwrap();
        let file = manager.file(id).unwrap();
        assert_eq!(id.file(), 0);
        assert_eq!(file.kind(), SourceFileKind::Root);
        assert_eq!(file.text(), "default {}");
        assert_eq!(file.display_name(), path.to_string_lossy());
        assert_eq!(file.canonical_path(), fs::canonicalize(&path).ok().as_ref());
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn records_missing_include_paths_and_keeps_identity_after_creation() {
        let directory = temp_dir();
        let path = directory.join("not-created-yet").join("shared.inc");
        let mut manager = SourceManager::new(unit(0));

        assert!(manager.record_include_dependency(&path));
        let recorded = manager
            .include_dependencies()
            .iter()
            .next()
            .unwrap()
            .clone();
        assert!(recorded.is_absolute());
        assert!(manager.files().is_empty());

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "integer shared = 1;").unwrap();
        assert_eq!(normalize_local_path(&path), recorded);

        #[cfg(unix)]
        {
            let real_parent = directory.join("real-parent");
            let alias_parent = directory.join("alias-parent");
            fs::create_dir_all(&real_parent).unwrap();
            std::os::unix::fs::symlink(&real_parent, &alias_parent).unwrap();
            let missing_via_alias = alias_parent.join("not-created.inc");
            let normalized_before_creation = normalize_local_path(&missing_via_alias);
            let missing_file = real_parent.join("not-created.inc");
            fs::write(&missing_file, "integer shared = 1;").unwrap();
            assert_eq!(
                normalized_before_creation,
                normalize_local_path(&missing_file)
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[cfg(feature = "lsp")]
    #[test]
    fn records_symlink_include_identity_after_the_link_is_removed() {
        let directory = temp_dir();
        let target = directory.join("target.inc");
        let alias = directory.join("shared.inc");
        fs::write(&target, "integer shared = 1;").unwrap();
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let mut manager = SourceManager::new(unit(0));

        assert!(manager.record_include_dependency(&alias));
        let canonical = normalize_local_path(&alias);
        let missing_alias = normalize_missing_leaf_path(&alias);
        assert_ne!(canonical, missing_alias);
        assert!(manager.include_dependencies().contains(&canonical));
        assert!(manager.include_dependencies().contains(&missing_alias));

        fs::remove_file(&alias).unwrap();
        assert_eq!(normalize_local_path(&alias), missing_alias);
        assert!(manager
            .include_dependencies()
            .contains(&normalize_local_path(&alias)));
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(feature = "lsp")]
    #[test]
    fn include_dependency_budget_marks_tracking_incomplete_without_growing_the_set() {
        let directory = temp_dir();
        let mut manager = SourceManager::new(unit(0));
        for index in 0..MAX_TRACKED_INCLUDE_DEPENDENCIES {
            assert!(
                manager.record_include_dependency(&directory.join(format!("missing-{index}.inc")))
            );
        }
        assert_eq!(
            manager.include_dependencies().len(),
            MAX_TRACKED_INCLUDE_DEPENDENCIES
        );
        assert!(manager.include_dependencies_complete());

        assert!(!manager.record_include_dependency(&directory.join("overflow.inc")));
        assert_eq!(
            manager.include_dependencies().len(),
            MAX_TRACKED_INCLUDE_DEPENDENCIES
        );
        assert!(!manager.include_dependencies_complete());
        assert!(!manager.record_include_dependency(&directory.join("later.inc")));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_included_editor_text_without_replacing_disk_contents() {
        let directory = temp_dir();
        let path = directory.join("shared.inc");
        fs::write(&path, "integer from_disk = 1;").unwrap();
        let mut manager = SourceManager::new(unit(0));
        let id = manager
            .load_included_text(&path, "shared.inc", "integer from_buffer = 2;")
            .unwrap();
        let file = manager.file(id).unwrap();
        assert_eq!(file.kind(), SourceFileKind::Included);
        assert_eq!(file.text(), "integer from_buffer = 2;");
        assert_eq!(file.canonical_path(), fs::canonicalize(&path).ok().as_ref());
        assert_eq!(fs::read_to_string(&path).unwrap(), "integer from_disk = 1;");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn included_editor_text_respects_the_source_size_limit() {
        let directory = temp_dir();
        let path = directory.join("shared.inc");
        fs::write(&path, "").unwrap();
        let mut manager = SourceManager::new(unit(0));
        assert!(matches!(
            manager.load_included_text(
                &path,
                "shared.inc",
                &"x".repeat(MAX_INCLUDED_SOURCE_BYTES + 1),
            ),
            Err(SourceLoadError::TooLarge {
                limit: MAX_INCLUDED_SOURCE_BYTES
            })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_stdin_text_and_reader_roots() {
        let mut manager = SourceManager::new(unit(0));
        let id = manager.load_root_text("<stdin>", "default {}").unwrap();
        assert_eq!(manager.text(id), Some("default {}"));
        assert_eq!(manager.file(id).unwrap().kind(), SourceFileKind::Stdin);
        assert!(manager.file(id).unwrap().canonical_path().is_none());

        let mut manager = SourceManager::new(unit(1));
        let id = manager
            .load_root_reader("<stdin>", "default {}".as_bytes())
            .unwrap();
        assert_eq!(manager.text(id), Some("default {}"));
    }

    #[test]
    fn rejects_invalid_utf8() {
        let directory = temp_dir();
        let path = directory.join("bad.lsl");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        let mut manager = SourceManager::new(unit(0));
        assert!(matches!(
            manager.load_root_file(&path),
            Err(SourceLoadError::InvalidUtf8)
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_non_regular_roots() {
        let directory = temp_dir();
        let mut manager = SourceManager::new(unit(0));
        assert!(matches!(
            manager.load_root_file(&directory),
            Err(SourceLoadError::NotRegularFile { .. })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_remote_and_network_paths() {
        for spelling in [
            "https://example.invalid/a.lsl",
            "http://example.invalid/a.lsl",
            "ftp://example.invalid/a.lsl",
            "//server/share/a.lsl",
            "\\\\server\\share\\a.lsl",
        ] {
            let mut manager = SourceManager::new(unit(0));
            assert!(
                matches!(
                    manager.load_root_file(spelling),
                    Err(SourceLoadError::RemotePath { .. })
                ),
                "{spelling}"
            );
        }
    }

    #[test]
    fn enforces_included_size_boundary_with_bounded_reads() {
        let directory = temp_dir();
        let exact = directory.join("exact.lsl");
        let too_large = directory.join("large.lsl");
        fs::write(&exact, vec![b'a'; MAX_INCLUDED_SOURCE_BYTES]).unwrap();
        fs::write(&too_large, vec![b'a'; MAX_INCLUDED_SOURCE_BYTES + 1]).unwrap();

        let mut manager = SourceManager::new(unit(0));
        assert!(manager.load_included_file(&exact, "exact.lsl").is_ok());

        let mut manager = SourceManager::new(unit(1));
        assert!(matches!(
            manager.load_included_file(&too_large, "large.lsl"),
            Err(SourceLoadError::TooLarge {
                limit: MAX_INCLUDED_SOURCE_BYTES
            })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn enforces_root_size_boundary_with_bounded_reads() {
        let directory = temp_dir();
        let exact = directory.join("exact.lsl");
        let too_large = directory.join("large.lsl");
        fs::write(&exact, vec![b'a'; MAX_ROOT_SOURCE_BYTES]).unwrap();
        fs::write(&too_large, vec![b'a'; MAX_ROOT_SOURCE_BYTES + 1]).unwrap();

        let mut manager = SourceManager::new(unit(0));
        assert!(manager.load_root_file(&exact).is_ok());

        let mut manager = SourceManager::new(unit(1));
        assert!(matches!(
            manager.load_root_file(&too_large),
            Err(SourceLoadError::TooLarge {
                limit: MAX_ROOT_SOURCE_BYTES
            })
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn loads_only_one_root() {
        let mut manager = SourceManager::new(unit(0));
        manager.load_root_text("a.lsl", "default {}").unwrap();
        assert!(matches!(
            manager.load_root_text("b.lsl", "default {}"),
            Err(SourceLoadError::RootAlreadyLoaded)
        ));
    }

    #[test]
    fn assigns_deterministic_unit_local_file_ids() {
        let first = SourceManager::new(unit(7));
        let second = SourceManager::new(unit(8));
        let mut first = first;
        let mut second = second;
        assert_eq!(first.load_root_text("root.lsl", "").unwrap().file(), 0);
        assert_eq!(second.load_root_text("root.lsl", "").unwrap().file(), 0);
        assert_ne!(
            first.file(SourceId::new(unit(7), 0)).unwrap().id(),
            second.file(SourceId::new(unit(8), 0)).unwrap().id()
        );
        assert!(first.file(SourceId::new(unit(8), 0)).is_none());
    }

    #[test]
    fn enforces_source_file_count_capacity() {
        let mut manager = SourceManager::new(unit(0));
        for index in 0..MAX_SOURCE_FILES_PER_UNIT {
            manager
                .register(
                    format!("file-{index}.lsl"),
                    None,
                    SourceFileKind::Included,
                    Arc::from(""),
                )
                .unwrap();
        }
        assert!(matches!(
            manager.check_capacity(),
            Err(SourceLoadError::TooManyFiles {
                limit: MAX_SOURCE_FILES_PER_UNIT
            })
        ));
    }
}
