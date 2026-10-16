//! The report the sensor posts every period, written as JSON straight into a
//! fixed buffer: no allocation, and the worst case fits (see the tests).

use core::fmt::{self, Write};

use crate::models::{
	counters::{Counters, Device, MAX_DEVICES},
	watched::MAX_NETS,
};

/// Fits a full table: 128 devices with every counter at its maximum.
pub const REPORT_BUF: usize = 24 * 1024;

pub struct ReportHeader<'a> {
	/// Plain ASCII, no quotes or backslashes: it's written as-is.
	pub sensor: &'a str,
	pub seq: u32,
	pub uptime_s: u64,
	pub channel: u8,
	/// Frames lost because the sniffer queue was full.
	pub dropped: u32,
	/// Frames from devices that didn't fit in the table.
	pub overflow: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BufferFull;

/// Writes the report into `buf` and returns its length. `Err` only if it
/// didn't fit, which the worst-case test rules out for a short sensor name.
pub fn write_report(
	buf: &mut [u8; REPORT_BUF],
	header: &ReportHeader,
	devices: impl Iterator<Item = Device>,
	broadcast: &[Counters; MAX_NETS],
) -> Result<usize, BufferFull> {
	let mut out = Out {
		buf,
		len: 0,
	};
	write_all(&mut out, header, devices, broadcast).map_err(|_| BufferFull)?;
	Ok(out.len)
}

fn write_all(
	out: &mut Out,
	h: &ReportHeader,
	devices: impl Iterator<Item = Device>,
	broadcast: &[Counters; MAX_NETS],
) -> fmt::Result {
	write!(
		out,
		r#"{{"sensor":"{}","seq":{},"uptime_s":{},"channel":{},"dropped":{},"overflow":{},"devices":["#,
		h.sensor, h.seq, h.uptime_s, h.channel, h.dropped, h.overflow
	)?;
	for (i, d) in devices.enumerate() {
		if i > 0 {
			out.write_char(',')?;
		}
		let m = d.mac.0;
		let c = d.counters;
		write!(
			out,
			r#"{{"mac":"{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}","net":{},"tx_frames":{},"tx_bytes":{},"rx_frames":{},"rx_bytes":{},"retries":{},"rssi_max":"#,
			m[0],
			m[1],
			m[2],
			m[3],
			m[4],
			m[5],
			d.net,
			c.tx_frames,
			c.tx_bytes,
			c.rx_frames,
			c.rx_bytes,
			c.retries
		)?;
		// no uplink frame means no RSSI from the device itself
		match c.rssi_max {
			i8::MIN => out.write_str("null}")?,
			rssi => write!(out, "{rssi}}}")?,
		}
	}
	out.write_str(r#"],"broadcast":["#)?;
	let mut first = true;
	for (net, b) in broadcast.iter().enumerate().filter(|(_, b)| b.rx_frames > 0) {
		if !first {
			out.write_char(',')?;
		}
		first = false;
		write!(out, r#"{{"net":{},"rx_frames":{},"rx_bytes":{}}}"#, net, b.rx_frames, b.rx_bytes)?;
	}
	out.write_str("]}")
}

struct Out<'a> {
	buf: &'a mut [u8; REPORT_BUF],
	len: usize,
}

impl Write for Out<'_> {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let end = self.len + s.len();
		self.buf.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes());
		self.len = end;
		Ok(())
	}
}

// keeps REPORT_BUF honest if MAX_DEVICES grows: a device is at most 165 B
const _: () = assert!(REPORT_BUF >= MAX_DEVICES * 170 + 1024);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::models::{
		counters::Counters,
		frame::{Direction, Frame, Mac},
	};

	fn header() -> ReportHeader<'static> {
		ReportHeader {
			sensor: "c6-1",
			seq: 42,
			uptime_s: 2520,
			channel: 6,
			dropped: 1,
			overflow: 0,
		}
	}

	fn render(devices: Vec<Device>, broadcast: [Counters; MAX_NETS]) -> serde_json::Value {
		let mut buf = [0u8; REPORT_BUF];
		let len = write_report(&mut buf, &header(), devices.into_iter(), &broadcast).unwrap();
		serde_json::from_slice(&buf[..len]).expect("valid JSON")
	}

	fn device(last: u8, up: bool) -> Device {
		let mut d = Device::new(Mac([0xaa, 0xbb, 0xcc, 0xdd, 0xee, last]), 1);
		let direction = if up {
			Direction::Up
		} else {
			Direction::Down
		};
		d.add(&Frame {
			station: d.mac,
			net: 1,
			direction,
			len: 1500,
			rssi: -41,
			retry: false,
			seq: 0,
		});
		d
	}

	#[test]
	fn empty_report() {
		let v = render(vec![], [Counters::EMPTY; MAX_NETS]);
		assert_eq!(v["seq"], 42);
		assert_eq!(v["channel"], 6);
		assert_eq!(v["dropped"], 1);
		assert_eq!(v["devices"].as_array().unwrap().len(), 0);
		assert_eq!(v["broadcast"].as_array().unwrap().len(), 0);
	}

	#[test]
	fn devices_and_broadcast() {
		let mut broadcast = [Counters::EMPTY; MAX_NETS];
		broadcast[1].rx_frames = 3;
		broadcast[1].rx_bytes = 300;
		let v = render(vec![device(1, true), device(2, false)], broadcast);

		let devices = v["devices"].as_array().unwrap();
		assert_eq!(devices.len(), 2);
		assert_eq!(devices[0]["mac"], "aa:bb:cc:dd:ee:01");
		assert_eq!(devices[0]["net"], 1);
		assert_eq!(
			(devices[0]["tx_bytes"].as_u64(), devices[0]["rssi_max"].as_i64()),
			(Some(1500), Some(-41))
		);
		// downlink only: no RSSI from the device
		assert!(devices[1]["rssi_max"].is_null());
		assert_eq!(devices[1]["rx_frames"], 1);

		let b = v["broadcast"].as_array().unwrap();
		assert_eq!(
			(b.len(), b[0]["net"].as_u64(), b[0]["rx_bytes"].as_u64()),
			(1, Some(1), Some(300))
		);
	}

	#[test]
	fn worst_case_fits() {
		let max = Counters {
			tx_frames: u32::MAX,
			tx_bytes: u32::MAX,
			rx_frames: u32::MAX,
			rx_bytes: u32::MAX,
			retries: u32::MAX,
			rssi_max: -100,
		};
		let devices: Vec<Device> = (0..MAX_DEVICES)
			.map(|i| {
				let mut d = Device::new(Mac([0xff, 0xff, 0xff, 0xff, 0xff, i as u8]), u8::MAX);
				d.counters = max;
				d
			})
			.collect();
		let v = render(devices, [max; MAX_NETS]);
		assert_eq!(v["devices"].as_array().unwrap().len(), MAX_DEVICES);
	}

	#[test]
	fn full_buffer_is_an_error() {
		let mut buf = [0u8; REPORT_BUF];
		let long = "x".repeat(REPORT_BUF);
		let h = ReportHeader {
			sensor: &long,
			..header()
		};
		let r = write_report(&mut buf, &h, core::iter::empty(), &[Counters::EMPTY; MAX_NETS]);
		assert_eq!(r, Err(BufferFull));
	}
}
