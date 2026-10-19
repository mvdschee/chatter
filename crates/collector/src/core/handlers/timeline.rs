use std::collections::BTreeMap;

use sqlx::Row;

use crate::{
	Result,
	core::models::timeline::{
		ChangeOverview, DeviceSeries, PortScanSummary, Timeline, Window, consolidate,
	},
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			DEVICE_MINUTES_PER_BUCKET_BETWEEN, REPORTS_TOTALS_BETWEEN, SCAN_CHANGES_BETWEEN,
			SCANS_PORT_SCANS_BETWEEN,
		},
	},
};

/// The window's traffic in buckets (average and peak), every device's series,
/// and its port scans and their changes.
pub async fn timeline(db: &DatabaseRepository, w: Window) -> Result<Timeline> {
	let n = w.buckets();
	let between = || vec![DbValue::Int(w.from), DbValue::Int(w.to)];

	// the network: each report's total is a minute
	let mut minutes = Vec::new();
	for row in db.get_records(REPORTS_TOTALS_BETWEEN, between()).await? {
		if let Some(b) = w.index(row.try_get("received_at")?) {
			minutes.push((b, row.try_get::<i64, _>("total")?));
		}
	}
	let total = consolidate(&minutes, n);
	let mut per_bucket = vec![0i64; n];
	for &(b, _) in &minutes {
		per_bucket[b] += 1;
	}

	// each device, averaged over the minutes in each bucket
	let mut series: BTreeMap<String, Vec<[i64; 2]>> = BTreeMap::new();
	let params = vec![DbValue::Int(w.from), DbValue::Int(w.bucket_s), DbValue::Int(w.to)];
	for row in db.get_records(DEVICE_MINUTES_PER_BUCKET_BETWEEN, params).await? {
		let Some(b) = usize::try_from(row.try_get::<i64, _>("bucket")?).ok().filter(|&b| b < n)
		else {
			continue;
		};
		let reports = per_bucket[b].max(1);
		let buckets = series.entry(row.try_get("mac")?).or_insert_with(|| vec![[0, 0]; n]);
		buckets[b] =
			[row.try_get::<i64, _>("tx")? / reports, row.try_get::<i64, _>("rx")? / reports];
	}
	let devices = series
		.into_iter()
		.map(|(mac, buckets)| DeviceSeries {
			mac,
			buckets,
		})
		.collect();

	let port_scans = db
		.get_records(SCANS_PORT_SCANS_BETWEEN, between())
		.await?
		.iter()
		.map(|row| {
			Ok(PortScanSummary {
				started_at: row.try_get("started_at")?,
				finished_at: row.try_get("finished_at")?,
				hosts: row.try_get("hosts")?,
				open_ports: row.try_get("open_ports")?,
				timed_out: row.try_get("timed_out")?,
				alerts: row.try_get("alerts")?,
			})
		})
		.collect::<Result<_>>()?;

	let changes = db
		.get_records(SCAN_CHANGES_BETWEEN, between())
		.await?
		.iter()
		.map(|row| {
			Ok(ChangeOverview {
				at: row.try_get("finished_at")?,
				mac: row.try_get("mac")?,
				ip: row.try_get("ip")?,
				hostname: row.try_get("hostname")?,
				vendor: row.try_get("vendor")?,
				kind: row.try_get("kind")?,
				alert: row.try_get::<i64, _>("alert")? == 1,
				port: row.try_get("port")?,
				protocol: row.try_get("protocol")?,
				service: row.try_get("service")?,
				product: row.try_get("product")?,
				before: row.try_get("before")?,
				ports: row.try_get("ports")?,
			})
		})
		.collect::<Result<_>>()?;

	Ok(Timeline {
		from: w.from,
		to: w.to,
		bucket_s: w.bucket_s,
		total,
		devices,
		port_scans,
		changes,
	})
}
