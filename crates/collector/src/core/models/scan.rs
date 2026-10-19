//! The collector's network scans. nmap's XML output (`-oX -`) comes in as
//! `NmapRun` and is checked into a `Scan`.

use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Duration, NaiveTime, TimeZone};
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

/// When the daily port scan starts, local time (`HH:MM`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortScanAt(NaiveTime);

/// A missed scan starts this long after the collector does, not at once: a
/// deploy restarts it, and the network is busy then anyway.
const CATCH_UP_AFTER_MIN: i64 = 5;

impl PortScanAt {
	pub fn new(raw: &str) -> Result<Self> {
		NaiveTime::parse_from_str(raw.trim(), "%H:%M")
			.map(Self)
			.map_err(|_| Error::Config(format!("port scan time: not HH:MM: {raw}")))
	}

	/// The next start after `now`: today's time if it's still ahead; soon if
	/// today's was missed (no port scan finished since it was due, like a
	/// systemd timer's `Persistent=true`); else tomorrow's. `last_finished`:
	/// the newest port scan's end, unix seconds. The flag: it's a catch-up.
	pub fn next_run<Tz: TimeZone>(
		&self,
		now: DateTime<Tz>,
		last_finished: Option<i64>,
	) -> (DateTime<Tz>, bool) {
		let tz = now.timezone();
		let at = |day: chrono::NaiveDate| {
			tz.from_local_datetime(&day.and_time(self.0)).earliest().unwrap_or_else(|| now.clone())
		};
		let today = at(now.date_naive());
		if now < today {
			(today, false)
		} else if last_finished.is_none_or(|t| t < today.timestamp()) {
			(now + Duration::minutes(CATCH_UP_AFTER_MIN), true)
		} else {
			(at(now.date_naive() + Duration::days(1)), false)
		}
	}
}

impl std::fmt::Display for PortScanAt {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0.format("%H:%M"))
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
	/// `"true"` when `--host-timeout` cut the host's scan short.
	#[serde(rename = "@timedout")]
	pub timed_out: Option<String>,
	pub status: Option<NmapStatus>,
	#[serde(rename = "address", default)]
	pub addresses: Vec<NmapAddress>,
	pub hostnames: Option<NmapHostnames>,
	pub ports: Option<NmapPorts>,
}

#[derive(Debug, Deserialize)]
pub struct NmapPorts {
	#[serde(rename = "port", default)]
	pub ports: Vec<NmapPort>,
}

#[derive(Debug, Deserialize)]
pub struct NmapPort {
	#[serde(rename = "@protocol")]
	pub protocol: String,
	#[serde(rename = "@portid")]
	pub port: u16,
	pub state: NmapStatus,
	pub service: Option<NmapService>,
}

