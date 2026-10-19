use std::path::{Path, PathBuf};

use crate::core::models::scan::{PortScanAt, ScanTargets};

/// What the binary was started to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	/// The server, with the scans in the background.
	Serve,
	/// `import <file>`: load a file of reports, then exit.
	Import(String),
	/// `port-scan`: one port scan now, then exit.
	PortScan,
}

#[derive(Debug, Clone)]
pub struct Config {
	pub bind: String,
	pub db_path: String,
	/// The test client's download, `GET /test.bin`; `None` serves none.
	pub test_file_bytes: Option<usize>,
	/// Development: serve the page from this directory, read on every request.
	pub ui_dir: Option<String>,
	/// What the host sweep covers; `None`: the /24 of this machine's network.
	pub scan_targets: Option<ScanTargets>,
	pub sweep_every_min: u64,
	/// The daily port scan's time; `None`: no port scans.
	pub port_scan_at: Option<PortScanAt>,
	pub command: Command,
}

impl Config {
	/// Where the collector keeps its files: the database's directory.
	pub fn data_dir(&self) -> PathBuf {
		match Path::new(&self.db_path).parent() {
			Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
			_ => PathBuf::from("."),
		}
	}
}
