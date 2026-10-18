//! The collector's network scans. nmap's XML output (`-oX -`) comes in as
//! `NmapRun` and is checked into a `Scan`.

use std::net::{IpAddr, Ipv4Addr};

use serde::Deserialize;

use crate::{Error, Result, core::models::report::Mac};

/// What to scan: IP addresses and CIDR ranges, checked before nmap sees
/// them, so a value can never turn into an nmap option.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanTargets(Vec<String>);

impl ScanTargets {
	/// Space- or comma-separated, e.g. `192.0.2.0/24 198.51.100.7`.
	pub fn new(raw: &str) -> Result<Self> {
		let targets: Vec<String> =
			raw.split([' ', ',']).filter(|t| !t.is_empty()).map(str::to_string).collect();
		if targets.is_empty() {
			return Err(Error::Config("scan targets: none given".into()));
		}
		for t in &targets {
			let (ip, prefix) =
				t.split_once('/').map_or((t.as_str(), None), |(ip, p)| (ip, Some(p)));
			let ip: IpAddr =
				ip.parse().map_err(|_| Error::Config(format!("scan target: not an IP: {t}")))?;
			let max = if ip.is_ipv4() {
				32
			} else {
				128
			};
			if let Some(p) = prefix
				&& !p.parse::<u8>().is_ok_and(|p| p <= max)
			{
				return Err(Error::Config(format!("scan target: bad prefix: {t}")));
			}
		}
		Ok(Self(targets))
	}

	/// The /24 around a LAN address: what home networks use, and at most 256
	/// addresses to sweep, whatever the network's real size.
	pub fn around(ip: Ipv4Addr) -> Self {
		let [a, b, c, _] = ip.octets();
		Self(vec![format!("{a}.{b}.{c}.0/24")])
	}

	pub fn as_args(&self) -> &[String] {
		&self.0
	}
}

impl std::fmt::Display for ScanTargets {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0.join(" "))
	}
}

// --- nmap's XML ------------------------------------------------------------
// Only the parts the collector uses; everything else in the file is skipped.

#[derive(Debug, Deserialize)]
pub struct NmapRun {
	#[serde(rename = "@start")]
	pub start: i64,
	#[serde(rename = "host", default)]
	pub hosts: Vec<NmapHost>,
	pub runstats: Option<NmapRunStats>,
}

#[derive(Debug, Deserialize)]
pub struct NmapHost {
	pub status: Option<NmapStatus>,
	#[serde(rename = "address", default)]
	pub addresses: Vec<NmapAddress>,
	pub hostnames: Option<NmapHostnames>,
}

