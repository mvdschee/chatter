//! Reads a DNS server's query log into counts per device, day and domain. See
//! `docs/dns.md`.

use std::{
	net::IpAddr,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::models::{
		dns::{
			DomainCount, KEEP_DAYS, Sighting, Window, day_of, day_totals, domain_counts,
			next_window, sightings_around,
		},
		report::Mac,
	},
	error,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{
			DNS_DAYS_ADD, DNS_DAYS_DELETE_BEFORE, DNS_DOMAINS_ADD, DNS_DOMAINS_DELETE_BEFORE,
			DNS_READ_UNTIL, DNS_READ_UNTIL_SET, SCAN_HOSTS_SIGHTINGS_BETWEEN, SCANS_LAST_SWEEP,
		},
	},
	services::{cmd::nmap, dns::DnsLog, web::shutdown},
};

/// Most polls find nothing to read: the window only opens after a sweep.
const POLL_EVERY: Duration = Duration::from_secs(60);

/// Longer than this and stopping doesn't wait for the DNS server.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// What can be read now, if anything. No request goes to the DNS server for
/// this.
async fn due_window(
	db: &DatabaseRepository,
	now: i64,
	sweep_every_s: i64,
) -> Result<Option<Window>> {
	let read_until = db
		.get_record(DNS_READ_UNTIL, vec![])
		.await?
		.map(|row| row.try_get("read_until"))
		.transpose()?;
	let last_sweep = db
		.get_record(SCANS_LAST_SWEEP, vec![])
		.await?
		.and_then(|row| row.try_get::<Option<i64>, _>("t").ok().flatten());
	Ok(next_window(now, read_until, last_sweep, sweep_every_s))
}

/// Reads one window and stores its counts, along with how far that got: all of
/// it or none, so that a failure anywhere reads the window again next time, and
/// a query is never counted twice. Returns how many queries it read.
async fn read_window<L: DnsLog>(
	db: &DatabaseRepository,
	log: &mut L,
	window: Window,
	gateway: Option<IpAddr>,
) -> Result<usize> {
	let queries = log.queries(window.from, window.until).await?;
	let clients = log.clients().await?;
	let sightings = sightings(db, window).await?;

	let mut statements = Vec::new();
	for count in domain_counts(&queries, &clients, &sightings) {
		statements.push(domain_row(count));
	}
	for day in day_totals(&queries, gateway) {
		statements.push((
			DNS_DAYS_ADD,
			vec![DbValue::Int(day.day), DbValue::Int(day.total), DbValue::Int(day.from_gateway)],
		));
	}
	statements.push((DNS_READ_UNTIL_SET, vec![DbValue::Int(window.until)]));

	let keep_from = day_of(window.until) - KEEP_DAYS;
	statements.push((DNS_DOMAINS_DELETE_BEFORE, vec![DbValue::Int(keep_from)]));
	statements.push((DNS_DAYS_DELETE_BEFORE, vec![DbValue::Int(keep_from)]));

	db.execute_all(statements).await?;
	Ok(queries.len())
}

fn domain_row(count: DomainCount) -> (&'static str, Vec<DbValue>) {
	(
		DNS_DOMAINS_ADD,
		vec![
			DbValue::Text(count.mac),
			DbValue::Int(count.day),
			DbValue::Text(count.domain),
			DbValue::Int(count.queries),
		],
	)
}

async fn sightings(db: &DatabaseRepository, window: Window) -> Result<Vec<Sighting>> {
	let (from, until) = sightings_around(window);
	let rows = db
		.get_records(SCAN_HOSTS_SIGHTINGS_BETWEEN, vec![DbValue::Int(from), DbValue::Int(until)])
		.await?;
	let mut sightings = Vec::with_capacity(rows.len());
	for row in rows {
		let (ip, mac): (String, String) = (row.try_get("ip")?, row.try_get("mac")?);
		sightings.push(Sighting {
			at: row.try_get("at")?,
			ip: ip.parse().map_err(|_| {
				crate::Error::Database(format!("scan_hosts: not an address: {ip:?}"))
			})?,
			mac: Mac::new(&mac)?,
		});
	}
	Ok(sightings)
}

