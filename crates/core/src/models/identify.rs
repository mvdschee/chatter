//! What the sensor hears in identify mode: per device, its strongest signal
//! and how many frames it sent in the last few seconds, posted strongest
//! first.

use core::fmt::{self, Write};

use crate::models::{
	frame::Mac,
	report::{BufferFull, Out, REPORT_BUF},
};

/// Room for every device that talks within one post, so the one next to the
/// sensor gets a slot; when it's full, a stronger newcomer replaces the
/// weakest (see `handlers::identify`).
pub const MAX_HEARD: usize = 64;
/// Devices per post: the strongest are the only ones worth showing.
pub const MAX_SENT: usize = 16;
/// Fits a post of `MAX_SENT` devices with every field at its widest.
pub const IDENTIFY_BUF: usize = 2 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
	pub mac: Mac,
	/// Strongest signal from the device itself (uplink frames only).
	pub rssi_max: i8,
	pub frames: u32,
}

impl Heard {
	pub const EMPTY: Self = Self {
		mac: Mac([0; 6]),
		rssi_max: i8::MIN,
		frames: 0,
	};
}

#[derive(Debug)]
pub struct IdentifyTable {
	/// The first `len` are in use.
	pub heard: [Heard; MAX_HEARD],
	pub len: usize,
}

impl IdentifyTable {
	pub const fn new() -> Self {
		Self {
			heard: [Heard::EMPTY; MAX_HEARD],
			len: 0,
		}
	}
}

impl Default for IdentifyTable {
	fn default() -> Self {
		Self::new()
	}
}

/// Writes the post into `buf` and returns its length, with at most `MAX_SENT`
/// devices from `heard`, which comes strongest first. `Err` only if it didn't
/// fit, which the worst-case test rules out for `IDENTIFY_BUF` and a short
/// sensor name (plain ASCII, written as-is).
pub fn write_identify(
	buf: &mut [u8],
	sensor: &str,
	heard: impl Iterator<Item = Heard>,
) -> Result<usize, BufferFull> {
	let mut out = Out {
		buf,
		len: 0,
	};
	write_all(&mut out, sensor, heard).map_err(|_| BufferFull)?;
	Ok(out.len)
}

fn write_all(out: &mut Out, sensor: &str, heard: impl Iterator<Item = Heard>) -> fmt::Result {
	write!(out, r#"{{"sensor":"{sensor}","devices":["#)?;
	for (i, h) in heard.take(MAX_SENT).enumerate() {
		if i > 0 {
			out.write_char(',')?;
		}
		let m = h.mac.0;
		write!(
			out,
			r#"{{"mac":"{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}","rssi_max":{},"frames":{}}}"#,
			m[0], m[1], m[2], m[3], m[4], m[5], h.rssi_max, h.frames
		)?;
	}
	out.write_str("]}")
}

// a device is at most 64 B with its comma; the report task's buffer can hold a post
const _: () = assert!(IDENTIFY_BUF >= MAX_SENT * 64 + 64);
const _: () = assert!(IDENTIFY_BUF <= REPORT_BUF);

#[cfg(test)]
mod tests {
	use super::*;

	fn render(sensor: &str, heard: Vec<Heard>) -> serde_json::Value {
		let mut buf = [0u8; IDENTIFY_BUF];
		let len = write_identify(&mut buf, sensor, heard.into_iter()).unwrap();
		serde_json::from_slice(&buf[..len]).expect("valid JSON")
	}

	#[test]
	fn empty_post() {
		let v = render("c6-1", vec![]);
		assert_eq!(v["sensor"], "c6-1");
		assert_eq!(v["devices"].as_array().unwrap().len(), 0);
	}

	#[test]
	fn devices_in_the_order_given() {
		let heard = vec![
			Heard {
				mac: Mac([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01]),
				rssi_max: -31,
				frames: 12,
			},
			Heard {
				mac: Mac([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x02]),
				rssi_max: -70,
				frames: 1,
			},
		];
		let v = render("c6-1", heard);
		let d = v["devices"].as_array().unwrap();
		assert_eq!(d.len(), 2);
		assert_eq!(d[0]["mac"], "aa:bb:cc:dd:ee:01");
		assert_eq!((d[0]["rssi_max"].as_i64(), d[0]["frames"].as_u64()), (Some(-31), Some(12)));
		assert_eq!(d[1]["rssi_max"], -70);
	}

	#[test]
	fn worst_case_fits() {
		let heard: Vec<Heard> = (0..MAX_HEARD)
			.map(|i| Heard {
				mac: Mac([0xff, 0xff, 0xff, 0xff, 0xff, i as u8]),
				rssi_max: i8::MIN,
				frames: u32::MAX,
			})
			.collect();
		let v = render("c6-1", heard);
		assert_eq!(v["devices"].as_array().unwrap().len(), MAX_SENT);
	}

	#[test]
	fn full_buffer_is_an_error() {
		let mut buf = [0u8; IDENTIFY_BUF];
		let long = "x".repeat(IDENTIFY_BUF);
		assert_eq!(write_identify(&mut buf, &long, core::iter::empty()), Err(BufferFull));
	}
}
