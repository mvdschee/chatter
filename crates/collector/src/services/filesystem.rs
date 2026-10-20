use std::fs;

use crate::{Error, Result};

pub fn read_to_string(path: &str) -> Result<String> {
	fs::read_to_string(path).map_err(|e| Error::FileSystem(format!("read {path}: {e}")))
}

/// The file's bytes; `None` if it doesn't exist.
pub fn read(path: &std::path::Path) -> Result<Option<Vec<u8>>> {
	match fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(Error::FileSystem(format!("read {}: {e}", path.display()))),
	}
}

/// Writes the file whole: to a temporary file beside it first, then renamed
/// over it, so a reader never sees half of it.
pub fn write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
	let tmp = path.with_extension("tmp");
	fs::write(&tmp, bytes)
		.and_then(|()| fs::rename(&tmp, path))
		.map_err(|e| Error::FileSystem(format!("write {}: {e}", path.display())))
}

/// How long ago `path` was last written; `None` if it doesn't exist.
pub fn age(path: &std::path::Path) -> Option<std::time::Duration> {
	fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()
}
