//! Identify mode (see `docs/identify.md`): which sensors are in it until
//! when, and the newest devices each one heard. Kept in memory only: a
//! session is minutes long, and the readings are only worth anything live.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use serde::Deserialize;

use crate::{
	Error, Result,
	core::models::report::{Mac, SensorName},
};

/// The longest session a start asks for: an hour.
pub const MAX_SECS: u64 = 3600;

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

#[derive(Debug, Default)]
pub struct Identify {
	/// Per sensor, until when it identifies, in unix seconds.
	pub until: HashMap<String, i64>,
	/// Per sensor, the newest readings.
	pub readings: HashMap<String, Readings>,
}

/// Shared by the web handlers; held only to read or change it, never across
/// an await.
pub type SharedIdentify = Arc<Mutex<Identify>>;
