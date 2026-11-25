//! Load ordered immutable migration artifacts from a read-only filesystem catalog.

use std::cmp::Ordering;
use std::path::Path;

use crate::migrations::{AppliedMigration, MigrationDigest, MigrationScript};

/// Read-only filesystem operations used by migration artifact discovery.
pub trait MigrationArtifactFilesystem {
    type Error;

    /// List the original directory entry names without applying ordering or filtering.
    fn read_directory(&self, directory: &Path) -> Result<Vec<String>, Self::Error>;

    /// Read the exact bytes of one selected migration artifact.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, Self::Error>;
}

/// Source-appropriate filename comparison supplied by the process locale owner.
pub trait MigrationArtifactNameOrder {
    /// Compare two admitted ASCII filenames using the source runtime's ordering policy.
    fn compare_names(&self, left: &str, right: &str) -> Ordering;
}

/// Approved SHA-256 boundary over decoded and re-encoded UTF-8 bytes.
pub trait MigrationArtifactSha256 {
    /// Digest exactly the supplied decoded UTF-8 representation bytes.
    fn sha256(&mut self, decoded_utf8_bytes: &[u8]) -> MigrationDigest;
}

/// Read-only process filesystem implementation for migration artifact loading.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMigrationArtifactFilesystem;

impl MigrationArtifactFilesystem for ProcessMigrationArtifactFilesystem {
    type Error = std::io::Error;

    /// List every directory entry using the source runtime's lossy string boundary.
    fn read_directory(&self, directory: &Path) -> Result<Vec<String>, Self::Error> {
        let mut names = Vec::new();
        // retain every raw directory entry before the exact filename grammar filters it
        for entry in std::fs::read_dir(directory)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        Ok(names)
    }

    /// Read one selected artifact without modifying or opening any sibling entry.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
        std::fs::read(path)
    }
}

/// Closed catalog-loading failure before any database or DDL operation exists.
#[derive(Debug, PartialEq, Eq)]
pub enum MigrationArtifactLoadError<FilesystemError> {
    Filesystem(FilesystemError),
    EmptyCatalog,
}

/// Load exact ordered migration scripts through injected ordering and digest owners.
pub fn load_migration_artifacts<Filesystem, Order, Sha256>(
    directory: &Path,
    filesystem: &Filesystem,
    order: &Order,
    sha256: &mut Sha256,
) -> Result<Vec<MigrationScript>, MigrationArtifactLoadError<Filesystem::Error>>
where
    Filesystem: MigrationArtifactFilesystem,
    Order: MigrationArtifactNameOrder,
    Sha256: MigrationArtifactSha256,
{
    let mut names = filesystem
        .read_directory(directory)
        .map_err(MigrationArtifactLoadError::Filesystem)?
        .into_iter()
        .filter(|name| is_migration_filename(name))
        .collect::<Vec<_>>();
    // delegate every ordering decision instead of approximating locale comparison here
    names.sort_by(|left, right| order.compare_names(left, right));

    // fail before file reads when no exact migration filename remains
    if names.is_empty() {
        return Err(MigrationArtifactLoadError::EmptyCatalog);
    }

    let mut scripts = Vec::with_capacity(names.len());
    // preserve sorted read and result order while retaining exact decoded SQL
    for name in names {
        let bytes = filesystem
            .read_file(&directory.join(&name))
            .map_err(MigrationArtifactLoadError::Filesystem)?;
        let sql = String::from_utf8(bytes).unwrap_or_else(
            // reuse the file allocation for ordinary valid sql and replace malformed text
            |error| String::from_utf8_lossy(error.as_bytes()).into_owned(),
        );
        let digest = sha256.sha256(sql.as_bytes());
        scripts.push(MigrationScript { name, digest, sql });
    }
    Ok(scripts)
}

/// Digest the observed ordered ledger through the source's canonical row representation.
pub fn digest_applied_migration_history(
    applied: &[AppliedMigration],
    sha256: &mut impl MigrationArtifactSha256,
) -> MigrationDigest {
    let mut canonical = Vec::new();
    // retain caller order and one exact colon/newline row boundary per applied migration
    for migration in applied {
        canonical.extend_from_slice(migration.name.as_bytes());
        canonical.push(b':');
        canonical.extend_from_slice(migration.digest.as_str().as_bytes());
        canonical.push(b'\n');
    }
    sha256.sha256(&canonical)
}

