//! Identify sessions: started and stopped from the page, told to the sensor
//! in the answer to whatever it posts next.

use crate::{
	core::models::{
		identify::{Identify, Readings, Start},
		report::SensorName,
	},
	info,
};

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
}
