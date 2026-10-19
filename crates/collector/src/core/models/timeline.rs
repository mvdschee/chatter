//! The history: the network's traffic over a stretch of time in buckets that
//! fit a graph, each with its average and its peak minute, every device's
//! series for replaying it on the plan, and the port scans and their changes
//! in it. See `docs/timeline.md`.

use serde::Serialize;

use crate::core::models::baseline::{UsualTraffic, hour_of};

/// The ranges the page offers, ending at the newest report.
pub const RANGES: [(&str, i64); 5] =
	[("1h", 3600), ("6h", 6 * 3600), ("24h", 86_400), ("7d", 7 * 86_400), ("30d", 30 * 86_400)];

/// The longest stretch one request may ask for.
pub const MAX_SPAN_S: i64 = 400 * 86_400;

/// Bucket sizes, smallest first; the classic traffic-graph tiers and some
/// between them.
const BUCKET_SIZES: [i64; 8] = [60, 300, 900, 1800, 3600, 7200, 21_600, 86_400];
/// At most this many buckets to a graph: about one per few pixels.
const MAX_BUCKETS: i64 = 400;

pub fn range_seconds(name: &str) -> Option<i64> {
	RANGES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// The smallest bucket that keeps a stretch within `MAX_BUCKETS`: 1 hour and
/// 6 hours in minutes, 24 hours in 5 minutes, 7 days in 30, 30 days in 2 hours.
pub fn bucket_for(span_s: i64) -> i64 {
	BUCKET_SIZES.into_iter().find(|b| span_s / b <= MAX_BUCKETS).unwrap_or(86_400)
}

/// A stretch of time on bucket boundaries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
	pub from: i64,
	pub to: i64,
	pub bucket_s: i64,
}

impl Window {
	/// From `from` up to (not including) `to`, widened to whole buckets.
	pub fn new(from: i64, to: i64) -> Self {
		let bucket_s = bucket_for((to - from).max(1));
		Self {
			from: from.div_euclid(bucket_s) * bucket_s,
			to: (to + bucket_s - 1).div_euclid(bucket_s) * bucket_s,
			bucket_s,
		}
	}

	/// The `span_s` that ends with the minute `latest` is in.
	pub fn ending_at(latest: i64, span_s: i64) -> Self {
		Self::new(latest + 1 - span_s, latest + 1)
	}

	pub fn buckets(&self) -> usize {
		((self.to - self.from) / self.bucket_s) as usize
	}

	/// Each bucket's hour of the day, in the collector's time zone, for
	/// scoring it against that hour's baselines. `None` for buckets longer
	/// than an hour: they span several, so they're scored against all hours.
	pub fn hours(&self) -> Vec<Option<usize>> {
		(0..self.buckets() as i64)
			.map(|b| (self.bucket_s <= 3600).then(|| hour_of(self.from + b * self.bucket_s)))
			.collect()
	}

	pub fn index(&self, t: i64) -> Option<usize> {
		(t >= self.from && t < self.to).then(|| ((t - self.from) / self.bucket_s) as usize)
	}
}

/// Each bucket's average and peak of the minutes in it: `[average, peak]`.
/// A report is a minute, so its total is bytes a minute.
pub fn consolidate(minutes: &[(usize, i64)], buckets: usize) -> Vec<[i64; 2]> {
	let mut sum = vec![0i64; buckets];
	let mut count = vec![0i64; buckets];
	let mut peak = vec![0i64; buckets];
	for &(b, total) in minutes.iter().filter(|(b, _)| *b < buckets) {
		sum[b] += total;
		count[b] += 1;
		peak[b] = peak[b].max(total);
	}
	(0..buckets).map(|b| [sum[b] / count[b].max(1), peak[b]]).collect()
}

#[derive(Debug, Serialize)]
pub struct Timeline {
	pub from: i64,
	pub to: i64,
	pub bucket_s: i64,
	/// See `Window::hours`.
	pub hours: Vec<Option<usize>>,
	/// The network per bucket: `[average, peak]` bytes a minute.
	pub total: Vec<[i64; 2]>,
	/// Every device with traffic in the stretch.
	pub devices: Vec<DeviceSeries>,
	/// The port scans that overlap the stretch, oldest first.
	pub port_scans: Vec<PortScanSummary>,
	/// What those port scans found changed, in order.
	pub changes: Vec<ChangeOverview>,
}

#[derive(Debug, Serialize)]
pub struct DeviceSeries {
	pub mac: String,
	/// Per bucket: `[tx, rx]` bytes a minute, averaged over its minutes.
	pub buckets: Vec<[i64; 2]>,
	/// Its baselines; `None` before the first are worked out.
	pub usual: Option<UsualTraffic>,
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn buckets_follow_the_traffic_graph_tiers() {
		let b = |name| bucket_for(range_seconds(name).unwrap());
		assert_eq!([b("1h"), b("6h"), b("24h"), b("7d"), b("30d")], [60, 60, 300, 1800, 7200]);
		for (_, span) in RANGES {
			assert!(span / bucket_for(span) <= MAX_BUCKETS);
		}
	}

	#[test]
	fn a_window_ends_with_the_newest_minute() {
		let w = Window::ending_at(1_700_000_123, 86_400);
		assert_eq!(w.bucket_s, 300);
		assert_eq!((w.from % 300, w.to % 300), (0, 0));
		assert_eq!(w.index(1_700_000_123), Some(w.buckets() - 1));
		assert_eq!(w.index(w.to), None);
		assert_eq!(w.index(w.from - 1), None);
	}

	#[test]
	fn only_buckets_up_to_an_hour_have_an_hour() {
		let day = Window::ending_at(1_700_000_123, 86_400);
		assert_eq!(day.hours().len(), day.buckets());
		assert!(day.hours().iter().all(|h| h.is_some_and(|h| h < 24)));
		let month = Window::ending_at(1_700_000_123, 30 * 86_400);
		assert!(month.hours().iter().all(Option::is_none));
	}

	#[test]
	fn a_bucket_keeps_its_peak_minute() {
		// a five-minute spike among quiet minutes, in a two-hour bucket
		let mut minutes: Vec<(usize, i64)> = (0..120).map(|_| (0, 1_000)).collect();
		minutes.extend((0..5).map(|_| (0, 10_000_000)));
		let c = consolidate(&minutes, 2);
		assert_eq!(c[0][1], 10_000_000, "the peak survives");
		assert!(c[0][0] < 500_000, "the average flattens it");
		assert_eq!(c[1], [0, 0], "an empty bucket");
	}
}