/// Match the source's exact ASCII migration filename grammar.
fn is_migration_filename(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(stem) = bytes.strip_suffix(b".sql") else {
        return false;
    };
    // require four ASCII digits, one separator, and at least one admitted suffix byte
    if stem.len() < 6 || !stem[..4].iter().all(u8::is_ascii_digit) || stem[4] != b'_' {
        return false;
    }
    stem[5..]
        .iter()
        .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || *value == b'_')
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FileError {
        Missing,
    }

    struct Filesystem {
        names: Vec<String>,
        reads: RefCell<Vec<PathBuf>>,
        values: BTreeMap<PathBuf, Vec<u8>>,
    }

    impl MigrationArtifactFilesystem for Filesystem {
        type Error = FileError;

        /// Return the injected unsorted directory entry sequence.
        fn read_directory(&self, _directory: &Path) -> Result<Vec<String>, Self::Error> {
            Ok(self.names.clone())
        }

        /// Return only explicitly injected artifact bytes and retain read order.
        fn read_file(&self, path: &Path) -> Result<Vec<u8>, Self::Error> {
            self.reads.borrow_mut().push(path.to_owned());
            self.values.get(path).cloned().ok_or(FileError::Missing)
        }
    }

    #[derive(Default)]
    struct SourceOrder {
        comparisons: Cell<usize>,
    }

    impl MigrationArtifactNameOrder for SourceOrder {
        /// Reproduce the fixture's source ordering after proving the injected port was used.
        fn compare_names(&self, left: &str, right: &str) -> Ordering {
            self.comparisons.set(self.comparisons.get() + 1);
            left.cmp(right)
        }
    }

    #[derive(Default)]
    struct Sha256 {
        inputs: Vec<Vec<u8>>,
    }

    impl MigrationArtifactSha256 for Sha256 {
        /// Retain exact decoded UTF-8 bytes and return one opaque approved fixture digest.
        fn sha256(&mut self, decoded_utf8_bytes: &[u8]) -> MigrationDigest {
            self.inputs.push(decoded_utf8_bytes.to_vec());
            MigrationDigest::admitted(format!("digest-{}", self.inputs.len()))
        }
    }

    /// Filter ordinary sibling entries, sort through the port, and load exact decoded SQL.
    #[test]
    fn loads_only_exact_names_in_injected_order_with_lossy_utf8_hash_bytes() {
        let directory = Path::new("/fixture/migrations");
        let first = directory.join("0001_initial.sql");
        let second = directory.join("0010_replacement.sql");
        let filesystem = Filesystem {
            names: vec![
                "README.md".to_owned(),
                "0010_replacement.sql".to_owned(),
                "0001_initial.sql".to_owned(),
                "002_bad.sql".to_owned(),
                "0002_UPPER.sql".to_owned(),
                "0003_missing.SQL".to_owned(),
                "0004_non-ascii-é.sql".to_owned(),
            ],
            reads: RefCell::new(Vec::new()),
            values: BTreeMap::from([
                (first.clone(), b"SELECT 1;\n".to_vec()),
                (second.clone(), b"SELECT '\xff';\n".to_vec()),
            ]),
        };
        let order = SourceOrder::default();
        let mut sha256 = Sha256::default();
        let scripts =
            load_migration_artifacts(directory, &filesystem, &order, &mut sha256).unwrap();
        assert_eq!(
            scripts
                .iter()
                .map(|script| script.name.as_str())
                .collect::<Vec<_>>(),
            ["0001_initial.sql", "0010_replacement.sql"]
        );
        assert_eq!(scripts[0].sql, "SELECT 1;\n");
        assert_eq!(scripts[1].sql, "SELECT '\u{fffd}';\n");
        assert_eq!(scripts[0].digest.as_str(), "digest-1");
        assert_eq!(scripts[1].digest.as_str(), "digest-2");
        assert_eq!(sha256.inputs[0], b"SELECT 1;\n");
        assert_eq!(sha256.inputs[1], "SELECT '\u{fffd}';\n".as_bytes());
        assert_eq!(filesystem.reads.borrow().as_slice(), [first, second]);
        assert!(order.comparisons.get() > 0);
    }

    /// Refuse an ordinary directory whose entries contain no exact migration scripts.
    #[test]
    fn refuses_directory_without_ordered_sql_files() {
        let filesystem = Filesystem {
            names: vec!["README.md".to_owned(), "0001_bad.txt".to_owned()],
            reads: RefCell::new(Vec::new()),
            values: BTreeMap::new(),
        };
        let mut sha256 = Sha256::default();
        assert_eq!(
            load_migration_artifacts(
                Path::new("/fixture/migrations"),
                &filesystem,
                &SourceOrder::default(),
                &mut sha256,
            ),
            Err(MigrationArtifactLoadError::EmptyCatalog)
        );
        assert!(filesystem.reads.borrow().is_empty());
        assert!(sha256.inputs.is_empty());
    }

    /// Hash exact applied order with the source colon and newline row boundaries.
    #[test]
    fn digests_canonical_applied_history_preimage() {
        let applied = [
            AppliedMigration {
                name: "0001_initial.sql".to_owned(),
                digest: MigrationDigest::admitted("first-checksum"),
            },
            AppliedMigration {
                name: "0002_next.sql".to_owned(),
                digest: MigrationDigest::admitted("second-checksum"),
            },
        ];
        let mut sha256 = Sha256::default();
        let digest = digest_applied_migration_history(&applied, &mut sha256);
        assert_eq!(digest.as_str(), "digest-1");
        assert_eq!(
            sha256.inputs,
            [b"0001_initial.sql:first-checksum\n0002_next.sql:second-checksum\n".to_vec()]
        );
    }
}