async fn poll<L: DnsLog>(db: &DatabaseRepository, log: &mut L, sweep_every_s: i64) -> Result<()> {
	let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
	if let Some(window) = due_window(db, now, sweep_every_s).await? {
		let gateway = nmap::default_gateway().await.unwrap_or(None).map(IpAddr::V4);
		read_window(db, log, window, gateway).await?;
	}
	Ok(())
}

/// Polls until the collector stops, then gives the server its session back.
pub async fn dns_loop<L: DnsLog>(db: DatabaseRepository, mut log: L, sweep_every_s: i64) {
	let polling = async {
		loop {
			if let Err(e) = poll(&db, &mut log, sweep_every_s).await {
				error!("dns: {e}");
			}
			tokio::time::sleep(POLL_EVERY).await;
		}
	};
	// stopping halfway through a read loses nothing: the transaction is whole
	tokio::select! {
		() = polling => {},
		() = shutdown() => {},
	}
	match tokio::time::timeout(CLOSE_TIMEOUT, log.close()).await {
		Ok(Err(e)) => error!("dns: {e}"),
		Err(_) => error!("dns: the server didn't answer in time to close the session"),
		Ok(Ok(())) => {},
	}
}

#[cfg(test)]
mod tests {
	use std::{
		collections::BTreeMap,
		sync::atomic::{AtomicU32, Ordering},
	};

	use super::*;
	use crate::{
		Error,
		core::models::dns::{
			DnsClient, DnsQuery, LONE_SIGHTING_MAX_AGE_S, READ_BEHIND_S, SIGHTING_MAX_AGE_S,
		},
		repository::schemas::{SCAN_HOSTS_INSERT, SCANS_INSERT},
	};

	const EVERY: i64 = 600;
	/// A UTC midnight, with room around it.
	const T: i64 = 1_700_006_400;
	const IP: &str = "192.0.2.7";
	const OTHER_IP: &str = "192.0.2.8";
	const GATEWAY: &str = "192.0.2.1";
	const MAC_A: &str = "e7:f2:5e:ef:88:30";
	const MAC_B: &str = "b5:11:ac:5e:9d:9c";

	/// A DNS server that holds `queries` and counts what it's asked.
	#[derive(Default)]
	struct FakeLog {
		queries: Vec<DnsQuery>,
		clients: Vec<DnsClient>,
		reads: Vec<(i64, i64)>,
		table_reads: usize,
		fail_queries: bool,
		fail_clients: bool,
	}

	impl FakeLog {
		fn with(queries: Vec<DnsQuery>) -> Self {
			Self {
				queries,
				..Self::default()
			}
		}
	}

	impl DnsLog for FakeLog {
		async fn queries(&mut self, from: i64, until: i64) -> Result<Vec<DnsQuery>> {
			self.reads.push((from, until));
			if self.fail_queries {
				return Err(Error::Dns("no answer".into()));
			}
			Ok(self.queries.iter().filter(|q| (from..until).contains(&q.at)).cloned().collect())
		}

		async fn clients(&mut self) -> Result<Vec<DnsClient>> {
			self.table_reads += 1;
			if self.fail_clients {
				return Err(Error::Dns("no answer".into()));
			}
			Ok(self.clients.clone())
		}
	}

