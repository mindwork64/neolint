//! Per-AnalysisUnit owner of original source files.
//!
//! The manager loads exactly one root per unit, assigns deterministic
//! [`SourceId`] values, enforces bounded reads and UTF-8, and rejects remote or
//! non-regular roots. It never tokenizes LSL and never expands includes.

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::source::file::{AnalysisUnitId, SourceFile, SourceFileKind, SourceId};

/// Maximum size of one root source in bytes.
pub const MAX_ROOT_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Future include-phase limit for one included source.
pub const MAX_INCLUDED_SOURCE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum original source files per AnalysisUnit.
pub const MAX_SOURCE_FILES_PER_UNIT: usize = 256;
/// Maximum root AnalysisUnits in one invocation.
pub const MAX_ANALYSIS_UNITS_PER_INVOCATION: usize = 65_536;

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
#[derive(Debug)]
pub struct SourceManager {
    unit: AnalysisUnitId,
    files: Vec<Arc<SourceFile>>,
    root: Option<SourceId>,
}

impl SourceManager {
    /// Create an empty manager for one unit.
    pub fn new(unit: AnalysisUnitId) -> Self {
        Self {
            unit,
            files: Vec::new(),
            root: None,
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
