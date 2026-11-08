//! What the monitoring page shows live: every device ever heard with its last
//! few minutes, and the sensors. Read-only, out as JSON. The page leaves out
//! the devices it hasn't seen for a week (`ui/gone.js`). The history is the
//! timeline (`timeline.rs`).

use serde::Serialize;

use crate::core::models::{
	baseline::{ScoreScale, UsualTraffic},
	dns::TopDomains,
};

/// Minutes in the page's replay loop.
pub const LOOP_MINUTES: usize = 5;
/// The devices' strongest signal and the sensors' heartbeats cover this much
/// before the newest report.
pub const RECENT_S: i64 = 24 * 3600;

#[derive(Debug, Serialize)]
pub struct Overview {
	/// The newest report's time. Unix seconds, like every time here.
	pub latest: i64,
	/// The times of the loop's minutes (the newest reports), oldest first.
	pub minutes: Vec<i64>,
	/// The hour of the day of each of those minutes, in the collector's time
	/// zone: which of a device's baselines it's scored against.
	pub hours: Vec<usize>,
	/// How a device's traffic becomes its score.
	pub score: ScoreScale,
	pub sensors: Vec<SensorStatus>,
	pub devices: Vec<DeviceOverview>,
	/// `None` without a DNS server: the page shows no DNS section.
	pub dns: Option<DnsOverview>,
}

/// What the DNS server's log says about the whole network, over the days shown.
#[derive(Debug, Serialize)]
pub struct DnsOverview {
	/// The router makes most of the queries, so few can be credited to a device.
	pub router_forwards: bool,
}

/// A device's queries in the DNS server's log, over the days shown.
#[derive(Debug, Default, Serialize)]
pub struct DeviceDns {
	#[serde(flatten)]
	pub domains: TopDomains,
	/// The DNS server's query log for its address; `None` without one.
	pub query_log: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SensorStatus {
	pub name: String,
	/// From its newest report.
	pub last_seen: i64,
	pub seq: i64,
	pub uptime_s: i64,
	pub channel: i64,
	pub dropped: i64,
	pub overflow: i64,
	/// The watched networks it knows a BSSID for.
	pub nets_known: Vec<u8>,
	/// Devices in its newest report.
	pub devices: i64,
	/// In the last 24 hours: reports that arrived, and how many went missing
	/// (gaps in `seq`) and restarts (`seq` going back).
	pub reports: i64,
	pub missing: i64,
	pub restarts: i64,
	/// When each of those reports arrived, oldest first: its heartbeat.
	pub beats: Vec<i64>,
}

/// What a run of sequence numbers says: how many reports went missing in
/// between, and how many times it started again (the sensor rebooted).
pub fn seq_stats(seqs: &[i64]) -> (i64, i64) {
	let (mut missing, mut restarts) = (0, 0);
	for pair in seqs.windows(2) {
		let (prev, seq) = (pair[0], pair[1]);
		if seq > prev + 1 {
			missing += seq - prev - 1;
		} else if seq <= prev {
			restarts += 1;
		}
	}
	(missing, restarts)
}

#[derive(Debug, Serialize)]
pub struct PortOverview {
	pub port: i64,
	pub protocol: String,
	pub service: Option<String>,
	pub product: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceOverview {
	pub mac: String,
	pub first_seen: i64,
	pub last_seen: i64,
	/// The strongest signal in the last 24 hours, `None` if it sent nothing the
	/// sensor heard.
	pub rssi_max: Option<i64>,
	/// When the newest scan that found it finished; `None` if none has. A
	/// device the sensor can't hear (on 5 GHz, say) still answers these.
	pub swept_at: Option<i64>,
	/// From that scan, like `hostname` and `vendor`.
	pub ip: Option<String>,
	pub hostname: Option<String>,
	pub vendor: Option<String>,
	/// The name the owner gave it; `None` if none.
	pub name: Option<String>,
	/// Open ports from its newest port scan that wasn't cut short; `None`
	/// before one has finished for it.
	pub ports: Option<Vec<PortOverview>>,
	/// When that scan finished.
	pub port_scanned_at: Option<i64>,
	/// `[tx, rx]` bytes per loop minute, like `Overview::minutes`.
	pub minutes: Vec<[i64; 2]>,
	/// Its baselines; `None` before the first are worked out.
	pub usual: Option<UsualTraffic>,
	/// `None` without a DNS server.
	pub dns: Option<DeviceDns>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sequence_gaps_and_restarts() {
		assert_eq!(seq_stats(&[]), (0, 0));
		assert_eq!(seq_stats(&[4, 5, 6]), (0, 0));
		assert_eq!(seq_stats(&[4, 5, 8, 9]), (2, 0));
		// a reboot starts at 0 again; the reports before it aren't missing
		assert_eq!(seq_stats(&[40, 41, 0, 1, 3]), (1, 1));
	}
}
