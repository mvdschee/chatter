//! Pi-hole v6's REST API. Where the published spec is unclear, this follows
//! what FTL's `src/api/queries.c` and `src/api/network.c` do.

use reqwest::{Client, Response, StatusCode, header::CONTENT_TYPE};
use serde::Deserialize;

use crate::{
	Error, Result,
	core::models::{
		dns::{DnsClient, DnsQuery},
		report::Mac,
	},
	services::dns::DnsLog,
};

const SID_HEADER: &str = "X-FTL-SID";

/// FTL answers with at most this many queries, whatever is asked for. It walks
/// every matching row for each page, so few long pages beat many short ones.
const PAGE: usize = 10_000;

/// The defaults are 10 devices of 3 addresses, the most recently active ones:
/// a table cut short with nothing to say so.
const ALL_DEVICES: &str = "max_devices=100000&max_addresses=1000";

/// FTL's address for a loopback, which isn't a device.
const LOOPBACK_MAC: &str = "00:00:00:00:00:00";

pub struct PiHole {
	client: Client,
	base: String,
	password: Option<String>,
	/// `None` until a request is refused: a server without a password never
	/// asks for a session.
	sid: Option<String>,
}

impl PiHole {
	pub fn new(client: Client, base: &str, password: Option<String>) -> Self {
		Self {
			client,
			base: base.trim_end_matches('/').to_string(),
			password,
			sid: None,
		}
	}

	/// A refused request logs in and goes again, once. One session lasts for
	/// as long as it's used: logging in on every poll would fill the server's
	/// seats.
	async fn get(&mut self, path: &str) -> Result<Vec<u8>> {
		let mut answer = self.send(path).await?;
		if answer.status() == StatusCode::UNAUTHORIZED {
			self.login().await?;
			answer = self.send(path).await?;
		}
		let answer = answer.error_for_status().map_err(http_err)?;
		Ok(answer.bytes().await.map_err(http_err)?.to_vec())
	}

	async fn send(&self, path: &str) -> Result<Response> {
		let mut request = self.client.get(format!("{}{path}", self.base));
		if let Some(sid) = &self.sid {
			request = request.header(SID_HEADER, sid);
		}
		request.send().await.map_err(http_err)
	}

	async fn login(&mut self) -> Result<()> {
		let Some(password) = &self.password else {
			return Err(Error::Dns("Pi-hole asks for a password".into()));
		};
		let answer = self
			.client
			.post(format!("{}/api/auth", self.base))
			.header(CONTENT_TYPE, "application/json")
			.body(serde_json::json!({ "password": password }).to_string())
			.send()
			.await
			.map_err(http_err)?;
		match answer.status() {
			status if status.is_success() => {
				self.sid = parse_sid(&answer.bytes().await.map_err(http_err)?)?;
				Ok(())
			},
			StatusCode::UNAUTHORIZED => Err(Error::Dns(
				"Pi-hole refused the password (with 2FA on it takes an app password)".into(),
			)),
			StatusCode::TOO_MANY_REQUESTS => {
				Err(Error::Dns("Pi-hole's login: too many tries, or no seats left".into()))
			},
			status => Err(Error::Dns(format!("Pi-hole's login: {status}"))),
		}
	}
}

impl DnsLog for PiHole {
	/// Newest first, page by page. The answer's `cursor` doesn't move on: it
	/// fixes the first page, so that new queries don't shift the later ones,
	/// and `start` walks through them.
	async fn queries(&mut self, from: i64, until: i64) -> Result<Vec<DnsQuery>> {
		let mut queries = Vec::new();
		let mut cursor = None;
		let mut start = 0;
		loop {
			let at = cursor.map(|c| format!("&cursor={c}")).unwrap_or_default();
			let path =
				format!("/api/queries?from={from}&until={until}&length={PAGE}&start={start}{at}");
			let page = parse_queries(&self.get(&path).await?, from, until)?;
			queries.extend(page.queries);
			if page.rows < PAGE {
				return Ok(queries);
			}
			cursor = cursor.or(page.cursor);
			start += PAGE;
		}
	}

	async fn clients(&mut self) -> Result<Vec<DnsClient>> {
		parse_devices(&self.get(&format!("/api/network/devices?{ALL_DEVICES}")).await?)
	}

