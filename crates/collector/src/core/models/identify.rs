//! Identify mode (see `docs/identify.md`): which sensors are in it until
//! when, the newest devices each one heard, and who answered the newest
//! sweep. Kept in memory only: a session is minutes long, and the readings
//! are only worth anything live.

use std::{
	collections::HashMap,
	net::IpAddr,
	sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use crate::{
	Error, Result,
	core::models::report::{Mac, SensorName},
};

/// The longest session a start asks for: an hour.
pub const MAX_SECS: u64 = 3600;
/// Readings this old are stale: the sensor missed a few posts (it posts
/// every 3 s), so what it heard says nothing about now.
pub const STALE_S: i64 = 10;
/// How often a session sweeps the LAN, so idle devices send a frame.
pub const SWEEP_EVERY_S: u64 = 5;
/// A sweep this old no longer says who's online.
pub const SWEEP_STALE_S: i64 = 30;

/// Starts a session for `secs` seconds, or ends it with 0. The sensor is in
/// the path.
#[derive(Debug, Deserialize)]
pub struct StartPayload {
	pub secs: u64,
	#[serde(skip)]
	pub sensor: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Start {
	pub sensor: SensorName,
	pub secs: u64,
}

impl TryFrom<StartPayload> for Start {
	type Error = Error;

	fn try_from(p: StartPayload) -> Result<Self> {
		if p.secs > MAX_SECS {
			return Err(Error::Identify(format!("at most {MAX_SECS} s, not {}", p.secs)));
		}
		Ok(Self {
			sensor: SensorName::new(&p.sensor)?,
			secs: p.secs,
		})
	}
}

/// What a sensor in identify mode posts every few seconds, strongest first.
#[derive(Debug, Deserialize)]
pub struct ReadingsPayload {
	pub sensor: String,
	pub devices: Vec<HeardPayload>,
	/// Not in the JSON: the web handler fills in when it arrived, in unix
	/// seconds.
	#[serde(skip)]
	pub received_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct HeardPayload {
	pub mac: String,
	pub rssi_max: i8,
	pub frames: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Readings {
	pub sensor: SensorName,
	pub heard: Vec<Heard>,
	pub received_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
	pub mac: Mac,
	pub rssi_max: i8,
	pub frames: u64,
}

impl TryFrom<ReadingsPayload> for Readings {
	type Error = Error;

	fn try_from(p: ReadingsPayload) -> Result<Self> {
		let heard = p
			.devices
			.into_iter()
			.map(|h| {
				Ok(Heard {
					mac: Mac::new(&h.mac)?,
					rssi_max: h.rssi_max,
					frames: h.frames,
				})
			})
			.collect::<Result<_>>()?;
		Ok(Self {
			sensor: SensorName::new(&p.sensor)?,
			heard,
			received_at: p.received_at,
		})
	}
}

/// A host that answered a sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct Online {
	pub mac: Mac,
	pub ip: IpAddr,
}

#[derive(Debug, Default)]
pub struct Identify {
	/// Per sensor, until when it identifies, in unix seconds.
	pub until: HashMap<String, i64>,
	/// Per sensor, the newest readings.
	pub readings: HashMap<String, Readings>,
	/// Who answered the newest sweep, and when it finished.
	pub online: Vec<Online>,
	pub swept_at: Option<i64>,
}

/// Shared by the web handlers; held only to read or change it, never across
/// an await.
pub type SharedIdentify = Arc<Mutex<Identify>>;

/// What the identify page shows, out as JSON (`GET /v1/identify`).
#[derive(Debug, Serialize, PartialEq)]
pub struct IdentifyView {
	/// The sensors in a session, by name.
	pub sensors: Vec<SensorIdentify>,
	/// Who answered the newest sweep, and anything a sensor hears now, by
	/// MAC.
	pub online: Vec<OnlineView>,
	/// The device the page is open on, found by its IP in the newest sweep.
	/// It's left out of what's heard: held against the sensor, it would
	/// always come first.
	pub own_mac: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SensorIdentify {
	pub sensor: String,
	pub left_s: u64,
	/// When its newest readings came in; `None` before its first post.
	pub received_at: Option<i64>,
	/// Strongest first; empty when the readings are stale.
	pub heard: Vec<HeardView>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct HeardView {
	pub mac: String,
	pub rssi_max: i8,
	pub frames: u64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct OnlineView {
	pub mac: String,
	/// `None` when it's only heard, not swept.
	pub ip: Option<String>,
}