#[derive(Debug, Deserialize)]
pub struct NmapStatus {
	#[serde(rename = "@state")]
	pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct NmapAddress {
	#[serde(rename = "@addr")]
	pub addr: String,
	#[serde(rename = "@addrtype")]
	pub addrtype: String,
	#[serde(rename = "@vendor")]
	pub vendor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NmapHostnames {
	#[serde(rename = "hostname", default)]
	pub names: Vec<NmapHostname>,
}

#[derive(Debug, Deserialize)]
pub struct NmapHostname {
	#[serde(rename = "@name")]
	pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NmapRunStats {
	pub finished: NmapFinished,
}

#[derive(Debug, Deserialize)]
pub struct NmapFinished {
	#[serde(rename = "@time")]
	pub time: i64,
}

impl NmapRun {
	pub fn from_xml(xml: &str) -> Result<Self> {
		quick_xml::de::from_str(xml).map_err(|e| Error::Scan(format!("nmap XML: {e}")))
	}
}

// --- the checked scan ------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
	pub started_at: i64,
	pub finished_at: i64,
	pub hosts: Vec<ScannedHost>,
}

/// A host that was up and answered from this network segment, so with a MAC.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedHost {
	pub mac: Mac,
	pub ip: IpAddr,
	pub hostname: Option<String>,
	pub vendor: Option<String>,
}

impl From<NmapRun> for Scan {
	/// Hosts without a MAC (the scanning machine itself, anything behind a
	/// router) can't be joined to the sensors' data and are left out.
	fn from(run: NmapRun) -> Self {
		let hosts = run
			.hosts
			.into_iter()
			.filter(|h| h.status.as_ref().is_none_or(|s| s.state == "up"))
			.filter_map(|h| {
				let mac = h.addresses.iter().find(|a| a.addrtype == "mac")?;
				let ip =
					h.addresses.iter().find(|a| a.addrtype == "ipv4" || a.addrtype == "ipv6")?;
				Some(ScannedHost {
					mac: Mac::new(&mac.addr).ok()?,
					ip: ip.addr.parse().ok()?,
					hostname: h.hostnames.and_then(|n| n.names.into_iter().next()).map(|n| n.name),
					vendor: mac.vendor.clone(),
				})
			})
			.collect();
		Self {
			started_at: run.start,
			finished_at: run.runstats.map_or(run.start, |r| r.finished.time),
			hosts,
		}
	}
}

/// The shape of `nmap -sn -oX -`; made-up addresses and names.
#[cfg(test)]
pub const SWEEP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE nmaprun>
<nmaprun scanner="nmap" args="nmap -sn -oX - 192.0.2.0/24" start="1700000000" version="7.93" xmloutputversion="1.05">
<verbose level="0"/>
<debugging level="0"/>
<hosthint><status state="up" reason="unknown-response" reason_ttl="0"/>
<address addr="192.0.2.10" addrtype="ipv4"/>
<address addr="AA:BB:CC:00:00:01" addrtype="mac" vendor="Espressif"/>
<hostnames></hostnames></hosthint>
<host><status state="up" reason="arp-response" reason_ttl="0"/>
<address addr="192.0.2.10" addrtype="ipv4"/>
<address addr="AA:BB:CC:00:00:01" addrtype="mac" vendor="Espressif"/>
<hostnames><hostname name="ESP_00001.lan" type="PTR"/></hostnames>
<times srtt="3000" rttvar="5000" to="100000"/>
</host>
<hosthint><status state="up" reason="unknown-response" reason_ttl="0"/>
<address addr="192.0.2.11" addrtype="ipv4"/>
<hostnames></hostnames></hosthint>
<host><status state="up" reason="arp-response" reason_ttl="0"/>
<address addr="192.0.2.11" addrtype="ipv4"/>
<address addr="AA:BB:CC:00:00:02" addrtype="mac"/>
<hostnames></hostnames>
</host>
<host><status state="up" reason="localhost-response" reason_ttl="0"/>
<address addr="192.0.2.2" addrtype="ipv4"/>
<hostnames><hostname name="scanner.lan" type="PTR"/></hostnames>
</host>
<runstats><finished time="1700000004" timestr="x" summary="x" elapsed="4.10" exit="success"/><hosts up="3" down="253" total="256"/>
</runstats>
</nmaprun>
"#;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_a_sweep() {
		let scan = Scan::from(NmapRun::from_xml(SWEEP_XML).unwrap());
		assert_eq!((scan.started_at, scan.finished_at), (1_700_000_000, 1_700_000_004));
		assert_eq!(scan.hosts.len(), 2, "the scanner itself has no MAC");
		let h = &scan.hosts[0];
		assert_eq!(h.mac.as_str(), "aa:bb:cc:00:00:01");
		assert_eq!(h.ip.to_string(), "192.0.2.10");
		assert_eq!(
			(h.hostname.as_deref(), h.vendor.as_deref()),
			(Some("ESP_00001.lan"), Some("Espressif"))
		);
		assert_eq!(
			(scan.hosts[1].hostname.as_deref(), scan.hosts[1].vendor.as_deref()),
			(None, None)
		);
	}

	#[test]
	fn targets_are_ips_or_ranges_only() {
		assert_eq!(
			ScanTargets::new("192.0.2.0/24, 198.51.100.7").unwrap().as_args(),
			["192.0.2.0/24", "198.51.100.7"]
		);
		for bad in ["", "-oN /etc/x", "192.0.2.0/33", "example.com", "192.0.2.0/24 --script=x"] {
			assert!(ScanTargets::new(bad).is_err(), "{bad:?}");
		}
	}

	#[test]
	fn around_an_address_is_its_slash_24() {
		let t = ScanTargets::around(Ipv4Addr::new(198, 51, 100, 23));
		assert_eq!(t.as_args(), ["198.51.100.0/24"]);
		assert_eq!(ScanTargets::new(&t.to_string()).unwrap(), t);
	}
}
