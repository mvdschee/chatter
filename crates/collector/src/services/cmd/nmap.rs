use std::{net::Ipv4Addr, process::Stdio, time::Duration};

use tokio::{process::Command, time::timeout};

use crate::{Error, Result, core::models::scan::ScanTargets};

/// A sweep of a home network takes seconds; past this, something hangs.
const SWEEP_TIMEOUT: Duration = Duration::from_secs(300);

/// Host discovery (`nmap -sn`), XML on stdout. Needs raw sockets (root, or
/// `NET_RAW` and `NET_ADMIN`) for the ARP requests that give MAC addresses.
/// `dns`: the server for the reverse lookups that give hostnames.
pub async fn sweep(targets: &ScanTargets, dns: Option<Ipv4Addr>) -> Result<String> {
	let mut cmd = Command::new("nmap");
	cmd.args(["-sn", "-oX", "-"]);
	if let Some(dns) = dns {
		cmd.arg("--dns-servers").arg(dns.to_string());
	}
	let run =
		cmd.arg("--").args(targets.as_args()).stdin(Stdio::null()).kill_on_drop(true).output();
	let output = timeout(SWEEP_TIMEOUT, run)
		.await
		.map_err(|_| Error::Scan(format!("nmap took over {} s", SWEEP_TIMEOUT.as_secs())))?
		.map_err(|e| Error::Scan(format!("can't run nmap: {e}")))?;
	if !output.status.success() {
		return Err(Error::Scan(format!(
			"nmap failed ({}): {}",
			output.status,
			String::from_utf8_lossy(&output.stderr).trim()
		)));
	}
	String::from_utf8(output.stdout).map_err(|_| Error::Scan("nmap output isn't UTF-8".into()))
}

/// The default route's gateway, from nmap's own view of the routes
/// (`nmap --iflist`, the same on Linux and macOS). `None` without one.
pub async fn default_gateway() -> Result<Option<Ipv4Addr>> {
	let output = Command::new("nmap")
		.arg("--iflist")
		.stdin(Stdio::null())
		.kill_on_drop(true)
		.output()
		.await
		.map_err(|e| Error::Scan(format!("can't run nmap: {e}")))?;
	Ok(gateway_from_iflist(&String::from_utf8_lossy(&output.stdout)))
}

/// The first IPv4 default route with a gateway in the ROUTES table:
/// `0.0.0.0/0  en0  0  192.0.2.1`. A VPN's default route often has none.
pub fn gateway_from_iflist(text: &str) -> Option<Ipv4Addr> {
	text.split("ROUTES").nth(1)?.lines().find_map(|line| {
		let mut cols = line.split_whitespace();
		if cols.next()? != "0.0.0.0/0" {
			return None;
		}
		cols.nth(2)?.parse().ok()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_the_default_gateway() {
		let iflist = "************************INTERFACES************************
DEV    (SHORT)  IP/MASK                      TYPE     UP MTU   MAC
en0    (en0)    192.0.2.23/24                ethernet up 1500  AA:BB:CC:00:00:09

**************************ROUTES**************************
DST/MASK                         DEV    METRIC GATEWAY
192.0.2.0/24                     en0    0
0.0.0.0/0                        utun10 0
0.0.0.0/0                        en0    0      192.0.2.1
::/0                             en0    0      fe80::1
";
		assert_eq!(gateway_from_iflist(iflist), Some(Ipv4Addr::new(192, 0, 2, 1)));
		assert_eq!(gateway_from_iflist("no routes here"), None);
	}
}
