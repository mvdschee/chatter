use std::fs;

use crate::{Error, Result};

pub fn read_to_string(path: &str) -> Result<String> {
	fs::read_to_string(path).map_err(|e| Error::FileSystem(format!("read {path}: {e}")))
}

/// How long ago `path` was last written; `None` if it doesn't exist.
pub fn age(path: &std::path::Path) -> Option<std::time::Duration> {
	fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()
}
