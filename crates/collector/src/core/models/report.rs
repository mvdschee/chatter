//! A sensor's report: what it heard in one period (see `docs/sensor.md`,
//! "Report"). In as `ReportPayload`, the JSON the firmware posts; checked into
//! a `Report`.

use serde::Deserialize;

use crate::{Error, Result};

#[derive(Debug, Deserialize)]
pub struct ReportPayload {
	pub sensor: String,
	/// Random per boot; missing from firmware older than 2026-10-22.
	pub boot: Option<u32>,
	pub seq: u64,
	pub uptime_s: u64,
	pub channel: u8,
	pub dropped: u64,
	pub overflow: u64,
	pub nets_known: Vec<u8>,
	pub devices: Vec<DeviceMinutePayload>,
	pub broadcast: Vec<BroadcastPayload>,
	/// Not in the JSON: the web handler fills in when the report arrived, in
	/// unix seconds.
	#[serde(skip)]
	pub received_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct DeviceMinutePayload {
	pub mac: String,
	pub net: u8,
	pub tx_frames: u64,
	pub tx_bytes: u64,
	pub rx_frames: u64,
	pub rx_bytes: u64,
	pub retries: u64,
	pub rssi_max: Option<i8>,
}

#[derive(Debug, Deserialize)]
pub struct BroadcastPayload {
	pub net: u8,
	pub rx_frames: u64,
	pub rx_bytes: u64,
}

/// A sensor's name, as set in its firmware: 1 to 32 of `a-z`, `0-9`, `-`, `_`.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorName(String);

impl SensorName {
	pub fn new(raw: &str) -> Result<Self> {
		let ok = (1..=32).contains(&raw.len())
			&& raw
				.bytes()
				.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
		if !ok {
			return Err(Error::Report(format!("bad sensor name: {raw:?}")));
		}
		Ok(Self(raw.to_string()))
	}
}

impl std::ops::Deref for SensorName {
	type Target = String;
	fn deref(&self) -> &String {
		&self.0
	}
}

/// A MAC address as `aa:bb:cc:dd:ee:ff`, lower case.
#[derive(Debug, Clone, PartialEq)]
pub struct Mac(String);

impl Mac {
	pub fn new(raw: &str) -> Result<Self> {
		let mac = raw.to_ascii_lowercase();
		let ok = mac.len() == 17
			&& mac.split(':').count() == 6
			&& mac
				.split(':')
				.all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_hexdigit()));
		if !ok {
			return Err(Error::Report(format!("bad MAC: {raw:?}")));
		}
		Ok(Self(mac))
	}
}

impl std::ops::Deref for Mac {
	type Target = String;
	fn deref(&self) -> &String {
		&self.0
	}
}

/// Counters as SQLite stores them; a `u64` past `i64::MAX` can't be real.
fn count(name: &str, value: u64) -> Result<i64> {
	i64::try_from(value).map_err(|_| Error::Report(format!("{name} out of range: {value}")))
}

#[derive(Debug, Clone)]
pub struct Report {
	pub sensor: SensorName,
	pub boot: Option<i64>,
	pub seq: i64,
	pub uptime_s: i64,
	pub received_at: i64,
	pub channel: u8,
	pub dropped: i64,
	pub overflow: i64,
	pub nets_known: Vec<u8>,
	pub devices: Vec<DeviceMinute>,
	pub broadcast: Vec<Broadcast>,
}

#[derive(Debug, Clone)]
pub struct DeviceMinute {
	pub mac: Mac,
	pub net: u8,
	pub tx_frames: i64,
	pub tx_bytes: i64,
	pub rx_frames: i64,
	pub rx_bytes: i64,
	pub retries: i64,
	pub rssi_max: Option<i8>,
}

#[derive(Debug, Clone)]
pub struct Broadcast {
	pub net: u8,
	pub rx_frames: i64,
	pub rx_bytes: i64,
}

impl TryFrom<DeviceMinutePayload> for DeviceMinute {
	type Error = Error;

