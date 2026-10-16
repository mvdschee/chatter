use std::{env, fs, path::Path};

// Same .env loading as firmware/build.rs, with the test client's keys.
// Each comes from the environment, or else from the repo's `.env`.
const ENV_KEYS: [&str; 3] = ["CHATTER_WIFI_SSID", "CHATTER_WIFI_PASSWORD", "CHATTER_TEST_ADDR"];

fn main() {
	load_env();

	println!("cargo:rustc-link-arg=-Tdefmt.x");
	// linkall.x must be the last linker script
	println!("cargo:rustc-link-arg=-Tlinkall.x");
}

fn load_env() {
	let path = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../.env");
	println!("cargo:rerun-if-changed={}", path.display());
	let file = fs::read_to_string(&path).unwrap_or_default();

	for key in ENV_KEYS {
		println!("cargo:rerun-if-env-changed={key}");
		let value = env::var(key).ok().or_else(|| {
			file.lines().find_map(|line| {
				let value = line.strip_prefix(key)?.strip_prefix('=')?;
				Some(value.trim_end_matches('\r').to_string())
			})
		});
		match value {
			Some(value) if !value.is_empty() => println!("cargo:rustc-env={key}={value}"),
			_ => panic!("{key} is not set: fill it in .env at the repo root"),
		}
	}
}
