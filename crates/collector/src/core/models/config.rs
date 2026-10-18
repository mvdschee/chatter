#[derive(Debug, Clone)]
pub struct Config {
	pub bind: String,
	pub db_path: String,
	/// The test client's download, `GET /test.bin`; `None` serves none.
	pub test_file_bytes: Option<usize>,
}