	fn try_from(p: DeviceMinutePayload) -> Result<Self> {
		Ok(Self {
			mac: Mac::new(&p.mac)?,
			net: p.net,
			tx_frames: count("tx_frames", p.tx_frames)?,
			tx_bytes: count("tx_bytes", p.tx_bytes)?,
			rx_frames: count("rx_frames", p.rx_frames)?,
			rx_bytes: count("rx_bytes", p.rx_bytes)?,
			retries: count("retries", p.retries)?,
			rssi_max: p.rssi_max,
		})
	}
}

impl TryFrom<BroadcastPayload> for Broadcast {
	type Error = Error;

	fn try_from(p: BroadcastPayload) -> Result<Self> {
		Ok(Self {
			net: p.net,
			rx_frames: count("rx_frames", p.rx_frames)?,
			rx_bytes: count("rx_bytes", p.rx_bytes)?,
		})
	}
}

impl TryFrom<ReportPayload> for Report {
	type Error = Error;

	fn try_from(p: ReportPayload) -> Result<Self> {
		// 2.4 GHz: channels 1 to 14; 0 until the sensor first connects
		if p.channel > 14 {
			return Err(Error::Report(format!("bad channel: {}", p.channel)));
		}
		Ok(Self {
			sensor: SensorName::new(&p.sensor)?,
			boot: p.boot.map(i64::from),
			seq: count("seq", p.seq)?,
			uptime_s: count("uptime_s", p.uptime_s)?,
			received_at: p.received_at,
			channel: p.channel,
			dropped: count("dropped", p.dropped)?,
			overflow: count("overflow", p.overflow)?,
			nets_known: p.nets_known,
			devices: p.devices.into_iter().map(DeviceMinute::try_from).collect::<Result<_>>()?,
			broadcast: p.broadcast.into_iter().map(Broadcast::try_from).collect::<Result<_>>()?,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// the shape core's `write_report` produces; made-up values
	const JSON: &str = r#"{"sensor":"c6-1","seq":42,"uptime_s":2520,"channel":6,"dropped":0,"overflow":0,"nets_known":[0,1],
		"devices":[{"mac":"AA:BB:CC:DD:EE:01","net":1,"tx_frames":10,"tx_bytes":1200,"rx_frames":8,"rx_bytes":9000,"retries":1,"rssi_max":-41},
			{"mac":"aa:bb:cc:dd:ee:02","net":0,"tx_frames":0,"tx_bytes":0,"rx_frames":3,"rx_bytes":300,"retries":0,"rssi_max":null}],
		"broadcast":[{"net":1,"rx_frames":40,"rx_bytes":5200}]}"#;

	fn payload() -> ReportPayload {
		serde_json::from_str(JSON).unwrap()
	}

	#[test]
	fn converts_a_firmware_report() {
		let mut p = payload();
		p.received_at = 1_700_000_000;
		let r = Report::try_from(p).unwrap();
		assert_eq!((r.sensor.as_str(), r.seq, r.received_at), ("c6-1", 42, 1_700_000_000));
		assert_eq!(r.devices[0].mac.as_str(), "aa:bb:cc:dd:ee:01");
		assert_eq!((r.devices[0].rssi_max, r.devices[1].rssi_max), (Some(-41), None));
		assert_eq!(r.broadcast[0].rx_bytes, 5200);
	}

	#[test]
	fn rejects_a_bad_mac() {
		let mut p = payload();
		p.devices[0].mac = "aa:bb:cc:dd:ee".into();
		assert!(Report::try_from(p).is_err());
		assert!(Mac::new("aa:bb:cc:dd:ee:zz").is_err());
		assert!(Mac::new("aabb:cc:dd:ee:ff:0").is_err());
	}

	#[test]
	fn rejects_a_bad_sensor_name() {
		for name in ["", "C6", "c6 1", &"x".repeat(33)] {
			assert!(SensorName::new(name).is_err(), "{name:?}");
		}
	}

	#[test]
	fn rejects_a_bad_channel_and_counters_past_i64() {
		let mut p = payload();
		p.channel = 15;
		assert!(Report::try_from(p).is_err());
		let mut p = payload();
		p.devices[0].rx_bytes = u64::MAX;
		assert!(Report::try_from(p).is_err());
	}
}
