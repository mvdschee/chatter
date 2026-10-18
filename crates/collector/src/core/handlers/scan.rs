use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Local;
use sqlx::Row;

use crate::{
	Result,
	core::models::scan::{NmapRun, PortScanAt, Scan, ScanTargets},
	error, info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{SCAN_HOSTS_INSERT, SCAN_PORTS_INSERT, SCANS_INSERT, SCANS_LAST_PORT_SCAN},
	},
	services::{cmd::nmap, network},
};

/// Sweeps the targets for hosts and stores what answered. The hostnames come
/// from the default gateway: on a home network that's the router, which hands
/// out the addresses and knows the names the devices gave it; the machine's
/// own DNS server (a Pi-hole, say) often doesn't. Returns how many hosts with
/// a MAC it found, and how many of those have a name.
pub async fn sweep(db: &DatabaseRepository, targets: &ScanTargets) -> Result<(usize, usize)> {
	let dns = nmap::default_gateway().await.unwrap_or(None);
	let xml = nmap::sweep(targets, dns).await?;
	let scan = Scan::from(NmapRun::from_xml(&xml)?);
	store_scan(db, "sweep", targets, &scan).await?;
	Ok((scan.hosts.len(), scan.hosts.iter().filter(|h| h.hostname.is_some()).count()))
}

/// Scans every TCP port and some UDP ones on the targets and stores the open
/// ones. This machine is left out: raw packets to its own address don't show
/// its open ports, and it has no MAC to join to the sensors' data. Returns how
/// many hosts it found, how many open ports, and how many hosts timed out.
pub async fn port_scan(
	db: &DatabaseRepository,
	targets: &ScanTargets,
) -> Result<(usize, usize, usize)> {
	let dns = nmap::default_gateway().await.unwrap_or(None);
	let me = network::default_ipv4().ok();
	let xml = nmap::port_scan(targets, dns, me).await?;
	let scan = Scan::from(NmapRun::from_xml(&xml)?);
	store_scan(db, "ports", targets, &scan).await?;
	Ok((
		scan.hosts.len(),
		scan.hosts.iter().map(|h| h.ports.len()).sum(),
		scan.hosts.iter().filter(|h| h.timed_out).count(),
	))
}

pub async fn store_scan(
	db: &DatabaseRepository,
	kind: &str,
	targets: &ScanTargets,
	scan: &Scan,
) -> Result<i64> {
	let mut rows = Vec::new();
	for h in &scan.hosts {
		rows.push((
			SCAN_HOSTS_INSERT,
			vec![
				DbValue::Text(h.mac.to_string()),
				DbValue::Text(h.ip.to_string()),
				DbValue::TextOpt(h.hostname.clone()),
				DbValue::TextOpt(h.vendor.clone()),
				DbValue::Int(h.timed_out.into()),
			],
		));
		for p in &h.ports {
			rows.push((
				SCAN_PORTS_INSERT,
				vec![
					DbValue::Text(h.mac.to_string()),
					DbValue::Int(p.port.into()),
					DbValue::Text(p.protocol.clone()),
					DbValue::TextOpt(p.service.clone()),
					DbValue::TextOpt(p.product.clone()),
				],
			));
		}
	}
	db.insert_record_with_children(
		SCANS_INSERT,
		vec![
			DbValue::Text(kind.to_string()),
			DbValue::Text(targets.to_string()),
			DbValue::Int(scan.started_at),
			DbValue::Int(scan.finished_at),
		],
		rows,
	)
	.await
}

/// What to sweep: the configured targets, else the network this machine is
/// on now, looked up again for every sweep (a laptop moves between networks).
pub fn current_targets(configured: Option<&ScanTargets>) -> Result<ScanTargets> {
	match configured {
		Some(targets) => Ok(targets.clone()),
		None => Ok(ScanTargets::around(network::default_ipv4()?)),
	}
}