	async fn close(&mut self) -> Result<()> {
		let Some(sid) = self.sid.take() else {
			return Ok(());
		};
		self.client
			.delete(format!("{}/api/auth", self.base))
			.header(SID_HEADER, sid)
			.send()
			.await
			.and_then(Response::error_for_status)
			.map_err(http_err)?;
		Ok(())
	}
}

#[derive(Deserialize)]
struct AuthAnswer {
	session: AuthSession,
}

#[derive(Deserialize)]
struct AuthSession {
	/// Null when the server has no password.
	sid: Option<String>,
}

#[derive(Deserialize)]
struct QueriesAnswer {
	queries: Vec<QueryRow>,
	/// Null at privacy level 3, which also hides every query.
	cursor: Option<i64>,
}

#[derive(Deserialize)]
struct QueryRow {
	time: f64,
	domain: String,
	client: QueryClient,
}

#[derive(Deserialize)]
struct QueryClient {
	ip: String,
}

#[derive(Deserialize)]
struct DevicesAnswer {
	devices: Vec<Device>,
}

#[derive(Deserialize)]
struct Device {
	hwaddr: String,
	ips: Vec<DeviceIp>,
}

#[derive(Deserialize)]
struct DeviceIp {
	ip: String,
}

/// One answer of `/api/queries`. `rows` counts what the server sent, the
/// queries left out too: only that says whether another page follows.
struct Page {
	queries: Vec<DnsQuery>,
	rows: usize,
	cursor: Option<i64>,
}

fn parse_sid(body: &[u8]) -> Result<Option<String>> {
	let answer: AuthAnswer = parse(body)?;
	Ok(answer.session.sid)
}

fn parse_queries(body: &[u8], from: i64, until: i64) -> Result<Page> {
	let answer: QueriesAnswer = parse(body)?;
	let rows = answer.queries.len();
	let queries = answer
		.queries
		.into_iter()
		.filter_map(|row| {
			Some(DnsQuery {
				// the query's second, as `from` and `until` count them
				at: row.time.floor() as i64,
				// a query's client can be empty in a broken row
				client: row.client.ip.parse().ok()?,
				name: row.domain,
			})
		})
		.filter(|query| (from..until).contains(&query.at))
		.collect();
	Ok(Page {
		queries,
		rows,
		cursor: answer.cursor,
	})
}

fn parse_devices(body: &[u8]) -> Result<Vec<DnsClient>> {
	let answer: DevicesAnswer = parse(body)?;
	let mut clients = Vec::new();
	for device in answer.devices {
		// `ip-192.0.2.7` for a client whose MAC it can't see: not a MAC, so
		// that address goes on to the sweeps
		let Ok(mac) = Mac::new(&device.hwaddr) else {
			continue;
		};
		if *mac == LOOPBACK_MAC {
			continue;
		}
		clients.extend(device.ips.into_iter().filter_map(|ip| {
			Some(DnsClient {
				ip: ip.ip.parse().ok()?,
				mac: mac.clone(),
			})
		}));
	}
	Ok(clients)
}

fn parse<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T> {
	serde_json::from_slice(body).map_err(|e| Error::Dns(format!("Pi-hole's answer: {e}")))
}

fn http_err(e: reqwest::Error) -> Error {
	Error::Dns(format!("Pi-hole: {e}"))
}

#[cfg(test)]
mod tests {
	use std::{
		collections::{HashMap, HashSet},
		net::IpAddr,
		sync::{
			Arc, Mutex,
			atomic::{AtomicBool, AtomicUsize, Ordering},
		},
	};

	use axum::{
		Json, Router,
		body::Bytes,
		extract::{Query, State},
		http::HeaderMap,
		response::{IntoResponse, Response},
		routing::{get, post},
	};
	use serde_json::{Value, json};

	use super::*;
	use crate::{core::models::dns::domain_counts, services::http::create_client};

	const PASSWORD: &str = "made-up-secret";
	const SID: &str = "vT1q+Zp0Lk4Xs9Hc7Ry2Wg=";

	const MAC_A: &str = "e7:f2:5e:ef:88:30";
	const MAC_B: &str = "b5:11:ac:5e:9d:9c";

