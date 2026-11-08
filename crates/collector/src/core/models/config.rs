use std::path::{Path, PathBuf};

use crate::{
	Error, Result,
	core::models::scan::{PortScanAt, ScanTargets},
};

/// Which kind of DNS server keeps the query log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DnsServer {
	PiHole,
}

/// A DNS server to read the query log of.
#[derive(Clone, PartialEq)]
pub struct DnsSource {
	pub server: DnsServer,
	pub url: String,
	pub password: Option<String>,
}

impl DnsSource {
	/// `pihole+http://…`. No space between them: `.env` can't hold one unquoted.
	pub fn new(raw: &str, password: Option<String>) -> Result<Self> {
		let err = || Error::Config(format!("{raw:?} is not `pihole+http://…`"));
		let (server, url) = raw.trim().split_once('+').ok_or_else(err)?;
		if !(url.starts_with("http://") || url.starts_with("https://")) {
			return Err(err());
		}
		let server = match server {
			"pihole" => DnsServer::PiHole,
			_ => return Err(err()),
		};
		Ok(Self {
			server,
			url: url.to_string(),
			password: password.filter(|p| !p.is_empty()),
		})
	}

	/// The DNS server's own query log, filtered to one address. Only the
	/// address: before the device had it, it may have been another's.
	pub fn query_log(&self, ip: &str) -> String {
		let base = self.url.trim_end_matches('/');
		match self.server {
			DnsServer::PiHole => format!("{base}/admin/queries?client_ip={ip}"),
		}
	}
}

/// Without the password: `Config` can end up in a log.
impl std::fmt::Debug for DnsSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DnsSource")
			.field("server", &self.server)
			.field("url", &self.url)
			.finish_non_exhaustive()
	}
}

/// What the binary was started to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	/// The server, with the scans in the background.
	Serve,
	/// `port-scan`: one port scan now, then exit.
	PortScan,
}

#[derive(Debug, Clone)]
pub struct Config {
	pub bind: String,
	pub db_path: String,
	/// Development: serve the page from this directory, read on every request.
	pub ui_dir: Option<String>,
	/// What the host sweep covers; `None`: the /24 of this machine's network.
	pub scan_targets: Option<ScanTargets>,
	pub sweep_every_min: u64,
	/// The daily port scan's time; `None`: no port scans.
	pub port_scan_at: Option<PortScanAt>,
	/// Where the names devices look up come from; `None`: nowhere, and no DNS
	/// section anywhere.
	pub dns: Option<DnsSource>,
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_dns_source_names_its_server_and_address() {
		let source =
			DnsSource::new("pihole+http://192.0.2.5:8081", Some("made-up".into())).unwrap();
		assert_eq!(source.server, DnsServer::PiHole);
		assert_eq!(source.url, "http://192.0.2.5:8081");
		assert_eq!(source.password.as_deref(), Some("made-up"));

		let source = DnsSource::new("pihole+https://192.0.2.5", Some(String::new())).unwrap();
		assert_eq!(source.url, "https://192.0.2.5");
		assert_eq!(source.password, None);
	}

	#[test]
	fn a_dns_source_that_cant_be_read_is_an_error() {
		for raw in
			["http://192.0.2.5", "pihole http://192.0.2.5", "pihole+192.0.2.5", "other+http://x"]
		{
			assert!(DnsSource::new(raw, None).is_err(), "{raw}");
		}
	}

	#[test]
	fn the_query_log_link_filters_to_the_address() {
		for raw in ["pihole+http://192.0.2.5:8081", "pihole+http://192.0.2.5:8081/"] {
			let source = DnsSource::new(raw, None).unwrap();
			assert_eq!(
				source.query_log("192.0.2.10"),
				"http://192.0.2.5:8081/admin/queries?client_ip=192.0.2.10"
			);
		}
	}

	#[test]
	fn the_password_stays_out_of_the_debug_output() {
		let source = DnsSource::new("pihole+http://192.0.2.5", Some("made-up".into())).unwrap();
		assert!(!format!("{source:?}").contains("made-up"));
	}
}