	async fn database() -> DatabaseRepository {
		static NEXT: AtomicU32 = AtomicU32::new(0);
		let path = std::env::temp_dir().join(format!(
			"chatter-test-dns-{}-{}.db",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::SeqCst)
		));
		let _ = std::fs::remove_file(&path);
		let db = DatabaseRepository::new(path.to_str().unwrap()).await.unwrap();
		db.init().await.unwrap();
		db
	}

	async fn sweep_kind(db: &DatabaseRepository, kind: &str, at: i64, hosts: &[(&str, &str)]) {
		let id = db
			.insert_record(
				SCANS_INSERT,
				vec![
					DbValue::Text(kind.into()),
					DbValue::Text("192.0.2.0/24".into()),
					DbValue::Int(at - 5),
					DbValue::Int(at),
				],
			)
			.await
			.unwrap();
		let rows = hosts
			.iter()
			.map(|(ip, mac)| {
				(
					SCAN_HOSTS_INSERT,
					vec![
						DbValue::Int(id),
						DbValue::Text((*mac).into()),
						DbValue::Text((*ip).into()),
						DbValue::TextOpt(None),
						DbValue::TextOpt(None),
						DbValue::Int(0),
					],
				)
			})
			.collect();
		db.execute_all(rows).await.unwrap();
	}

	async fn sweep(db: &DatabaseRepository, at: i64, hosts: &[(&str, &str)]) {
		sweep_kind(db, "sweep", at, hosts).await;
	}

	fn query(at: i64, client: &str, name: &str) -> DnsQuery {
		DnsQuery {
			at,
			client: client.parse().unwrap(),
			name: name.into(),
		}
	}

	/// One poll at `now`, without the look-up of the gateway. Returns whether a
	/// window was read.
	async fn poll_at(db: &DatabaseRepository, log: &mut FakeLog, now: i64) -> Result<bool> {
		let Some(window) = due_window(db, now, EVERY).await? else {
			return Ok(false);
		};
		read_window(db, log, window, GATEWAY.parse().ok()).await?;
		Ok(true)
	}

	async fn domains(db: &DatabaseRepository) -> BTreeMap<(String, i64, String), i64> {
		db.get_records("SELECT mac, day, domain, queries FROM dns_domains", vec![])
			.await
			.unwrap()
			.iter()
			.map(|row| {
				(
					(
						row.try_get("mac").unwrap(),
						row.try_get("day").unwrap(),
						row.try_get("domain").unwrap(),
					),
					row.try_get("queries").unwrap(),
				)
			})
			.collect()
	}

	async fn queries_of(db: &DatabaseRepository, mac: &str) -> i64 {
		domains(db).await.iter().filter(|((m, ..), _)| m == mac).map(|(_, n)| n).sum()
	}

	async fn read_until(db: &DatabaseRepository) -> Option<i64> {
		db.get_record(DNS_READ_UNTIL, vec![])
			.await
			.unwrap()
			.map(|row| row.try_get("read_until").unwrap())
	}

	#[tokio::test]
	async fn a_query_waits_for_the_sweep_after_it() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::with(vec![query(T + 60, IP, "example.com")]);

		poll_at(&db, &mut log, T + 300).await.unwrap();
		assert!(domains(&db).await.is_empty(), "newer than the last sweep");
		assert_eq!(read_until(&db).await, Some(T));

		sweep(&db, T + EVERY, &[(IP, MAC_A)]).await;
		poll_at(&db, &mut log, T + EVERY + 120).await.unwrap();
		assert_eq!(queries_of(&db, MAC_A).await, 1);
		assert_eq!(read_until(&db).await, Some(T + EVERY));
	}

	#[tokio::test]
	async fn between_sweeps_the_server_gets_no_request() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::default();

		assert!(poll_at(&db, &mut log, T + 120).await.unwrap());
		for minute in 3..10 {
			assert!(!poll_at(&db, &mut log, T + minute * 60).await.unwrap());
		}
		assert_eq!(log.reads.len(), 1);
		assert_eq!(log.table_reads, 1);
	}

	#[tokio::test]
	async fn every_poll_counts_each_query_once() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		sweep(&db, T + EVERY, &[(IP, MAC_A)]).await;
		sweep(&db, T + 2 * EVERY, &[(IP, MAC_A)]).await;
		sweep(&db, T + 3 * EVERY, &[(IP, MAC_A)]).await;
		// on the seconds where the windows meet, too
		let mut log = FakeLog::with(vec![
			query(T + EVERY - 1, IP, "example.com"),
			query(T + EVERY, IP, "example.com"),
			query(T + 2 * EVERY - 1, IP, "example.com"),
			query(T + 2 * EVERY, IP, "example.com"),
		]);
		let mut until = T + EVERY;
		for _ in 0..3 {
			let window = Window {
				from: read_until(&db).await.unwrap_or(T),
				until,
			};
			read_window(&db, &mut log, window, None).await.unwrap();
			until += EVERY;
		}
		assert_eq!(queries_of(&db, MAC_A).await, 4);
	}

	#[tokio::test]
	async fn a_restart_picks_up_where_it_stopped() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		sweep(&db, T + EVERY, &[(IP, MAC_A)]).await;
		let queries = vec![query(T + 60, IP, "example.com")];

		poll_at(&db, &mut FakeLog::with(queries.clone()), T + EVERY + 120).await.unwrap();
		assert_eq!(queries_of(&db, MAC_A).await, 1);

		// a new adapter, the same database
		let mut log = FakeLog::with(queries);
		assert!(!poll_at(&db, &mut log, T + EVERY + 180).await.unwrap());
		assert!(log.reads.is_empty());
		assert_eq!(queries_of(&db, MAC_A).await, 1);
	}

	#[tokio::test]
	async fn a_server_that_fails_loses_nothing() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		sweep(&db, T + EVERY, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::with(vec![query(T + 60, IP, "example.com")]);

		for fail in [(true, false), (false, true)] {
			(log.fail_queries, log.fail_clients) = fail;
			assert!(poll_at(&db, &mut log, T + EVERY + 120).await.is_err());
			assert_eq!(read_until(&db).await, None);
			assert!(domains(&db).await.is_empty());
		}

		(log.fail_queries, log.fail_clients) = (false, false);
		poll_at(&db, &mut log, T + EVERY + 120).await.unwrap();
		assert_eq!(queries_of(&db, MAC_A).await, 1);
	}

	#[tokio::test]
	async fn a_long_gap_reads_a_day_at_most() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::default();
		poll_at(&db, &mut log, T + 120).await.unwrap();

		let later = T + 10 * 86_400;
		sweep(&db, later, &[(IP, MAC_A)]).await;
		poll_at(&db, &mut log, later + 120).await.unwrap();
		assert_eq!(log.reads.last(), Some(&(later - 86_400, later)));
	}

	#[tokio::test]
	async fn the_first_read_goes_back_a_day_at_most() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::default();
		poll_at(&db, &mut log, T + 120).await.unwrap();
		assert_eq!(log.reads, [(T - 86_400, T)]);
	}

	#[tokio::test]
	async fn with_no_sweeps_it_still_reads_and_only_the_table_can_credit() {
		let db = database().await;
		let mut log = FakeLog::with(vec![
			query(T - 300, IP, "example.com"),
			query(T - 300, OTHER_IP, "example.org"),
		]);
		log.clients = vec![DnsClient {
			ip: IP.parse().unwrap(),
			mac: Mac::new(MAC_B).unwrap(),
		}];
		poll_at(&db, &mut log, T).await.unwrap();
		assert_eq!(log.reads, [(T - 86_400 - READ_BEHIND_S, T - READ_BEHIND_S)]);
		assert_eq!(queries_of(&db, MAC_B).await, 1);
		assert_eq!(domains(&db).await.len(), 1);
	}

	#[tokio::test]
	async fn stopped_sweeps_stop_holding_queries_back() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::with(vec![
			query(T + 60, IP, "example.com"),
			query(T + LONE_SIGHTING_MAX_AGE_S + 1, IP, "example.org"),
		]);
		poll_at(&db, &mut log, T + 120).await.unwrap();
		assert_eq!(read_until(&db).await, Some(T), "waiting for the next sweep");

		let now = T + 2 * LONE_SIGHTING_MAX_AGE_S;
		poll_at(&db, &mut log, now).await.unwrap();
		assert_eq!(read_until(&db).await, Some(now - READ_BEHIND_S));
		// within the hour of the one sweep there is, and past it
		assert_eq!(
			domains(&db).await.keys().map(|(.., d)| d.as_str().to_owned()).collect::<Vec<_>>(),
			["example.com"]
		);
	}

	#[tokio::test]
	async fn sweeps_a_day_around_the_window_are_used_and_port_scans_are_not() {
		let db = database().await;
		// a later sweep names another MAC, far off but within a day: moved
		sweep(&db, T - SIGHTING_MAX_AGE_S, &[(IP, MAC_A)]).await;
		// the port scan's time says nothing, so it can't confirm a query
		sweep_kind(&db, "ports", T - 60, &[(OTHER_IP, MAC_B)]).await;
		sweep(&db, T, &[(IP, MAC_B)]).await;
		let mut log = FakeLog::with(vec![
			query(T - 60, IP, "example.com"),
			query(T - 60, OTHER_IP, "example.org"),
		]);
		poll_at(&db, &mut log, T + 120).await.unwrap();
		assert!(domains(&db).await.is_empty());
	}

	#[tokio::test]
	async fn days_are_totalled_before_the_join() {
		let db = database().await;
		let midnight = T;
		sweep(&db, midnight + 60, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::with(vec![
			query(midnight - 1, GATEWAY, "example.com"),
			query(midnight - 1, OTHER_IP, "example.com"),
			query(midnight, IP, "example.com"),
		]);
		poll_at(&db, &mut log, midnight + 120).await.unwrap();

		let days = |row: &sqlx::sqlite::SqliteRow| -> (i64, i64, i64) {
			(
				row.try_get("day").unwrap(),
				row.try_get("total").unwrap(),
				row.try_get("from_gateway").unwrap(),
			)
		};
		let rows = db
			.get_records("SELECT day, total, from_gateway FROM dns_days ORDER BY day", vec![])
			.await
			.unwrap();
		let day = day_of(midnight);
		assert_eq!(rows.iter().map(days).collect::<Vec<_>>(), [(day - 1, 2, 1), (day, 1, 0)]);
	}

	#[tokio::test]
	async fn the_counts_add_up_over_windows_of_one_day() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A), (GATEWAY, MAC_B)]).await;
		sweep(&db, T + EVERY, &[(IP, MAC_A), (GATEWAY, MAC_B)]).await;
		sweep(&db, T + 2 * EVERY, &[(IP, MAC_A), (GATEWAY, MAC_B)]).await;
		let mut log = FakeLog::with(vec![
			query(T + 60, IP, "example.com"),
			query(T + 60, GATEWAY, "example.com"),
			query(T + EVERY + 60, IP, "example.com"),
			query(T + EVERY + 60, GATEWAY, "example.com"),
		]);
		for until in [T + EVERY, T + 2 * EVERY] {
			let from = until - EVERY;
			read_window(
				&db,
				&mut log,
				Window {
					from,
					until,
				},
				GATEWAY.parse().ok(),
			)
			.await
			.unwrap();
		}
		assert_eq!(queries_of(&db, MAC_A).await, 2);
		let row = db
			.get_record("SELECT total, from_gateway FROM dns_days", vec![])
			.await
			.unwrap()
			.unwrap();
		assert_eq!(row.try_get::<i64, _>("total").unwrap(), 4);
		assert_eq!(row.try_get::<i64, _>("from_gateway").unwrap(), 2);
	}

	/// A window of five minutes, with sweeps on either side of its query.
	async fn credited_by_sweeps(hosts: &[(i64, &str)]) -> i64 {
		let db = database().await;
		for (at, mac) in hosts {
			sweep(&db, *at, &[(IP, mac)]).await;
		}
		let mut log = FakeLog::with(vec![query(T - 100, IP, "example.com")]);
		let window = Window {
			from: T - 300,
			until: T,
		};
		read_window(&db, &mut log, window, None).await.unwrap();
		domains(&db).await.values().sum()
	}

	#[tokio::test]
	async fn the_sweeps_are_looked_up_well_outside_the_window() {
		let hour = LONE_SIGHTING_MAX_AGE_S;
		// the later one closes the window; only with the earlier does it say moved
		assert_eq!(credited_by_sweeps(&[(T, MAC_B)]).await, 1);
		assert_eq!(credited_by_sweeps(&[(T - 2 * hour, MAC_A), (T, MAC_B)]).await, 0);
		// the earlier is too far to stand alone, and needs the one at `until` itself
		assert_eq!(credited_by_sweeps(&[(T - 2 * hour, MAC_A)]).await, 0);
		assert_eq!(credited_by_sweeps(&[(T - 2 * hour, MAC_A), (T, MAC_A)]).await, 1);
		// and the same with the later one past the window's end
		assert_eq!(credited_by_sweeps(&[(T - 2 * hour, MAC_A), (T + 2 * hour, MAC_A)]).await, 1);
	}

	#[tokio::test]
	async fn a_port_scan_does_not_open_the_window() {
		let db = database().await;
		sweep(&db, T, &[(IP, MAC_A)]).await;
		sweep_kind(&db, "ports", T + 300, &[(IP, MAC_A)]).await;
		poll_at(&db, &mut FakeLog::default(), T + 400).await.unwrap();
		assert_eq!(read_until(&db).await, Some(T));
	}

	#[tokio::test]
	async fn thirty_days_are_kept() {
		let db = database().await;
		let today = day_of(T);
		for day in [today - KEEP_DAYS - 1, today - KEEP_DAYS] {
			db.execute_all(vec![
				(
					DNS_DOMAINS_ADD,
					vec![
						DbValue::Text(MAC_A.into()),
						DbValue::Int(day),
						DbValue::Text("example.com".into()),
						DbValue::Int(1),
					],
				),
				(DNS_DAYS_ADD, vec![DbValue::Int(day), DbValue::Int(1), DbValue::Int(0)]),
			])
			.await
			.unwrap();
		}
		sweep(&db, T, &[(IP, MAC_A)]).await;
		poll_at(&db, &mut FakeLog::default(), T + 120).await.unwrap();

		let left: Vec<i64> = domains(&db).await.keys().map(|(_, day, _)| *day).collect();
		assert_eq!(left, [today - KEEP_DAYS]);
		let days = db.get_records("SELECT day FROM dns_days", vec![]).await.unwrap();
		assert_eq!(days.len(), 1);
		assert_eq!(days[0].try_get::<i64, _>("day").unwrap(), today - KEEP_DAYS);
	}

	#[tokio::test]
	async fn the_overview_shows_the_counts_only_with_a_dns_server() {
		use crate::core::{
			handlers::{overview::overview, report::store_report},
			models::{
				config::DnsSource,
				dns::{SHOWN_DAYS, day_of},
				report::{Report, ReportPayload},
			},
		};
		let db = database().await;
		let mut payload: ReportPayload = serde_json::from_str(&format!(
			r#"{{"sensor":"s","seq":1,"uptime_s":60,"channel":6,"dropped":0,"overflow":0,"nets_known":[0],
			"devices":[{{"mac":"{MAC_A}","net":0,"tx_frames":1,"tx_bytes":100,"rx_frames":1,"rx_bytes":100,"retries":0,"rssi_max":-50}}],
			"broadcast":[]}}"#
		))
		.unwrap();
		payload.received_at = T + 3 * EVERY;
		store_report(&db, &Report::try_from(payload).unwrap()).await.unwrap();
		sweep(&db, T, &[(IP, MAC_A)]).await;
		sweep(&db, T + EVERY, &[(IP, MAC_A)]).await;
		let mut log = FakeLog::with(vec![
			query(T + 60, IP, "a.example.com"),
			query(T + 120, IP, "b.example.com"),
			query(T + 180, IP, "example.org"),
		]);
		poll_at(&db, &mut log, T + EVERY + 2 * READ_BEHIND_S).await.unwrap();
		// a day too old to show
		let too_old = day_of(T) - SHOWN_DAYS;
		db.execute_all(vec![(
			DNS_DOMAINS_ADD,
			vec![
				DbValue::Text(MAC_A.into()),
				DbValue::Int(too_old),
				DbValue::Text("example.net".into()),
				DbValue::Int(50),
			],
		)])
		.await
		.unwrap();

		let o = overview(&db, None).await.unwrap();
		assert!(o.dns.is_none());
		assert!(o.devices[0].dns.is_none());

		let source = DnsSource::new("pihole+http://192.0.2.53", None).unwrap();
		let o = overview(&db, Some(&source)).await.unwrap();
		assert!(!o.dns.unwrap().router_forwards, "too few queries to say");
		let dns = o.devices[0].dns.as_ref().unwrap();
		let top: Vec<(&str, i64)> =
			dns.domains.top.iter().map(|d| (d.domain.as_str(), d.queries)).collect();
		assert_eq!(top, [("example.com", 2), ("example.org", 1)]);
		assert_eq!(dns.domains.more, 0);
		assert_eq!(dns.query_log.as_deref(), Some(source.query_log(IP).as_str()));
	}
}
