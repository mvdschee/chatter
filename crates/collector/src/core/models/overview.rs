//! What the monitoring page shows: every device with its last few minutes and
//! its day in buckets, and the sensors. Read-only, out as JSON.

use serde::Serialize;

/// Minutes in the page's replay loop.
pub const LOOP_MINUTES: usize = 5;
/// The day view: 288 buckets of 5 minutes.
pub const BUCKET_S: i64 = 300;
pub const BUCKETS: usize = 288;

#[derive(Debug, Serialize)]
pub struct Overview {
	/// The newest report's time; the day ends there. Unix seconds, like every
	/// time here.
	pub latest: i64,
	/// The times of the loop's minutes (the newest reports), oldest first.
	pub minutes: Vec<i64>,
	/// The first bucket's start.
	pub day_start: i64,
	pub bucket_s: i64,
	pub sensors: Vec<SensorStatus>,
	pub devices: Vec<DeviceOverview>,
	/// The port scans that finished in the day, oldest first.
	pub port_scans: Vec<PortScanSummary>,
	/// What those port scans found changed, in order.
	pub changes: Vec<ChangeOverview>,
}

#[derive(Debug, Serialize)]
pub struct ChangeOverview {
	/// When the scan that found it finished.
	pub at: i64,
	pub mac: String,
	/// The host as that scan saw it.
	pub ip: Option<String>,
	pub hostname: Option<String>,
	pub vendor: Option<String>,
	/// `new_device`, `new_port`, `new_rotating_port`, `closed_port`,
	/// `changed_port`
	pub kind: String,
	pub alert: bool,
	pub port: Option<i64>,
	pub protocol: Option<String>,
	pub service: Option<String>,
	pub product: Option<String>,
	/// `changed_port`: the product before.
	pub before: Option<String>,
	/// Its open ports in that scan.
	pub ports: i64,
}

#[derive(Debug, Serialize)]
pub struct PortScanSummary {
	pub started_at: i64,
	pub finished_at: i64,
	pub hosts: i64,
	pub open_ports: i64,
	/// Hosts the per-host time limit cut short: their ports aren't counted.
	pub timed_out: i64,
	/// Changes since the scan before that are worth an alert.
	pub alerts: i64,
}

#[derive(Debug, Serialize)]
pub struct PortOverview {
	pub port: i64,
	pub protocol: String,
	pub service: Option<String>,
	pub product: Option<String>,
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
	/// In the day: reports that arrived, and how many went missing (gaps in
	/// `seq`) and restarts (`seq` going back).
	pub reports: i64,
	pub missing: i64,
	pub restarts: i64,
	/// Reports per bucket of the day: its heartbeat.
	pub heartbeat: Vec<i64>,
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
pub struct DeviceOverview {
	pub mac: String,
	pub first_seen: i64,
	pub last_seen: i64,
	/// The strongest signal in the day, `None` if it sent nothing the sensor heard.
	pub rssi_max: Option<i64>,
	/// From the newest host sweep that found it; `None` if none has.
	pub ip: Option<String>,
	pub hostname: Option<String>,
	pub vendor: Option<String>,
	/// Open ports from its newest port scan that wasn't cut short; `None`
	/// before one has finished for it.
	pub ports: Option<Vec<PortOverview>>,
	/// When that scan finished.
	pub port_scanned_at: Option<i64>,
	/// `[tx, rx]` bytes per loop minute, like `Overview::minutes`.
	pub minutes: Vec<[i64; 2]>,
	/// `[tx, rx]` bytes a minute per bucket, averaged over the reports in it.
	pub day: Vec<[i64; 2]>,
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