	/// A Pi-hole that behaves as FTL's source does on the parts the adapter
	/// relies on: sessions, the query filters and pages, the device limits.
	#[derive(Default)]
	struct Fake {
		password: Option<&'static str>,
		/// Oldest first; `id` is the place plus one.
		queries: Mutex<Vec<(f64, String, String)>>,
		/// A query arrives while the first page is being answered.
		query_arrives: AtomicBool,
		devices: Vec<Value>,
		sessions: Mutex<HashSet<String>>,
		logins: AtomicUsize,
		logouts: AtomicUsize,
		pages: AtomicUsize,
	}

	impl Fake {
		fn allowed(&self, headers: &HeaderMap) -> bool {
			self.password.is_none()
				|| headers
					.get(SID_HEADER)
					.and_then(|sid| sid.to_str().ok())
					.is_some_and(|sid| self.sessions.lock().unwrap().contains(sid))
		}

		fn expire_sessions(&self) {
			self.sessions.lock().unwrap().clear();
		}
	}

	fn refused() -> Response {
		(StatusCode::UNAUTHORIZED, Json(json!({ "error": { "key": "unauthorized" } })))
			.into_response()
	}

	async fn fake_login(State(fake): State<Arc<Fake>>, body: Bytes) -> Response {
		fake.logins.fetch_add(1, Ordering::SeqCst);
		let body: Value = serde_json::from_slice(&body).unwrap();
		if body["password"].as_str() != fake.password {
			return (
				StatusCode::UNAUTHORIZED,
				Json(json!({ "session": { "valid": false, "sid": null } })),
			)
				.into_response();
		}
		fake.sessions.lock().unwrap().insert(SID.to_string());
		Json(json!({ "session": { "valid": true, "sid": SID, "validity": 1800 } })).into_response()
	}

	async fn fake_logout(State(fake): State<Arc<Fake>>, headers: HeaderMap) -> Response {
		if !fake.allowed(&headers) {
			return refused();
		}
		fake.logouts.fetch_add(1, Ordering::SeqCst);
		fake.expire_sessions();
		StatusCode::NO_CONTENT.into_response()
	}

	async fn fake_queries(
		State(fake): State<Arc<Fake>>,
		headers: HeaderMap,
		Query(params): Query<HashMap<String, String>>,
	) -> Response {
		if !fake.allowed(&headers) {
			return refused();
		}
		fake.pages.fetch_add(1, Ordering::SeqCst);
		let number = |key: &str| params.get(key).map(|v| v.parse::<f64>().unwrap());
		let (from, until) = (number("from").unwrap(), number("until").unwrap());
		let length = number("length").unwrap_or(100.0).min(PAGE as f64) as usize;
		let start = number("start").unwrap_or(0.0) as usize;

		let mut all = fake.queries.lock().unwrap();
		let newest = all.len();
		let cursor = number("cursor").map_or(newest, |c| c as usize);
		let page: Vec<Value> = all
			.iter()
			.enumerate()
			.rev()
			.map(|(place, query)| (place + 1, query))
			.filter(|(id, (time, ..))| *id <= cursor && *time >= from && *time < until)
			.skip(start)
			.take(length)
			.map(|(id, (time, domain, ip))| {
				json!({
					"id": id, "time": time, "type": "A", "domain": domain, "cname": null,
					"status": "FORWARDED", "client": { "ip": ip, "name": null },
				})
			})
			.collect();
		if fake.query_arrives.swap(false, Ordering::SeqCst) {
			let (time, ..) = all[newest - 1];
			all.push((time, "arrived.example.org".into(), "192.0.2.9".into()));
		}
		Json(json!({ "queries": page, "cursor": cursor, "recordsTotal": newest })).into_response()
	}

	async fn fake_devices(
		State(fake): State<Arc<Fake>>,
		headers: HeaderMap,
		Query(params): Query<HashMap<String, usize>>,
	) -> Response {
		if !fake.allowed(&headers) {
			return refused();
		}
		let max_devices = params.get("max_devices").copied().unwrap_or(10);
		let max_addresses = params.get("max_addresses").copied().unwrap_or(3);
		let devices: Vec<Value> = fake
			.devices
			.iter()
			.take(max_devices)
			.map(|device| {
				let mut device = device.clone();
				let ips = device["ips"].as_array().unwrap();
				device["ips"] = ips.iter().take(max_addresses).cloned().collect();
				device
			})
			.collect();
		Json(json!({ "devices": devices })).into_response()
	}

