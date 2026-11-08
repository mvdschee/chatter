use std::{
	collections::HashMap,
	time::{SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::{
		handlers::{baseline::load_usual_traffic, device::device_names},
		models::{
			baseline::{ScoreScale, hour_of},
			config::DnsSource,
			dns::{first_shown_day, router_forwards, top_domains},
			overview::{
				DeviceDns, DeviceOverview, DnsOverview, LOOP_MINUTES, Overview, PortOverview,
				RECENT_S, SensorStatus, seq_stats,
			},
		},
	},
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			DEVICE_MINUTES_COUNT_FOR_REPORT, DEVICE_MINUTES_DEVICES, DEVICE_MINUTES_LAST_N_REPORTS,
			DNS_DAYS_SINCE, DNS_DOMAINS_SINCE, REPORTS_LAST_N, REPORTS_LATEST_TIME,
			REPORTS_SENSORS, REPORTS_TIMES_SINCE, SCAN_HOSTS_LAST_PORT_SCAN, SCAN_HOSTS_LATEST,
			SCAN_PORTS_LATEST,
		},
	},
};

/// The newest report's time; the clock without reports. Everything "recent"
/// counts back from it, not from the clock, so a copy of old data still shows
/// its last day.
pub async fn latest_report(db: &DatabaseRepository) -> Result<i64> {
	let latest = match db.get_record(REPORTS_LATEST_TIME, vec![]).await? {
		Some(row) => row.try_get::<Option<i64>, _>("t")?,
		None => None,
	};
	Ok(latest.unwrap_or_else(|| {
		SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
	}))
}

/// The live part: the loop's minutes, every device ever heard, and the
/// sensors. The history is `timeline`. With a DNS server, also what its log
/// says about each device.
pub async fn overview(db: &DatabaseRepository, dns: Option<&DnsSource>) -> Result<Overview> {
	let latest = latest_report(db).await?;
	let since = latest - RECENT_S;

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

	let mut usual = load_usual_traffic(db).await?;
	let mut names = device_names(db).await?;
	let mut devices: Vec<DeviceOverview> = db
		.get_records(DEVICE_MINUTES_DEVICES, vec![DbValue::Int(since)])
		.await?
		.iter()
		.map(|row| {
			let mac: String = row.try_get("mac")?;
			Ok(DeviceOverview {
				usual: usual.remove(&mac),
				name: names.remove(&mac),
				mac,
				first_seen: row.try_get("first_seen")?,
				last_seen: row.try_get("last_seen")?,
				rssi_max: row.try_get("rssi_max")?,
				swept_at: None,
				ip: None,
				hostname: None,
				vendor: None,
				ports: None,
				port_scanned_at: None,
				minutes: vec![[0, 0]; loop_reports.len()],
				dns: None,
			})
		})
		.collect::<Result<_>>()?;
	let index: HashMap<String, usize> =
		devices.iter().enumerate().map(|(i, d)| (d.mac.clone(), i)).collect();

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
			devices[d].swept_at = row.try_get("finished_at")?;
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

	let dns = match dns {
		Some(source) => Some(add_dns(db, source, latest, &mut devices).await?),
		None => None,
	};

	// each sensor's last 24 hours: its sequence numbers and report times, in order
	let mut recent: HashMap<String, (Vec<i64>, Vec<i64>)> = HashMap::new();
	for row in db.get_records(REPORTS_TIMES_SINCE, vec![DbValue::Int(since)]).await? {
		let (seqs, beats) = recent.entry(row.try_get("sensor")?).or_default();
		seqs.push(row.try_get("seq")?);
		beats.push(row.try_get("received_at")?);
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
		let (seqs, beats) = recent.remove(&name).unwrap_or_default();
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
			beats,
		});
	}

	Ok(Overview {
		latest,
		minutes: loop_reports.iter().map(|(_, t)| *t).collect(),
		hours: loop_reports.iter().map(|(_, t)| hour_of(*t)).collect(),
		score: ScoreScale::default(),
		sensors,
		devices,
		dns,
	})
}

/// Each device's top domains over the days shown, and the link to its address in
/// the DNS server's query log. Every device gets them, an empty list included.
async fn add_dns(
	db: &DatabaseRepository,
	source: &DnsSource,
	latest: i64,
	devices: &mut [DeviceOverview],
) -> Result<DnsOverview> {
	let since = first_shown_day(latest);
	let rows = db
		.get_records(DNS_DOMAINS_SINCE, vec![DbValue::Int(since)])
		.await?
		.iter()
		.map(|row| Ok((row.try_get("mac")?, row.try_get("domain")?, row.try_get("queries")?)))
		.collect::<Result<_>>()?;
	let mut top = top_domains(rows);
	for device in devices.iter_mut() {
		device.dns = Some(DeviceDns {
			domains: top.remove(&device.mac).unwrap_or_default(),
			query_log: device.ip.as_deref().map(|ip| source.query_log(ip)),
		});
	}
	let (total, from_gateway) =
		match db.get_record(DNS_DAYS_SINCE, vec![DbValue::Int(since)]).await? {
			Some(row) => (row.try_get("total")?, row.try_get("from_gateway")?),
			None => (0, 0),
		};
	Ok(DnsOverview {
		router_forwards: router_forwards(from_gateway, total),
	})
}
