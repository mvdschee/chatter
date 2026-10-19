use std::{
	collections::HashMap,
	time::{SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::models::overview::{
		BUCKET_S, BUCKETS, ChangeOverview, DeviceOverview, LOOP_MINUTES, Overview, PortOverview,
		PortScanSummary, SensorStatus, seq_stats,
	},
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			DEVICE_MINUTES_COUNT_FOR_REPORT, DEVICE_MINUTES_DEVICES, DEVICE_MINUTES_LAST_N_REPORTS,
			DEVICE_MINUTES_PER_BUCKET, REPORTS_LAST_N, REPORTS_LATEST_TIME, REPORTS_PER_BUCKET,
			REPORTS_SENSORS, REPORTS_SEQS_SINCE, SCAN_CHANGES_SINCE, SCAN_HOSTS_LAST_PORT_SCAN,
			SCAN_HOSTS_LATEST, SCAN_PORTS_LATEST, SCANS_PORT_SCANS_SINCE,
		},
	},
};

/// The day ending at the newest report (not the clock: a copy of old data
/// still shows its last day), the loop's minutes, and the sensors.
pub async fn overview(db: &DatabaseRepository) -> Result<Overview> {
	let latest = match db.get_record(REPORTS_LATEST_TIME, vec![]).await? {
		Some(row) => row.try_get::<Option<i64>, _>("t")?,
		None => None,
	}
	.unwrap_or_else(|| {
		SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
	});
	let day_start = day_start(latest);

	// the loop: the newest reports, oldest first
	let mut loop_reports: Vec<(i64, i64)> = db
		.get_records(REPORTS_LAST_N, vec![DbValue::Int(LOOP_MINUTES as i64)])
		.await?
		.iter()
		.map(|row| Ok((row.try_get("id")?, row.try_get("received_at")?)))
		.collect::<Result<_>>()?;
	loop_reports.reverse();
	let slot: HashMap<i64, usize> =
		loop_reports.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();

	let window = vec![DbValue::Int(day_start), DbValue::Int(BUCKET_S)];
	let mut reports_in = vec![0i64; BUCKETS];
	for row in db.get_records(REPORTS_PER_BUCKET, window).await? {
		if let Some(n) = bucket_index(row.try_get("bucket")?) {
			reports_in[n] = row.try_get("n")?;
		}
	}

	let mut devices: Vec<DeviceOverview> = db
		.get_records(DEVICE_MINUTES_DEVICES, vec![DbValue::Int(day_start)])
		.await?
		.iter()
		.map(|row| {
			Ok(DeviceOverview {
				mac: row.try_get("mac")?,
				first_seen: row.try_get("first_seen")?,
				last_seen: row.try_get("last_seen")?,
				rssi_max: row.try_get("rssi_max")?,
				ip: None,
				hostname: None,
				vendor: None,
				ports: None,
				port_scanned_at: None,
				minutes: vec![[0, 0]; loop_reports.len()],
				day: vec![[0, 0]; BUCKETS],
			})
		})
		.collect::<Result<_>>()?;
	let index: HashMap<String, usize> =
		devices.iter().enumerate().map(|(i, d)| (d.mac.clone(), i)).collect();

	let window = vec![DbValue::Int(day_start), DbValue::Int(BUCKET_S)];
	for row in db.get_records(DEVICE_MINUTES_PER_BUCKET, window).await? {
		let mac: String = row.try_get("mac")?;
		let (Some(&d), Some(b)) = (index.get(&mac), bucket_index(row.try_get("bucket")?)) else {
			continue;
		};
		let n = reports_in[b].max(1);
		devices[d].day[b] = [row.try_get::<i64, _>("tx")? / n, row.try_get::<i64, _>("rx")? / n];
	}

	let last_n = vec![DbValue::Int(LOOP_MINUTES as i64)];
	for row in db.get_records(DEVICE_MINUTES_LAST_N_REPORTS, last_n).await? {
		let mac: String = row.try_get("mac")?;
		let report: i64 = row.try_get("report_id")?;
		if let (Some(&d), Some(&m)) = (index.get(&mac), slot.get(&report)) {
			devices[d].minutes[m] = [row.try_get("tx_bytes")?, row.try_get("rx_bytes")?];
		}
	}

	for row in db.get_records(SCAN_HOSTS_LATEST, vec![]).await? {
		let mac: String = row.try_get("mac")?;
		if let Some(&d) = index.get(&mac) {
			devices[d].ip = row.try_get("ip")?;
			devices[d].hostname = row.try_get("hostname")?;
			devices[d].vendor = row.try_get("vendor")?;
		}
	}

	for row in db.get_records(SCAN_HOSTS_LAST_PORT_SCAN, vec![]).await? {
		let mac: String = row.try_get("mac")?;
		if let Some(&d) = index.get(&mac) {
			devices[d].port_scanned_at = row.try_get("finished_at")?;
			devices[d].ports = Some(Vec::new());
		}
	}
	for row in db.get_records(SCAN_PORTS_LATEST, vec![]).await? {
		let mac: String = row.try_get("mac")?;
		if let Some(ports) = index.get(&mac).and_then(|&d| devices[d].ports.as_mut()) {
			ports.push(PortOverview {
				port: row.try_get("port")?,
				protocol: row.try_get("protocol")?,
				service: row.try_get("service")?,
				product: row.try_get("product")?,
			});
		}
	}

	let port_scans = db
		.get_records(SCANS_PORT_SCANS_SINCE, vec![DbValue::Int(day_start)])
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
		.get_records(SCAN_CHANGES_SINCE, vec![DbValue::Int(day_start)])
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

	// each sensor's day: its sequence numbers in order, and its reports per bucket
	let mut days: HashMap<String, (Vec<i64>, Vec<i64>)> = HashMap::new();
	let window = vec![DbValue::Int(day_start), DbValue::Int(BUCKET_S)];
	for row in db.get_records(REPORTS_SEQS_SINCE, window).await? {
		let (seqs, beats) =
			days.entry(row.try_get("sensor")?).or_insert_with(|| (Vec::new(), vec![0; BUCKETS]));
		seqs.push(row.try_get("seq")?);
		if let Some(b) = bucket_index(row.try_get("bucket")?) {
			beats[b] += 1;
		}
	}

	let mut sensors = Vec::new();
	for row in db.get_records(REPORTS_SENSORS, vec![]).await? {
		let name: String = row.try_get("sensor")?;
		let report: i64 = row.try_get("id")?;
		let devices = db
			.get_record(DEVICE_MINUTES_COUNT_FOR_REPORT, vec![DbValue::Int(report)])
			.await?
			.map(|r| r.try_get("n"))
			.transpose()?
			.unwrap_or(0);
		let (seqs, heartbeat) =
			days.remove(&name).unwrap_or_else(|| (Vec::new(), vec![0; BUCKETS]));
		let (missing, restarts) = seq_stats(&seqs);
		let nets_known: String = row.try_get("nets_known")?;
		sensors.push(SensorStatus {
			name,
			last_seen: row.try_get("last_seen")?,
			seq: row.try_get("seq")?,
			uptime_s: row.try_get("uptime_s")?,
			channel: row.try_get("channel")?,
			dropped: row.try_get("dropped")?,
			overflow: row.try_get("overflow")?,
			nets_known: serde_json::from_str(&nets_known).unwrap_or_default(),
			devices,
			reports: seqs.len() as i64,
			missing,
			restarts,
			heartbeat,
		});
	}

	Ok(Overview {
		latest,
		minutes: loop_reports.iter().map(|(_, t)| *t).collect(),
		day_start,
		bucket_s: BUCKET_S,
		sensors,
		devices,
		port_scans,
		changes,
	})
}

/// The first bucket's start, so the last bucket holds `latest`.
pub fn day_start(latest: i64) -> i64 {
	(latest / BUCKET_S - (BUCKETS as i64 - 1)) * BUCKET_S
}

fn bucket_index(bucket: i64) -> Option<usize> {
	usize::try_from(bucket).ok().filter(|&b| b < BUCKETS)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_last_bucket_holds_the_latest_report() {
		let latest = 1_700_000_123;
		let start = day_start(latest);
		assert_eq!(start % BUCKET_S, 0);
		assert_eq!((latest - start) / BUCKET_S, BUCKETS as i64 - 1);
	}

	#[test]
	fn buckets_outside_the_day_are_dropped() {
		assert_eq!(bucket_index(-1), None);
		assert_eq!(bucket_index(0), Some(0));
		assert_eq!(bucket_index(BUCKETS as i64), None);
	}
}
