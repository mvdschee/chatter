use std::{env, fs, path::Path};

// Build-time settings. Each comes from the environment, or else from the
// repo's git-ignored `.env`. They end up as `env!` constants in config.rs.
const ENV_KEYS: [&str; 4] =
	["CHATTER_WIFI_SSID", "CHATTER_WIFI_PASSWORD", "CHATTER_WATCH_SSIDS", "CHATTER_COLLECTOR_ADDR"];
/// Optional, with its default: a second board needs its own name.
const SENSOR_NAME_KEY: &str = "CHATTER_SENSOR_NAME";
const SENSOR_NAME_DEFAULT: &str = "c6-1";

fn main() {
	load_env();

	println!("cargo:rustc-link-arg-tests=-Tembedded-test.x");
	println!("cargo:rustc-link-arg=-Tdefmt.x");
	// linkall.x must be the last linker script
	println!("cargo:rustc-link-arg=-Tlinkall.x");
}

fn load_env() {
	let path = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../.env");
	println!("cargo:rerun-if-changed={}", path.display());
	let file = fs::read_to_string(&path).unwrap_or_default();

	for key in ENV_KEYS {
		match lookup(key, &file) {
			Some(value) => println!("cargo:rustc-env={key}={value}"),
			None => panic!("{key} is not set: fill it in .env at the repo root"),
		}
	}

	// goes into the report's JSON and the `GET /mode` path as is; the
	// collector's `SensorName` takes the same
	let name = lookup(SENSOR_NAME_KEY, &file).unwrap_or_else(|| SENSOR_NAME_DEFAULT.into());
	let ok = (1..=32).contains(&name.len())
		&& name
			.bytes()
			.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
	if !ok {
		panic!("{SENSOR_NAME_KEY}: 1 to 32 of a-z, 0-9, - and _: {name}");
	}
	println!("cargo:rustc-env={SENSOR_NAME_KEY}={name}");
}

/// A key's value from the environment, or else from `.env`; empty is unset.
fn lookup(key: &str, file: &str) -> Option<String> {
	println!("cargo:rerun-if-env-changed={key}");
	env::var(key)
		.ok()
		.or_else(|| {
			file.lines().find_map(|line| {
				let value = line.strip_prefix(key)?.strip_prefix('=')?;
				Some(value.trim_end_matches('\r').to_string())
			})
		})
		.filter(|value| !value.is_empty())
}
