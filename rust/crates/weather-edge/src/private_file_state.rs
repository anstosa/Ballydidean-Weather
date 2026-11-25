use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

/// read one optional file without treating absence as an error.
pub(crate) fn read_optional_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    // preserve only a first-run missing file as none
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// create the target parent recursively with private directory mode.
pub(crate) fn create_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // normalize a bare relative file to the current directory
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(parent)
}

/// write one complete private temporary file.
pub(crate) fn write_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)
}

/// atomically replace one private file from a caller-admitted temporary path.
pub(crate) fn atomic_replace_private_file(
    target: &Path,
    temporary: &Path,
    content: &[u8],
) -> io::Result<()> {
    create_parent_directory(target)?;
    write_private_file(temporary, content)?;
    fs::rename(temporary, target)
}
