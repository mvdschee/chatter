use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Local;
use sqlx::Row;

use crate::{
	Result,
	core::{
		handlers::vendor::apply_vendors,
		models::{
			scan::{
				Change, DYNAMIC_FROM, NmapRun, OpenPort, PortScanAt, Scan, ScanTargets, compare,
			},
			vendor::SharedVendors,
		},
	},
	error, info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			SCAN_CHANGES_INSERT, SCAN_CHANGES_ROTATES, SCAN_HOSTS_INSERT,
			SCAN_HOSTS_PREVIOUS_PORT_SCAN, SCAN_PORTS_INSERT, SCAN_PORTS_OF_HOST, SCANS_INSERT,
			SCANS_LAST_PORT_SCAN, SCANS_PORT_SCANS_BEFORE,
		},
	},
	services::{cmd::nmap, network},
};

/// Sweeps the targets for hosts and stores what answered. The hostnames come
/// from the default gateway: on a home network that's the router, which hands
/// out the addresses and knows the names the devices gave it; the machine's
/// own DNS server (a Pi-hole, say) often doesn't. Returns how many hosts with
/// a MAC it found, and how many of those have a name.
pub async fn sweep(
	db: &DatabaseRepository,
	targets: &ScanTargets,
	vendors: &SharedVendors,
) -> Result<(usize, usize)> {
	let dns = nmap::default_gateway().await.unwrap_or(None);
	let xml = nmap::sweep(targets, dns).await?;
	let mut scan = Scan::from(NmapRun::from_xml(&xml)?);
	apply_vendors(&mut scan, vendors);
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
	vendors: &SharedVendors,
) -> Result<(usize, usize, usize)> {
	let dns = nmap::default_gateway().await.unwrap_or(None);
	let me = network::default_ipv4().ok();
	let xml = nmap::port_scan(targets, dns, me).await?;
	let mut scan = Scan::from(NmapRun::from_xml(&xml)?);
	apply_vendors(&mut scan, vendors);
	let id = store_scan(db, "ports", targets, &scan).await?;
	let alerts = record_changes(db, id, &scan).await?;
	if alerts > 0 {
		info!("port scan of {targets}: {alerts} alerts, see the Timeline");
	}
	Ok((
		scan.hosts.len(),
		scan.hosts.iter().map(|h| h.ports.len()).sum(),
		scan.hosts.iter().filter(|h| h.timed_out).count(),
	))
}

/// Compares each host in port scan `id` with the port scan before that saw
/// it, and stores what changed. The first port scan is the baseline: nothing
/// to compare with. A host that timed out keeps what it had: no comparison.
/// Returns how many changes are alerts.
pub async fn record_changes(db: &DatabaseRepository, id: i64, scan: &Scan) -> Result<usize> {
	let before = db.get_record(SCANS_PORT_SCANS_BEFORE, vec![DbValue::Int(id)]).await?;
	if before.map(|r| r.try_get::<i64, _>("n")).transpose()?.unwrap_or(0) == 0 {
		return Ok(0);
	}
	let mut alerts = 0;
	for host in scan.hosts.iter().filter(|h| !h.timed_out) {
		let mac = DbValue::Text(host.mac.to_string());
		let previous_scan: Option<i64> = db
			.get_record(SCAN_HOSTS_PREVIOUS_PORT_SCAN, vec![mac, DbValue::Int(id)])
			.await?
			.and_then(|r| r.try_get("scan_id").ok().flatten());
		let previous = match previous_scan {
			Some(prev) => {
				let rows = db
					.get_records(
						SCAN_PORTS_OF_HOST,
						vec![DbValue::Int(prev), DbValue::Text(host.mac.to_string())],
					)
					.await?;
				let ports = rows
					.iter()
					.map(|r| {
						Ok(OpenPort {
							port: u16::try_from(r.try_get::<i64, _>("port")?).unwrap_or(0),
							protocol: r.try_get("protocol")?,
							service: r.try_get("service")?,
							product: r.try_get("product")?,
						})
					})
					.collect::<Result<Vec<_>>>()?;
				Some(ports)
			},
			None => None,
		};
		let rotates = db
			.get_record(
				SCAN_CHANGES_ROTATES,
				vec![DbValue::Text(host.mac.to_string()), DbValue::Int(DYNAMIC_FROM.into())],
			)
			.await?
			.is_some();
		for change in compare(previous.as_deref(), &host.ports, rotates) {
			alerts += usize::from(change.is_alert());
			store_change(db, id, &host.mac.to_string(), &change).await?;
		}
	}
	Ok(alerts)
}