#[derive(Debug, Deserialize)]
pub struct NmapService {
	#[serde(rename = "@name")]
	pub name: Option<String>,
	#[serde(rename = "@product")]
	pub product: Option<String>,
	#[serde(rename = "@version")]
	pub version: Option<String>,
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
	/// A port scan cut short: `ports` is only part of what's open.
	pub timed_out: bool,
	/// Open ports; empty for a sweep, which doesn't look at ports.
	pub ports: Vec<OpenPort>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenPort {
	pub port: u16,
	/// `tcp` or `udp`
	pub protocol: String,
	/// nmap's service name, e.g. `http`
	pub service: Option<String>,
	/// product and version, e.g. `lighttpd 1.4.59`
	pub product: Option<String>,
}

impl From<NmapPort> for OpenPort {
	fn from(p: NmapPort) -> Self {
		let product = [
			p.service.as_ref().and_then(|s| s.product.clone()),
			p.service.as_ref().and_then(|s| s.version.clone()),
		]
		.into_iter()
		.flatten()
		.collect::<Vec<_>>()
		.join(" ");
		Self {
			port: p.port,
			protocol: p.protocol,
			service: p.service.and_then(|s| s.name).filter(|n| !n.is_empty()),
			product: Some(product).filter(|p| !p.is_empty()),
		}
	}
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
				// only "open": UDP's "open|filtered" means no answer at all
				let ports = h.ports.map_or_else(Vec::new, |p| {
					p.ports
						.into_iter()
						.filter(|p| p.state.state == "open")
						.map(OpenPort::from)
						.collect()
				});
				Some(ScannedHost {
					mac: Mac::new(&mac.addr).ok()?,
					ip: ip.addr.parse().ok()?,
					hostname: h.hostnames.and_then(|n| n.names.into_iter().next()).map(|n| n.name),
					vendor: mac.vendor.clone(),
					timed_out: h.timed_out.as_deref() == Some("true"),
					ports,
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

// --- changes between port scans --------------------------------------------
// The infra repo's lan_scan rules, which this replaces.

/// The OS hands out ports from here up (Linux from 32768, iOS from 49152).
pub const DYNAMIC_FROM: u16 = 32768;

/// `-sV` answers that say nothing; a change to or from these isn't news.
const VAGUE: [&str; 3] = ["", "unknown", "tcpwrapped"];

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
	/// Not in any port scan before, with this many open ports.
	NewDevice {
		ports: usize,
	},
	NewPort(OpenPort),
	/// A new high port on a device that rotates them (once one of its high
	/// ports has closed: iOS, Home Assistant, a TV): not an alert.
	NewRotatingPort(OpenPort),
	ClosedPort(OpenPort),
	/// The same port, another product or version (a firmware update?).
	ChangedPort {
		port: OpenPort,
		before: String,
	},
}

impl Change {
	pub fn is_alert(&self) -> bool {
		matches!(self, Change::NewDevice { .. } | Change::NewPort(_))
	}

	pub fn kind(&self) -> &'static str {
		match self {
			Change::NewDevice {
				..
			} => "new_device",
			Change::NewPort(_) => "new_port",
			Change::NewRotatingPort(_) => "new_rotating_port",
			Change::ClosedPort(_) => "closed_port",
			Change::ChangedPort {
				..
			} => "changed_port",
		}
	}

	pub fn port(&self) -> Option<&OpenPort> {
		match self {
			Change::NewDevice {
				..
			} => None,
			Change::NewPort(p) | Change::NewRotatingPort(p) | Change::ClosedPort(p) => Some(p),
			Change::ChangedPort {
				port,
				..
			} => Some(port),
		}
	}
}

fn key(p: &OpenPort) -> (&str, u16) {
	(p.protocol.as_str(), p.port)
}

/// A device's changes since its port scan before. `previous`: its open ports
/// then, `None` if no port scan has seen it. `volatile_high`: one of its high
/// ports has closed before; a high port closing now makes it so too.
pub fn compare(
	previous: Option<&[OpenPort]>,
	current: &[OpenPort],
	volatile_high: bool,
) -> Vec<Change> {
	let Some(previous) = previous else {
		return vec![Change::NewDevice {
			ports: current.len(),
		}];
	};
	let had = |p: &OpenPort| previous.iter().any(|q| key(q) == key(p));
	let has = |p: &OpenPort| current.iter().any(|q| key(q) == key(p));
	let mut gone: Vec<&OpenPort> = previous.iter().filter(|p| !has(p)).collect();
	let mut new: Vec<&OpenPort> = current.iter().filter(|p| !had(p)).collect();
	gone.sort_by_key(|p| key(p));
	new.sort_by_key(|p| key(p));
	let volatile = volatile_high || gone.iter().any(|p| p.port >= DYNAMIC_FROM);

	let mut changes: Vec<Change> = new
		.into_iter()
		.map(|p| {
			if volatile && p.port >= DYNAMIC_FROM {
				Change::NewRotatingPort(p.clone())
			} else {
				Change::NewPort(p.clone())
			}
		})
		.collect();
	changes.extend(gone.into_iter().map(|p| Change::ClosedPort(p.clone())));
	let vague = |p: &Option<String>| VAGUE.contains(&p.as_deref().unwrap_or(""));
	let mut kept: Vec<&OpenPort> = current.iter().filter(|p| had(p)).collect();
	kept.sort_by_key(|p| key(p));
	for p in kept {
		let Some(old) = previous.iter().find(|q| key(q) == key(p)) else {
			continue;
		};
		if old.product != p.product && !vague(&old.product) && !vague(&p.product) {
			changes.push(Change::ChangedPort {
				port: p.clone(),
				before: old.product.clone().unwrap_or_default(),
			});
		}
	}
	changes
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

/// The shape of a port scan's XML (`-sS -sU -sV`); made-up addresses.
#[cfg(test)]
pub const PORTS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<nmaprun scanner="nmap" args="nmap -sS -sU -sV" start="1700050000" version="7.93" xmloutputversion="1.05">
<scaninfo type="syn" protocol="tcp" numservices="65535" services="1-65535"/>
<host starttime="1700050001" endtime="1700050400"><status state="up" reason="arp-response" reason_ttl="0"/>
<address addr="192.0.2.10" addrtype="ipv4"/>
<address addr="AA:BB:CC:00:00:01" addrtype="mac" vendor="Espressif"/>
<hostnames><hostname name="ESP_00001.lan" type="PTR"/></hostnames>
<ports><extraports state="closed" count="65532"><extrareasons reason="reset" count="65532"/></extraports>
<port protocol="tcp" portid="80"><state state="open" reason="syn-ack" reason_ttl="64"/><service name="http" product="lighttpd" version="1.4.59" method="probed" conf="10"/></port>
<extraports state="open|filtered" count="4"/>
<port protocol="udp" portid="5353"><state state="open" reason="udp-response" reason_ttl="255"/><service name="mdns" method="table" conf="3"/></port>
<port protocol="udp" portid="1900"><state state="open|filtered" reason="no-response" reason_ttl="0"/><service name="upnp" method="table" conf="3"/></port>
</ports>
</host>
<host starttime="1700050001" endtime="1700051800" timedout="true"><status state="up" reason="arp-response" reason_ttl="0"/>
<address addr="192.0.2.11" addrtype="ipv4"/>
<address addr="AA:BB:CC:00:00:02" addrtype="mac"/>
<hostnames></hostnames>
</host>
<runstats><finished time="1700051900" timestr="x" summary="x" elapsed="1900" exit="success"/><hosts up="2" down="254" total="256"/></runstats>
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
	fn the_port_scan_runs_daily_and_catches_up_once() {
		use chrono::Utc;
		let at = PortScanAt::new("18:00").unwrap();
		let t = |h, m| Utc.with_ymd_and_hms(2026, 10, 18, h, m, 0).unwrap();
		// before the time: today's
		assert_eq!(at.next_run(t(9, 0), None), (t(18, 0), false));
		// after it, today's done: tomorrow's
		let done = Some(t(18, 40).timestamp());
		assert_eq!(at.next_run(t(20, 0), done), (t(18, 0) + Duration::days(1), false));
		// after it, today's missed (the last one was yesterday): soon
		let yesterday = Some((t(18, 40) - Duration::days(1)).timestamp());
		assert_eq!(at.next_run(t(20, 0), yesterday), (t(20, 5), true));
		assert_eq!(at.next_run(t(20, 0), None), (t(20, 5), true));
		assert!(PortScanAt::new("25:00").is_err());
		assert!(PortScanAt::new("off").is_err());
	}

	fn port(port: u16, product: Option<&str>) -> OpenPort {
		OpenPort {
			port,
			protocol: "tcp".into(),
			service: None,
			product: product.map(str::to_string),
		}
	}

	#[test]
	fn changes_follow_lan_scans_rules() {
		// never seen: a new device, an alert
		let c = compare(None, &[port(80, None)], false);
		assert_eq!(
			c,
			[Change::NewDevice {
				ports: 1
			}]
		);
		assert!(c[0].is_alert());

		// a new low port: an alert; a closed one: information
		let c = compare(Some(&[port(80, None)]), &[port(443, None)], false);
		assert_eq!(c, [Change::NewPort(port(443, None)), Change::ClosedPort(port(80, None))]);
		assert_eq!(c.iter().filter(|c| c.is_alert()).count(), 1);

		// a high port closing makes the device rotate: the new high port is information
		let c = compare(Some(&[port(40000, None)]), &[port(40001, None)], false);
		assert_eq!(c[0], Change::NewRotatingPort(port(40001, None)));
		assert!(!c[0].is_alert());
		// and it stays that way next time, even if nothing closes
		let c = compare(Some(&[]), &[port(50000, None)], true);
		assert_eq!(c, [Change::NewRotatingPort(port(50000, None))]);
		// a device that doesn't rotate: a new high port is an alert
		let c = compare(Some(&[]), &[port(50000, None)], false);
		assert_eq!(c, [Change::NewPort(port(50000, None))]);

		// a new version is news; to or from a vague answer isn't
		let c = compare(Some(&[port(80, Some("x 1.0"))]), &[port(80, Some("x 1.1"))], false);
		assert_eq!(
			c,
			[Change::ChangedPort {
				port: port(80, Some("x 1.1")),
				before: "x 1.0".into()
			}]
		);
		assert!(compare(Some(&[port(80, Some("x 1.0"))]), &[port(80, None)], false).is_empty());
		assert!(
			compare(Some(&[port(80, Some("tcpwrapped"))]), &[port(80, Some("x"))], false)
				.is_empty()
		);
	}

	#[test]
	fn reads_a_port_scan() {
		let scan = Scan::from(NmapRun::from_xml(PORTS_XML).unwrap());
		assert_eq!(scan.finished_at, 1_700_051_900);
		let h = &scan.hosts[0];
		assert!(!h.timed_out);
		let ports: Vec<(u16, &str)> =
			h.ports.iter().map(|p| (p.port, p.protocol.as_str())).collect();
		assert_eq!(ports, [(80, "tcp"), (5353, "udp")], "open|filtered isn't open");
		assert_eq!(h.ports[0].service.as_deref(), Some("http"));
		assert_eq!(h.ports[0].product.as_deref(), Some("lighttpd 1.4.59"));
		assert_eq!(h.ports[1].product, None);
		assert!(scan.hosts[1].timed_out);
		assert!(scan.hosts[1].ports.is_empty());
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