	async fn serve(fake: Fake) -> (PiHole, Arc<Fake>) {
		let fake = Arc::new(fake);
		let app = Router::new()
			.route("/api/auth", post(fake_login).delete(fake_logout))
			.route("/api/queries", get(fake_queries))
			.route("/api/network/devices", get(fake_devices))
			.with_state(fake.clone());
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}/", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, app).await });
		let log = PiHole::new(create_client().unwrap(), &base, fake.password.map(str::to_string));
		(log, fake)
	}

	fn numbered_queries(count: usize, time: f64) -> Mutex<Vec<(f64, String, String)>> {
		Mutex::new(
			(0..count)
				.map(|n| (time, format!("n{n}.example.org"), "192.0.2.7".to_string()))
				.collect(),
		)
	}

	fn distinct_names(queries: &[DnsQuery]) -> usize {
		queries.iter().map(|q| &q.name).collect::<HashSet<_>>().len()
	}

	#[tokio::test]
	async fn every_query_comes_once_over_the_pages() {
		// one short of, exactly at and past a page boundary
		for count in [PAGE - 1, PAGE, 2 * PAGE + 7] {
			let (mut log, fake) = serve(Fake {
				queries: numbered_queries(count, 1_000.5),
				..Fake::default()
			})
			.await;
			let queries = log.queries(1_000, 1_001).await.unwrap();
			assert_eq!(queries.len(), count);
			assert_eq!(distinct_names(&queries), count);
			assert_eq!(fake.pages.load(Ordering::SeqCst), count / PAGE + 1);
		}
	}

	#[tokio::test]
	async fn a_query_arriving_while_paging_does_not_shift_the_pages() {
		let count = PAGE + 7;
		let (mut log, fake) = serve(Fake {
			queries: numbered_queries(count, 1_000.5),
			query_arrives: AtomicBool::new(true),
			..Fake::default()
		})
		.await;
		let queries = log.queries(1_000, 1_001).await.unwrap();
		assert_eq!(queries.len(), count);
		assert_eq!(distinct_names(&queries), count);
		assert_eq!(fake.queries.lock().unwrap().len(), count + 1, "one did arrive");
	}

	#[tokio::test]
	async fn windows_that_follow_each_other_read_every_query_once() {
		let (mut log, _fake) = serve(Fake {
			queries: Mutex::new(
				[999.9, 1_000.0, 1_000.5, 1_001.0, 1_001.99, 1_002.0]
					.iter()
					.enumerate()
					.map(|(n, &time)| (time, format!("n{n}.example.org"), "192.0.2.7".into()))
					.collect(),
			),
			..Fake::default()
		})
		.await;
		let mut first = log.queries(1_000, 1_001).await.unwrap();
		let second = log.queries(1_001, 1_002).await.unwrap();
		first.sort_by_key(|q| q.name.clone());
		assert_eq!(
			first.iter().map(|q| (q.at, q.name.as_str())).collect::<Vec<_>>(),
			[(1_000, "n1.example.org"), (1_000, "n2.example.org")]
		);
		assert_eq!(second.iter().map(|q| q.at).collect::<Vec<_>>(), [1_001, 1_001]);
	}

	#[test]
	fn a_page_stays_inside_the_window_whatever_the_server_sends() {
		let body = json!({ "cursor": 3, "queries": [
			{ "time": 1_000.0, "domain": "first.example.org", "client": { "ip": "192.0.2.7" } },
			{ "time": 1_001.0, "domain": "at-until.example.org", "client": { "ip": "192.0.2.7" } },
			{ "time": 999.9, "domain": "before.example.org", "client": { "ip": "192.0.2.7" } },
		]});
		let page = parse_queries(body.to_string().as_bytes(), 1_000, 1_001).unwrap();
		assert_eq!(page.rows, 3);
		assert_eq!(
			page.queries,
			[DnsQuery {
				at: 1_000,
				client: "192.0.2.7".parse().unwrap(),
				name: "first.example.org".into(),
			}]
		);
	}

	#[tokio::test]
	async fn a_full_page_of_broken_rows_is_still_a_full_page() {
		let mut rows = numbered_queries(PAGE + 1, 1_000.5).into_inner().unwrap();
		// the newest ones, which make up the first page
		for row in rows.iter_mut().skip(1) {
			row.2 = String::new();
		}
		let (mut log, _fake) = serve(Fake {
			queries: Mutex::new(rows),
			..Fake::default()
		})
		.await;
		let queries = log.queries(1_000, 1_001).await.unwrap();
		assert_eq!(queries.iter().map(|q| q.name.as_str()).collect::<Vec<_>>(), ["n0.example.org"]);
	}

	#[test]
	fn the_answer_at_privacy_level_3_is_no_queries() {
		let body = json!({ "queries": [], "cursor": null, "recordsTotal": 0 });
		let page = parse_queries(body.to_string().as_bytes(), 0, 1).unwrap();
		assert_eq!((page.queries.len(), page.rows, page.cursor), (0, 0, None));
	}

	#[tokio::test]
	async fn one_login_lasts_while_the_session_does() {
		let (mut log, fake) = serve(Fake {
			password: Some(PASSWORD),
			..Fake::default()
		})
		.await;
		for _ in 0..3 {
			log.queries(0, 1).await.unwrap();
			log.clients().await.unwrap();
		}
		assert_eq!(fake.logins.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn an_expired_session_logs_in_again_once() {
		let (mut log, fake) = serve(Fake {
			password: Some(PASSWORD),
			queries: numbered_queries(1, 1_000.5),
			..Fake::default()
		})
		.await;
		log.queries(1_000, 1_001).await.unwrap();
		fake.expire_sessions();
		assert_eq!(log.queries(1_000, 1_001).await.unwrap().len(), 1);
		assert_eq!(fake.logins.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn a_wrong_password_is_an_error_and_not_a_loop() {
		let (log, fake) = serve(Fake {
			password: Some(PASSWORD),
			..Fake::default()
		})
		.await;
		let mut log = PiHole {
			password: Some("not-the-password".into()),
			..log
		};
		let err = log.queries(0, 1).await.unwrap_err().to_string();
		assert!(err.contains("refused the password"), "{err}");
		assert!(!err.contains("not-the-password"), "{err}");
		assert_eq!(fake.logins.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn a_session_refused_right_after_login_is_an_error() {
		// a login the server accepts, for a session it doesn't honour
		let app = Router::new()
			.route(
				"/api/auth",
				post(async || Json(json!({ "session": { "valid": true, "sid": SID } }))),
			)
			.route("/api/queries", get(async || refused()));
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, app).await });
		let mut log = PiHole::new(create_client().unwrap(), &base, Some(PASSWORD.into()));
		let err = log.queries(0, 1).await.unwrap_err().to_string();
		assert!(err.contains("401"), "{err}");
		assert!(!err.contains(SID), "{err}");
	}

	#[tokio::test]
	async fn a_server_that_asks_for_a_password_gets_told_there_is_none() {
		let (log, _fake) = serve(Fake {
			password: Some(PASSWORD),
			..Fake::default()
		})
		.await;
		let mut log = PiHole {
			password: None,
			..log
		};
		let err = log.queries(0, 1).await.unwrap_err().to_string();
		assert!(err.contains("asks for a password"), "{err}");
	}

	#[tokio::test]
	async fn a_server_without_a_password_needs_no_login() {
		let (mut log, fake) = serve(Fake {
			queries: numbered_queries(1, 1_000.5),
			..Fake::default()
		})
		.await;
		assert_eq!(log.queries(1_000, 1_001).await.unwrap().len(), 1);
		assert_eq!(fake.logins.load(Ordering::SeqCst), 0);
		log.close().await.unwrap();
		assert_eq!(fake.logouts.load(Ordering::SeqCst), 0);
	}

	#[test]
	fn no_password_server_answers_with_a_null_session_id() {
		let body = json!({ "session": { "valid": true, "sid": null, "validity": -1 } });
		assert_eq!(parse_sid(body.to_string().as_bytes()).unwrap(), None);
	}

	#[tokio::test]
	async fn closing_gives_the_session_back_once() {
		let (mut log, fake) = serve(Fake {
			password: Some(PASSWORD),
			..Fake::default()
		})
		.await;
		log.queries(0, 1).await.unwrap();
		log.close().await.unwrap();
		log.close().await.unwrap();
		assert_eq!(fake.logouts.load(Ordering::SeqCst), 1);
		assert!(fake.sessions.lock().unwrap().is_empty());
	}

	#[tokio::test]
	async fn the_table_holds_every_device_and_address() {
		// past FTL's defaults of 10 devices and 3 addresses each
		let devices = (0..12)
			.map(|n| {
				json!({ "hwaddr": format!("5c:34:82:e3:c5:{n:02x}"), "ips": [
					{ "ip": format!("192.0.2.{n}") },
					{ "ip": format!("2001:db8::{n:x}:1") },
					{ "ip": format!("2001:db8::{n:x}:2") },
					{ "ip": format!("2001:db8::{n:x}:3") },
				]})
			})
			.collect();
		let (mut log, _fake) = serve(Fake {
			devices,
			..Fake::default()
		})
		.await;
		assert_eq!(log.clients().await.unwrap().len(), 12 * 4);
	}

	#[test]
	fn the_table_keeps_real_macs_only() {
		let body = json!({ "devices": [
			{
				"id": 1, "hwaddr": MAC_A.to_uppercase(), "interface": "eth0",
				"firstSeen": 1_664_623_620, "lastQuery": 1_664_688_620, "numQueries": 585,
				"macVendor": null,
				"ips": [
					{ "ip": "192.0.2.7", "name": "a-host", "lastSeen": 1_664_688_620 },
					{ "ip": "2001:db8::7", "name": null, "lastSeen": 1_664_688_620 },
					{ "ip": "not-an-address", "name": null, "lastSeen": 1_664_688_620 },
				],
			},
			{ "hwaddr": MAC_B, "ips": [] },
			{ "hwaddr": "ip-192.0.2.8", "ips": [{ "ip": "192.0.2.8" }] },
			{ "hwaddr": LOOPBACK_MAC, "ips": [{ "ip": "127.0.0.1" }] },
		]});
		let clients = parse_devices(body.to_string().as_bytes()).unwrap();
		let mac = Mac::new(MAC_A).unwrap();
		assert_eq!(
			clients,
			[
				DnsClient {
					ip: "192.0.2.7".parse().unwrap(),
					mac: mac.clone(),
				},
				DnsClient {
					ip: "2001:db8::7".parse().unwrap(),
					mac,
				},
			]
		);
	}

	/// The check against a real server, with `CHATTER_DNS` and
	/// `CHATTER_DNS_PASSWORD` in `.env`. Prints counts only, nothing about
	/// who looked up what:
	/// `cargo test --manifest-path crates/collector/Cargo.toml pihole::tests::live -- --ignored --nocapture`
	#[tokio::test]
	#[ignore = "needs a real Pi-hole"]
	async fn live() {
		let _ = dotenvy::dotenv();
		let dns = std::env::var("CHATTER_DNS").expect("CHATTER_DNS in .env");
		let base = dns.strip_prefix("pihole+").expect("CHATTER_DNS: pihole+http://…");
		let password = std::env::var("CHATTER_DNS_PASSWORD").ok();
		let mut log = PiHole::new(create_client().unwrap(), base, password);

		// a minute back, so that the windows are all in the past
		let until = chrono::Utc::now().timestamp() - 60;
		let from = until - 3_600;
		let middle = from + 1_800;
		let queries = log.queries(from, until).await.unwrap();
		let older = log.queries(from, middle).await.unwrap();
		let newer = log.queries(middle, until).await.unwrap();
		assert_eq!(older.len() + newer.len(), queries.len(), "windows overlap or leave a gap");
		assert!(queries.iter().all(|q| (from..until).contains(&q.at)));

		let clients = log.clients().await.unwrap();
		let table: HashSet<IpAddr> = clients.iter().map(|c| c.ip.to_canonical()).collect();
		let asking: HashSet<IpAddr> = queries.iter().map(|q| q.client.to_canonical()).collect();
		let counts = domain_counts(&queries, &clients, &[]);
		println!("queries in the last hour: {}", queries.len());
		println!("addresses asking: {}, in the table: {}", asking.len(), table.len());
		println!("asking and in the table: {}", asking.intersection(&table).count());
		println!(
			"queries from a table address: {}",
			queries.iter().filter(|q| table.contains(&q.client.to_canonical())).count()
		);
		println!(
			"counted: {}, devices: {}",
			counts.iter().map(|c| c.queries).sum::<i64>(),
			counts.iter().map(|c| &c.mac).collect::<HashSet<_>>().len()
		);
		log.close().await.unwrap();
	}
}
