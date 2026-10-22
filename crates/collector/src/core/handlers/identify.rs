//! Identify sessions: started and stopped from the page, told to the sensor
//! in the answer to whatever it posts next. While one runs, the LAN is swept
//! every few seconds, so idle devices send a frame and the page knows who's
//! online.

use std::{
	collections::{BTreeMap, HashMap},
	net::IpAddr,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::Row;

use crate::{
	Result,
	core::{
		handlers::scan::current_targets,
		models::{
			identify::{
				HeardView, Identify, IdentifyView, Online, OnlineView, Readings, STALE_S,
				SWEEP_EVERY_S, SWEEP_STALE_S, SensorIdentify, Session, SharedIdentify, Start,
			},
			report::SensorName,
			scan::{NmapRun, Scan, ScanTargets},
		},
	},
	error, info,
	repository::{
		db::{DatabaseRepository, DbValue},
		schemas::{IDENTIFY_SESSIONS_UPSERT, SCAN_HOSTS_LATEST},
	},
	services::cmd::nmap,
};

pub fn unix_now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Starts or extends a session, or ends it when `secs` is 0. Returns the
/// session to store as it now stands; `None` for stopping a sensor that
/// isn't in one.
pub fn start_identify(state: &mut Identify, start: &Start, now: i64) -> Option<Session> {
	let sensor = start.sensor.to_string();
	let running = state.sessions.get(&sensor).filter(|s| s.ended_at > now).cloned();
	if start.secs == 0 {
		state.sessions.remove(&sensor);
		state.readings.remove(&sensor);
		info!("identify {sensor}: stopped");
		return running.map(|s| Session {
			ended_at: now,
			..s
		});
	}
	info!("identify {sensor}: on for {} s", start.secs);
	// a new session starts with nothing heard: the last one's readings stay
	// until the sensor next asks, and would look like this one's
	if running.is_none() {
		state.readings.remove(&sensor);
	}
	let session = Session {
		ended_at: now + start.secs as i64,
		..running.unwrap_or(Session {
			sensor: sensor.clone(),
			started_at: now,
			ended_at: now,
		})
	};
	state.sessions.insert(sensor, session.clone());
	Some(session)
}

/// Stores a session, or its new end: one row per session.
pub async fn store_session(db: &DatabaseRepository, s: &Session) -> Result<()> {
	db.execute_all(vec![(
		IDENTIFY_SESSIONS_UPSERT,
		vec![DbValue::Text(s.sensor.clone()), DbValue::Int(s.started_at), DbValue::Int(s.ended_at)],
	)])
	.await
}

/// How many seconds the sensor has left to identify, `None` when it should
/// count. A session that ran out is forgotten here.
pub fn identify_left(state: &mut Identify, sensor: &SensorName, now: i64) -> Option<u64> {
	let until = state.sessions.get(&**sensor)?.ended_at;
	if until <= now {
		state.sessions.remove(&**sensor);
		state.readings.remove(&**sensor);
		info!("identify {}: time is up", &**sensor);
		return None;
	}
	Some((until - now) as u64)
}

/// Keeps the newest readings. Readings from a sensor that isn't in a session
/// are dropped: the answer tells it to stop.
pub fn record_readings(state: &mut Identify, readings: Readings, now: i64) -> Option<u64> {
	let left = identify_left(state, &readings.sensor, now)?;
	state.readings.insert(readings.sensor.to_string(), readings);
	Some(left)
}

/// The sessions, what their sensors hear now, and who's online, for the page
/// open on `own_ip`. Changes nothing: a session that ran out is only left
/// out here.
pub fn identify_view(state: &Identify, own_ip: Option<IpAddr>, now: i64) -> IdentifyView {
	let swept = state.swept_at.is_some_and(|at| now - at <= SWEEP_STALE_S);
	let swept_hosts = if swept {
		&state.online[..]
	} else {
		&[]
	};
	let own_mac =
		own_ip.and_then(|ip| swept_hosts.iter().find(|h| h.ip == ip)).map(|h| h.mac.to_string());

	let mut online: BTreeMap<String, OnlineView> = swept_hosts
		.iter()
		.map(|h| {
			(
				h.mac.to_string(),
				OnlineView {
					mac: h.mac.to_string(),
					ip: Some(h.ip.to_string()),
					hostname: h.hostname.clone(),
					vendor: h.vendor.clone(),
				},
			)
		})
		.collect();
	let mut sensors: Vec<SensorIdentify> = state
		.sessions
		.iter()
		.filter(|(_, s)| s.ended_at > now)
		.map(|(sensor, s)| {
			let readings = state.readings.get(sensor);
			let fresh = readings.filter(|r| now - r.received_at <= STALE_S);
			let heard: Vec<HeardView> = fresh
				.map(|r| &r.heard[..])
				.unwrap_or_default()
				.iter()
				.filter(|h| own_mac.as_deref() != Some(&*h.mac))
				.map(|h| HeardView {
					mac: h.mac.to_string(),
					rssi_max: h.rssi_max,
					frames: h.frames,
				})
				.collect();
			for h in &heard {
				online.entry(h.mac.clone()).or_insert_with(|| OnlineView {
					mac: h.mac.clone(),
					ip: None,
					hostname: None,
					vendor: None,
				});
			}
			SensorIdentify {
				sensor: sensor.clone(),
				left_s: (s.ended_at - now) as u64,
				received_at: readings.map(|r| r.received_at),
				heard,
			}
		})
		.collect();
	sensors.sort_by(|a, b| a.sensor.cmp(&b.sensor));

	IdentifyView {
		sensors,
		online: online.into_values().collect(),
		own_mac,
	}
}

/// While a session runs, sweeps the LAN every few seconds and keeps who
/// answered. A failed sweep is logged and the loop goes on, like the
/// regular sweeps.
pub async fn identify_sweep_loop(
	db: DatabaseRepository,
	shared: SharedIdentify,
	configured: Option<ScanTargets>,
) {
	loop {
		tokio::time::sleep(Duration::from_secs(SWEEP_EVERY_S)).await;
		let now = unix_now();
		let active = {
			let state = shared.lock().unwrap_or_else(|e| e.into_inner());
			state.sessions.values().any(|s| s.ended_at > now)
		};
		if !active {
			continue;
		}
		match sweep_online(&db, configured.as_ref()).await {
			Ok(online) => {
				let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
				state.online = online;
				state.swept_at = Some(unix_now());
			},
			Err(e) => error!("identify sweep: {e}"),
		}
	}
}

/// Who answers ARP now, with their hostnames from the stored sweeps. Not
/// stored: the regular sweeps do that.
async fn sweep_online(
	db: &DatabaseRepository,
	configured: Option<&ScanTargets>,
) -> Result<Vec<Online>> {
	let targets = current_targets(configured)?;
	let xml = nmap::arp_sweep(&targets).await?;
	let scan = Scan::from(NmapRun::from_xml(&xml)?);
	let mut hostnames: HashMap<String, String> = HashMap::new();
	for row in db.get_records(SCAN_HOSTS_LATEST, vec![]).await? {
		if let Some(hostname) = row.try_get::<Option<String>, _>("hostname")? {
			hostnames.insert(row.try_get("mac")?, hostname);
		}
	}
	Ok(scan
		.hosts
		.into_iter()
		.map(|h| Online {
			hostname: hostnames.remove(&*h.mac),
			mac: h.mac,
			ip: h.ip,
			vendor: h.vendor,
		})
		.collect())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::models::{identify::Heard, report::Mac};

	fn sensor() -> SensorName {
		SensorName::new("c6-test").unwrap()
	}

	fn start(secs: u64) -> Start {
		Start {
			sensor: sensor(),
			secs,
		}
	}

	fn readings(at: i64) -> Readings {
		Readings {
			sensor: sensor(),
			heard: vec![Heard {
				mac: Mac::new("02:5e:a1:07:3c:9d").unwrap(),
				rssi_max: -35,
				frames: 4,
			}],
			received_at: at,
		}
	}

	#[test]
	fn counts_without_a_session() {
		let mut s = Identify::default();
		assert_eq!(identify_left(&mut s, &sensor(), 1000), None);
	}

	#[test]
	fn on_until_the_time_is_up() {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		assert_eq!(identify_left(&mut s, &sensor(), 1000), Some(600));
		assert_eq!(identify_left(&mut s, &sensor(), 1599), Some(1));
		assert_eq!(identify_left(&mut s, &sensor(), 1600), None);
		// and stays off
		assert!(s.sessions.is_empty());
	}

	#[test]
	fn only_the_sensor_named() {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		let other = SensorName::new("c6-other").unwrap();
		assert_eq!(identify_left(&mut s, &other, 1000), None);
	}

	#[test]
	fn a_new_start_extends_and_zero_stops() {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		start_identify(&mut s, &start(600), 1500);
		assert_eq!(identify_left(&mut s, &sensor(), 1500), Some(600));
		record_readings(&mut s, readings(1500), 1500);
		start_identify(&mut s, &start(0), 1510);
		assert_eq!(identify_left(&mut s, &sensor(), 1510), None);
		assert!(s.readings.is_empty());
	}

	#[test]
	fn readings_kept_only_in_a_session() {
		let mut s = Identify::default();
		assert_eq!(record_readings(&mut s, readings(1000), 1000), None);
		assert!(s.readings.is_empty());

		start_identify(&mut s, &start(600), 1000);
		assert_eq!(record_readings(&mut s, readings(1003), 1003), Some(597));
		assert_eq!(record_readings(&mut s, readings(1006), 1006), Some(594));
		assert_eq!(s.readings["c6-test"].received_at, 1006);
	}

	#[test]
	fn a_new_session_starts_with_nothing_heard() {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		record_readings(&mut s, readings(1003), 1003);
		// it ran out, and the sensor hasn't asked since
		start_identify(&mut s, &start(600), 1700);
		assert!(s.readings.is_empty());
		// extending keeps what it hears
		record_readings(&mut s, readings(1703), 1703);
		start_identify(&mut s, &start(600), 1704);
		assert_eq!(s.readings["c6-test"].received_at, 1703);
	}

	fn online(mac: &str, ip: &str) -> Online {
		Online {
			mac: Mac::new(mac).unwrap(),
			ip: ip.parse().unwrap(),
			hostname: None,
			vendor: None,
		}
	}

	fn view_state() -> Identify {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		s.online = vec![
			online("02:5e:a1:07:3c:9d", "192.0.2.41"),
			Online {
				hostname: Some("printer.lan".into()),
				vendor: Some("Example Corp".into()),
				..online("06:c4:18:9b:e2:50", "192.0.2.42")
			},
		];
		s.swept_at = Some(1000);
		let mut r = readings(1000);
		r.heard.push(Heard {
			mac: Mac::new("0a:73:2f:d6:41:b8").unwrap(),
			rssi_max: -60,
			frames: 2,
		});
		record_readings(&mut s, r, 1000);
		s
	}

	fn heard_macs(v: &IdentifyView) -> Vec<&str> {
		v.sensors[0].heard.iter().map(|h| h.mac.as_str()).collect()
	}

	#[test]
	fn view_of_a_session() {
		let v = identify_view(&view_state(), None, 1002);
		assert_eq!(v.sensors.len(), 1);
		assert_eq!((v.sensors[0].left_s, v.sensors[0].received_at), (598, Some(1000)));
		assert_eq!(heard_macs(&v), ["02:5e:a1:07:3c:9d", "0a:73:2f:d6:41:b8"]);
		// swept, plus heard but not swept
		let online: Vec<_> = v.online.iter().map(|o| (o.mac.as_str(), o.ip.as_deref())).collect();
		assert_eq!(
			online,
			[
				("02:5e:a1:07:3c:9d", Some("192.0.2.41")),
				("06:c4:18:9b:e2:50", Some("192.0.2.42")),
				("0a:73:2f:d6:41:b8", None),
			]
		);
		// what's known of a swept host goes with it
		assert_eq!(v.online[1].hostname.as_deref(), Some("printer.lan"));
		assert_eq!(v.online[1].vendor.as_deref(), Some("Example Corp"));
		assert_eq!(v.own_mac, None);
	}

	#[test]
	fn the_pages_own_device_is_left_out() {
		let v = identify_view(&view_state(), Some("192.0.2.41".parse().unwrap()), 1002);
		assert_eq!(v.own_mac.as_deref(), Some("02:5e:a1:07:3c:9d"));
		assert_eq!(heard_macs(&v), ["0a:73:2f:d6:41:b8"]);
	}

	#[test]
	fn stale_readings_and_sweeps_say_nothing() {
		let s = view_state();
		let v = identify_view(&s, None, 1000 + STALE_S + 1);
		assert!(v.sensors[0].heard.is_empty());
		// still says when it last heard from the sensor
		assert_eq!(v.sensors[0].received_at, Some(1000));
		assert_eq!(v.online.len(), 2);

		let v = identify_view(&s, Some("192.0.2.41".parse().unwrap()), 1000 + SWEEP_STALE_S + 1);
		assert!(v.online.is_empty());
		assert_eq!(v.own_mac, None);
	}

	#[test]
	fn no_session_no_sensors() {
		let s = view_state();
		assert!(identify_view(&s, None, 1600).sensors.is_empty());
		assert!(identify_view(&Identify::default(), None, 1000).sensors.is_empty());
	}

	#[test]
	fn sessions_to_store() {
		let mut s = Identify::default();
		let started = |ended_at| Session {
			sensor: "c6-test".into(),
			started_at: 1000,
			ended_at,
		};
		assert_eq!(start_identify(&mut s, &start(600), 1000), Some(started(1600)));
		// extended: the same session, a later end
		assert_eq!(start_identify(&mut s, &start(600), 1300), Some(started(1900)));
		// stopped early: it ends now
		assert_eq!(start_identify(&mut s, &start(0), 1400), Some(started(1400)));
		assert_eq!(start_identify(&mut s, &start(0), 1410), None);
	}

	#[test]
	fn a_start_after_one_ran_out_is_a_new_session() {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		let again = start_identify(&mut s, &start(600), 2000).unwrap();
		assert_eq!((again.started_at, again.ended_at), (2000, 2600));
		// and stopping one that ran out stores nothing
		start_identify(&mut s, &start(60), 3000);
		assert_eq!(start_identify(&mut s, &start(0), 3100), None);
	}
}