/// Sweeps now, then every `every`. A failed sweep is logged and the loop goes
/// on: no network, or nmap missing or without rights, shouldn't stop the
/// collector.
pub async fn sweep_loop(db: DatabaseRepository, configured: Option<ScanTargets>, every: Duration) {
	loop {
		match current_targets(configured.as_ref()) {
			Ok(targets) => match sweep(&db, &targets).await {
				Ok((hosts, named)) => {
					info!("sweep of {targets}: {hosts} hosts, {named} with a name")
				},
				Err(e) => error!("sweep of {targets}: {e}"),
			},
			Err(e) => error!("sweep: {e}"),
		}
		tokio::time::sleep(every).await;
	}
}

/// Port-scans daily at `at` (plus up to an hour, so it doesn't hit the
/// network at the same minute every day), catching up once if a run was
/// missed. A failed scan is logged and the loop goes on.
pub async fn port_scan_loop(
	db: DatabaseRepository,
	configured: Option<ScanTargets>,
	at: PortScanAt,
) {
	loop {
		let last = match db.get_record(SCANS_LAST_PORT_SCAN, vec![]).await {
			Ok(row) => row.and_then(|r| r.try_get::<Option<i64>, _>("t").ok().flatten()),
			Err(e) => {
				error!("port scan: {e}");
				None
			},
		};
		let now = Local::now();
		let (next, catch_up) = at.next_run(now, last);
		// the scheduled run gets a spread; a catch-up doesn't
		let spread = if catch_up {
			0
		} else {
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.map_or(0, |d| d.subsec_nanos() as u64 % 3600)
		};
		let wait = (next - now).to_std().unwrap_or_default() + Duration::from_secs(spread);
		info!("next port scan in {} min", wait.as_secs() / 60);
		tokio::time::sleep(wait).await;

		match current_targets(configured.as_ref()) {
			Ok(targets) => {
				info!("port scan of {targets} started");
				match port_scan(&db, &targets).await {
					Ok((hosts, ports, timed_out)) => {
						info!(
							"port scan of {targets}: {hosts} hosts, {ports} open ports, {timed_out} timed out"
						)
					},
					Err(e) => error!("port scan of {targets}: {e}"),
				}
			},
			Err(e) => error!("port scan: {e}"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::{
		handlers::{overview::overview, report::store_report},
		models::{
			report::{Report, ReportPayload},
			scan::{PORTS_XML, SWEEP_XML},
		},
	};

	#[tokio::test]
	async fn a_sweep_names_the_sensors_devices() {
		let path = std::env::temp_dir().join(format!("chatter-test-{}.db", std::process::id()));
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();

		let mut payload: ReportPayload = serde_json::from_str(
			r#"{"sensor":"c6-1","seq":1,"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],
			"devices":[{"mac":"aa:bb:cc:00:00:01","net":0,"tx_frames":1,"tx_bytes":100,"rx_frames":1,"rx_bytes":100,"retries":0,"rssi_max":-50}],
			"broadcast":[]}"#,
		)
		.unwrap();
		payload.received_at = 1_700_000_000;
		store_report(&db, &Report::try_from(payload).unwrap()).await.unwrap();

		let targets = ScanTargets::new("192.0.2.0/24").unwrap();
		let scan = Scan::from(NmapRun::from_xml(SWEEP_XML).unwrap());
		store_scan(&db, "sweep", &targets, &scan).await.unwrap();

		let o = overview(&db).await.unwrap();
		let d = &o.devices[0];
		assert_eq!(d.hostname.as_deref(), Some("ESP_00001.lan"));
		assert_eq!((d.ip.as_deref(), d.vendor.as_deref()), (Some("192.0.2.10"), Some("Espressif")));
		assert!(d.ports.is_none(), "a sweep doesn't look at ports");

		let scan = Scan::from(NmapRun::from_xml(PORTS_XML).unwrap());
		store_scan(&db, "ports", &targets, &scan).await.unwrap();
		let o = overview(&db).await.unwrap();
		let ports: Vec<i64> = o.devices[0].ports.as_ref().unwrap().iter().map(|p| p.port).collect();
		assert_eq!(ports, [80, 5353]);
		assert_eq!(o.devices[0].port_scanned_at, Some(1_700_051_900));

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
