use std::env;

use crate::{
	Error, Result,
	core::models::{
		config::{Command, Config},
		scan::{PortScanAt, ScanTargets},
	},
};

/// Where to listen.
const BIND_KEY: &str = "CHATTER_BIND";
const BIND_DEFAULT: &str = "127.0.0.1:8080";

/// The SQLite file; created when missing.
const DB_KEY: &str = "CHATTER_DB";
const DB_DEFAULT: &str = "chatter.db";

/// Development: the directory with the page's files (`crates/collector/ui`),
/// read on every request. Unset: the files built into the binary.
const UI_DIR_KEY: &str = "CHATTER_UI_DIR";

/// The networks to scan with nmap, as IP addresses or CIDR ranges. Unset: the
/// /24 of the network this machine is on, looked up before every sweep.
const SCAN_TARGETS_KEY: &str = "CHATTER_SCAN_TARGETS";

/// Minutes between host sweeps.
const SWEEP_EVERY_MIN_KEY: &str = "CHATTER_SWEEP_MINUTES";
const SWEEP_EVERY_MIN_DEFAULT: u64 = 10;

/// The daily port scan's local time, `HH:MM`, or `off`.
const PORT_SCAN_AT_KEY: &str = "CHATTER_PORT_SCAN_AT";
const PORT_SCAN_AT_DEFAULT: &str = "18:00";

impl Config {
	pub fn init() -> Result<Self> {
		// local runs: the repo's .env (cargo runs from the repo root); a
		// container has no .env and sets the same keys in its environment.
		// Keys already set win.
		let _ = dotenvy::dotenv();
		Ok(Self {
			bind: load_env(BIND_KEY).unwrap_or_else(|_| BIND_DEFAULT.to_string()),
			db_path: load_env(DB_KEY).unwrap_or_else(|_| DB_DEFAULT.to_string()),
			ui_dir: load_env(UI_DIR_KEY).ok(),
			scan_targets: load_env(SCAN_TARGETS_KEY)
				.ok()
				.filter(|t| !t.trim().is_empty())
				.map(|t| ScanTargets::new(&t))
				.transpose()?,
			sweep_every_min: match load_env(SWEEP_EVERY_MIN_KEY) {
				Ok(min) => min.parse().ok().filter(|&m| m > 0).ok_or_else(|| {
					Error::Config(format!("{SWEEP_EVERY_MIN_KEY}: not a number of minutes: {min}"))
				})?,
				Err(_) => SWEEP_EVERY_MIN_DEFAULT,
			},
			port_scan_at: match load_env(PORT_SCAN_AT_KEY)
				.unwrap_or_else(|_| PORT_SCAN_AT_DEFAULT.to_string())
				.trim()
			{
				"off" => None,
				at => Some(PortScanAt::new(at)?),
			},
			command: command()?,
		})
	}
}

/// No arguments runs the server; `port-scan` runs one port scan now.
fn command() -> Result<Command> {
	let args: Vec<String> = env::args().skip(1).collect();
	match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
		[] => Ok(Command::Serve),
		["port-scan"] => Ok(Command::PortScan),
		_ => Err(Error::Config("usage: chatter-collector [port-scan]".into())),
	}
}

fn load_env(key: &str) -> Result<String> {
	env::var(key).map_err(|_| Error::Env(key.to_string()))
}
