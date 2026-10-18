use crate::core::models::scan::ScanTargets;

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
	/// `chatter-collector import <file>`: load that file of reports, then exit.
	pub import: Option<String>,
}
