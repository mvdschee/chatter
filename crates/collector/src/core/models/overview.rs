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
}

#[derive(Debug, Serialize)]
pub struct PortScanSummary {
	pub started_at: i64,
	pub finished_at: i64,
	pub hosts: i64,
	pub open_ports: i64,
	/// Hosts the per-host time limit cut short: their ports aren't counted.
	pub timed_out: i64,
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
	pub channel: i64,
	pub dropped: i64,
	pub overflow: i64,
	pub last_seen: i64,
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
