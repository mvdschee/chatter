use std::time::Duration;

use crate::{
	Result,
	core::models::scan::{NmapRun, Scan, ScanTargets},
	error, info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{SCAN_HOSTS_INSERT, SCANS_INSERT},
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

pub async fn store_scan(
	db: &DatabaseRepository,
	kind: &str,
	targets: &ScanTargets,
	scan: &Scan,
) -> Result<i64> {
	let hosts = scan
		.hosts
		.iter()
		.map(|h| {
			(
				SCAN_HOSTS_INSERT,
				vec![
					DbValue::Text(h.mac.to_string()),
					DbValue::Text(h.ip.to_string()),
					DbValue::TextOpt(h.hostname.clone()),
					DbValue::TextOpt(h.vendor.clone()),
				],
			)
		})
		.collect();
	db.insert_record_with_children(
		SCANS_INSERT,
		vec![
			DbValue::Text(kind.to_string()),
			DbValue::Text(targets.to_string()),
			DbValue::Int(scan.started_at),
			DbValue::Int(scan.finished_at),
		],
		hosts,
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::{
		handlers::{overview::overview, report::store_report},
		models::{
			report::{Report, ReportPayload},
			scan::SWEEP_XML,
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

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
