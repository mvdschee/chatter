//! Identify sessions: started and stopped from the page, told to the sensor
//! in the answer to whatever it posts next. While one runs, the LAN is swept
//! every few seconds, so idle devices send a frame and the page knows who's
//! online.

use std::{
	collections::BTreeMap,
	net::IpAddr,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
	Result,
	core::{
		handlers::scan::current_targets,
		models::{
			identify::{
				HeardView, Identify, IdentifyView, Online, OnlineView, Readings, STALE_S,
				SWEEP_EVERY_S, SWEEP_STALE_S, SensorIdentify, SharedIdentify, Start,
			},
			report::SensorName,
			scan::{NmapRun, Scan, ScanTargets},
		},
	},
	error, info,
	services::cmd::nmap,
};

pub fn unix_now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Starts or extends a session, or ends it when `secs` is 0.
pub fn start_identify(state: &mut Identify, start: &Start, now: i64) {
	let sensor = start.sensor.to_string();
	if start.secs == 0 {
		state.until.remove(&sensor);
		state.readings.remove(&sensor);
		info!("identify {sensor}: stopped");
		return;
	}
	state.until.insert(sensor.clone(), now + start.secs as i64);
	info!("identify {sensor}: on for {} s", start.secs);
}

/// How many seconds the sensor has left to identify, `None` when it should
/// count. A session that ran out is forgotten here.
pub fn identify_left(state: &mut Identify, sensor: &SensorName, now: i64) -> Option<u64> {
	let until = *state.until.get(&**sensor)?;
	if until <= now {
		state.until.remove(&**sensor);
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

	let mut online: BTreeMap<String, Option<String>> =
		swept_hosts.iter().map(|h| (h.mac.to_string(), Some(h.ip.to_string()))).collect();
	let mut sensors: Vec<SensorIdentify> = state
		.until
		.iter()
		.filter(|&(_, &until)| until > now)
		.map(|(sensor, &until)| {
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
				online.entry(h.mac.clone()).or_insert(None);
			}
			SensorIdentify {
				sensor: sensor.clone(),
				left_s: (until - now) as u64,
				received_at: readings.map(|r| r.received_at),
				heard,
			}
		})
		.collect();
	sensors.sort_by(|a, b| a.sensor.cmp(&b.sensor));

	IdentifyView {
		sensors,
		online: online
			.into_iter()
			.map(|(mac, ip)| OnlineView {
				mac,
				ip,
			})
			.collect(),
		own_mac,
	}
}

/// While a session runs, sweeps the LAN every few seconds and keeps who
/// answered. A failed sweep is logged and the loop goes on, like the
/// regular sweeps.
pub async fn identify_sweep_loop(shared: SharedIdentify, configured: Option<ScanTargets>) {
	loop {
		tokio::time::sleep(Duration::from_secs(SWEEP_EVERY_S)).await;
		let now = unix_now();
		let active = {
			let state = shared.lock().unwrap_or_else(|e| e.into_inner());
			state.until.values().any(|&until| until > now)
		};
		if !active {
			continue;
		}
		match sweep_online(configured.as_ref()).await {
			Ok(online) => {
				let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
				state.online = online;
				state.swept_at = Some(unix_now());
			},
			Err(e) => error!("identify sweep: {e}"),
		}
	}
}

/// Who answers ARP now. Not stored: the regular sweeps do that.
async fn sweep_online(configured: Option<&ScanTargets>) -> Result<Vec<Online>> {
	let targets = current_targets(configured)?;
	let xml = nmap::arp_sweep(&targets).await?;
	let scan = Scan::from(NmapRun::from_xml(&xml)?);
	Ok(scan
		.hosts
		.into_iter()
		.map(|h| Online {
			mac: h.mac,
			ip: h.ip,
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
		assert!(s.until.is_empty());
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

	fn online(mac: &str, ip: &str) -> Online {
		Online {
			mac: Mac::new(mac).unwrap(),
			ip: ip.parse().unwrap(),
		}
	}

	fn view_state() -> Identify {
		let mut s = Identify::default();
		start_identify(&mut s, &start(600), 1000);
		s.online = vec![
			online("02:5e:a1:07:3c:9d", "192.0.2.41"),
			online("06:c4:18:9b:e2:50", "192.0.2.42"),
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
}
