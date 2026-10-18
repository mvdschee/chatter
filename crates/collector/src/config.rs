use std::env;

use crate::{Error, Result, core::models::config::Config};

/// Where to listen.
const BIND_KEY: &str = "CHATTER_BIND";
const BIND_DEFAULT: &str = "127.0.0.1:8080";

/// The SQLite file; created when missing.
const DB_KEY: &str = "CHATTER_DB";
const DB_DEFAULT: &str = "chatter.db";

/// Development: the directory with the page's files (`crates/collector/ui`),
/// read on every request. Unset: the files built into the binary.
const UI_DIR_KEY: &str = "CHATTER_UI_DIR";

/// With a size, `GET /test.bin` serves that many MiB for the ESP32-C3 test
/// client. Unset: no test file.
const TEST_FILE_MIB_KEY: &str = "CHATTER_TEST_FILE_MIB";

impl Config {
	pub fn init() -> Result<Self> {
		// local runs: the repo's .env (cargo runs from the repo root); a
		// container has no .env and sets the same keys in its environment.
		// Keys already set win.
		let _ = dotenvy::dotenv();
		let test_file_bytes = match load_env(TEST_FILE_MIB_KEY) {
			Ok(mib) => Some(
				mib.parse::<usize>().map_err(|_| {
					Error::Config(format!("{TEST_FILE_MIB_KEY}: not a number: {mib}"))
				})? * 1024 * 1024,
			),
			Err(_) => None,
		};
		Ok(Self {
			bind: load_env(BIND_KEY).unwrap_or_else(|_| BIND_DEFAULT.to_string()),
			db_path: load_env(DB_KEY).unwrap_or_else(|_| DB_DEFAULT.to_string()),
			test_file_bytes,
			ui_dir: load_env(UI_DIR_KEY).ok(),
			import: import_arg()?,
		})
	}
}

/// No arguments runs the server; `import <file>` loads a file of reports.
fn import_arg() -> Result<Option<String>> {
	let args: Vec<String> = env::args().skip(1).collect();
	match args.as_slice() {
		[] => Ok(None),
		[cmd, path] if cmd == "import" => Ok(Some(path.clone())),
		_ => Err(Error::Config("usage: chatter-collector [import <reports.jsonl>]".into())),
	}
}

fn load_env(key: &str) -> Result<String> {
	env::var(key).map_err(|_| Error::Env(key.to_string()))
}
