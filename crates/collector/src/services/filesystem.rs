use std::fs;

use crate::{Error, Result};

pub fn read_to_string(path: &str) -> Result<String> {
	fs::read_to_string(path).map_err(|e| Error::FileSystem(format!("read {path}: {e}")))
}
