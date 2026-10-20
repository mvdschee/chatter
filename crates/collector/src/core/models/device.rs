//! A name the owner gives a device, shown instead of its sweep hostname or
//! its MAC. In as `DeviceNamePayload`, the MAC filled in from the path;
//! checked into a `DeviceName`. See `docs/floorplan.md`, chunk 9.

use serde::Deserialize;

use crate::{Error, Result, core::models::floorplan::is_mac};

const MAX_NAME: usize = 64;

#[derive(Debug, Deserialize)]
pub struct DeviceNamePayload {
	/// From the path.
	#[serde(skip)]
	pub mac: String,
	/// Empty or missing: the device goes back to its hostname or MAC.
	#[serde(default)]
	pub name: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct DeviceName {
	pub mac: String,
	/// Trimmed; `None` takes the name away.
	pub name: Option<String>,
}

impl TryFrom<DeviceNamePayload> for DeviceName {
	type Error = Error;

	fn try_from(p: DeviceNamePayload) -> Result<Self> {
		if !is_mac(&p.mac) {
			return Err(Error::Device(format!("{:?} isn't a MAC like 0a:1b:2c:3d:4e:5f", p.mac)));
		}
		let name = p.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
		if name.as_ref().is_some_and(|n| n.chars().count() > MAX_NAME) {
			return Err(Error::Device(format!("a name is at most {MAX_NAME} characters")));
		}
		Ok(Self {
			mac: p.mac,
			name,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const MAC: &str = "ea:5f:12:c4:9b:07";

	fn named(mac: &str, name: Option<&str>) -> Result<DeviceName> {
		DeviceName::try_from(DeviceNamePayload {
			mac: mac.into(),
			name: name.map(Into::into),
		})
	}

	#[test]
	fn a_name_is_trimmed_and_empty_takes_it_away() {
		assert_eq!(named(MAC, Some("  Desk lamp ")).unwrap().name.as_deref(), Some("Desk lamp"));
		assert_eq!(named(MAC, Some("   ")).unwrap().name, None);
		assert_eq!(named(MAC, None).unwrap().name, None);
	}

	#[test]
	fn a_bad_mac_or_a_long_name_is_refused() {
		assert!(named("EA:5F:12:C4:9B:07", Some("x")).is_err());
		assert!(named("ea:5f:12", Some("x")).is_err());
		assert!(named(MAC, Some(&"x".repeat(65))).is_err());
		assert!(named(MAC, Some(&"é".repeat(64))).is_ok());
	}
}