async fn store_change(
	db: &DatabaseRepository,
	scan: i64,
	mac: &str,
	change: &Change,
) -> Result<i64> {
	let port = change.port();
	let before = match change {
		Change::ChangedPort {
			before,
			..
		} => Some(before.clone()),
		_ => None,
	};
	db.insert_record(
		SCAN_CHANGES_INSERT,
		vec![
			DbValue::Int(scan),
			DbValue::Text(mac.to_string()),
			DbValue::Text(change.kind().to_string()),
			DbValue::Int(change.is_alert().into()),
			DbValue::IntOpt(port.map(|p| p.port.into())),
			DbValue::TextOpt(port.map(|p| p.protocol.clone())),
			DbValue::TextOpt(port.and_then(|p| p.service.clone())),
			DbValue::TextOpt(port.and_then(|p| p.product.clone())),
			DbValue::TextOpt(before),
		],
	)
	.await
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
pub async fn sweep_loop(
	db: DatabaseRepository,
	configured: Option<ScanTargets>,
	every: Duration,
	vendors: SharedVendors,
) {
	loop {
		match current_targets(configured.as_ref()) {
			Ok(targets) => match sweep(&db, &targets, &vendors).await {
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
	vendors: SharedVendors,
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
				match port_scan(&db, &targets, &vendors).await {
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

	#[tokio::test]
	async fn a_second_port_scan_finds_the_changes() {
		let path =
			std::env::temp_dir().join(format!("chatter-test-changes-{}.db", std::process::id()));
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();
		let targets = ScanTargets::new("192.0.2.0/24").unwrap();

		// the baseline: nothing to compare with
		let first = Scan::from(NmapRun::from_xml(PORTS_XML).unwrap());
		let id = store_scan(&db, "ports", &targets, &first).await.unwrap();
		assert_eq!(record_changes(&db, id, &first).await.unwrap(), 0);

		// a day later: port 80 moved to 8080, and the host that timed out finished
		let xml = PORTS_XML
			.replace(r#"portid="80""#, r#"portid="8080""#)
			.replace(r#" timedout="true""#, "")
			.replace(r#"start="1700050000""#, r#"start="1700136400""#)
			.replace(r#"time="1700051900""#, r#"time="1700138300""#);
		let second = Scan::from(NmapRun::from_xml(&xml).unwrap());
		let id = store_scan(&db, "ports", &targets, &second).await.unwrap();
		// the new port, and the host that's new to a finished scan
		assert_eq!(record_changes(&db, id, &second).await.unwrap(), 2);

		// the overview's day ends at the newest report: one at the second scan
		let mut payload: ReportPayload = serde_json::from_str(
			r#"{"sensor":"c6-1","seq":1,"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],"devices":[],"broadcast":[]}"#,
		)
		.unwrap();
		payload.received_at = 1_700_138_300;
		store_report(&db, &Report::try_from(payload).unwrap()).await.unwrap();

		let latest = crate::core::handlers::overview::latest_report(&db).await.unwrap();
		let window = crate::core::models::timeline::Window::ending_at(latest, 86_400);
		let o = crate::core::handlers::timeline::timeline(&db, window).await.unwrap();
		let kinds: Vec<&str> = o.changes.iter().map(|c| c.kind.as_str()).collect();
		assert_eq!(kinds, ["new_port", "closed_port", "new_device"]);
		assert_eq!(o.changes[0].port, Some(8080));
		assert_eq!(o.port_scans.last().unwrap().alerts, 2);

		for suffix in ["", "-wal", "-shm"] {
			let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
		}
	}
}
