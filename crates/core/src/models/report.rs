//! The report the sensor posts every period, written as JSON straight into a
//! fixed buffer: no allocation, and the worst case fits (see the tests).

use core::fmt::{self, Write};

use crate::models::{
	counters::{Counters, Device, MAX_DEVICES},
	streams::StreamTable,
	watched::MAX_NETS,
};

/// Fits a full table: 128 devices with every counter at its maximum and a
/// stream count each.
pub const REPORT_BUF: usize = 27 * 1024;

pub struct ReportHeader<'a> {
	/// Plain ASCII, no quotes or backslashes: it's written as-is.
	pub sensor: &'a str,
	/// Random per boot: `seq` and uptime start again at every boot, so only
	/// this tells a resend from another boot's report.
	pub boot: u32,
	pub seq: u32,
	pub uptime_s: u64,
	pub channel: u8,
	/// Frames lost because the sniffer queue was full.
	pub dropped: u32,
	/// Frames from devices that didn't fit in the table.
	pub overflow: u32,
	/// Which watched networks have a learned BSSID; until then, their frames
	/// can't be told apart from the neighbours' and aren't counted.
	pub nets_known: [bool; MAX_NETS],
}

#[derive(Debug, PartialEq, Eq)]
pub struct BufferFull;

/// Writes the report into `buf` and returns its length. `Err` only if it
/// didn't fit, which the worst-case test rules out for a short sensor name.
/// A device's stream count goes along when `streams` knows it.
pub fn write_report(
	buf: &mut [u8; REPORT_BUF],
	header: &ReportHeader,
	devices: impl Iterator<Item = Device>,
	broadcast: &[Counters; MAX_NETS],
	streams: &StreamTable,
) -> Result<usize, BufferFull> {
	let mut out = Out {
		buf,
		len: 0,
	};
	write_all(&mut out, header, devices, broadcast, streams).map_err(|_| BufferFull)?;
	Ok(out.len)
}

fn write_all(
	out: &mut Out,
	h: &ReportHeader,
	devices: impl Iterator<Item = Device>,
	broadcast: &[Counters; MAX_NETS],
	streams: &StreamTable,
) -> fmt::Result {
	write!(
		out,
		r#"{{"sensor":"{}","boot":{},"seq":{},"uptime_s":{},"channel":{},"dropped":{},"overflow":{},"nets_known":["#,
		h.sensor, h.boot, h.seq, h.uptime_s, h.channel, h.dropped, h.overflow
	)?;
	let mut first = true;
	for net in (0..MAX_NETS).filter(|&net| h.nets_known[net]) {
		if !first {
			out.write_char(',')?;
		}
		first = false;
		write!(out, "{net}")?;
	}
	out.write_str(r#"],"devices":["#)?;
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
			i8::MIN => out.write_str("null")?,
			rssi => write!(out, "{rssi}")?,
		}
		if let Some(s) = streams.get(&d.mac) {
			write!(out, r#","streams":{},"streams_from":"{}""#, s.streams, s.from.name())?;
		}
		out.write_char('}')?;
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

/// Writes into a fixed buffer, failing when it's full.
pub(crate) struct Out<'a> {
	pub(crate) buf: &'a mut [u8],
	pub(crate) len: usize,
}

impl Write for Out<'_> {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let end = self.len + s.len();
		self.buf.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes());
		self.len = end;
		Ok(())
	}
}

// keeps REPORT_BUF honest if MAX_DEVICES grows: a device is at most 199 B
// with 8 streams, the most HE can say
const _: () = assert!(REPORT_BUF >= MAX_DEVICES * 200 + 1024);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::models::{
		counters::Counters,
		frame::{Direction, Frame, Mac},
		streams::{Source, StreamCount},
	};

	fn header() -> ReportHeader<'static> {
		ReportHeader {
			sensor: "c6-1",
			boot: u32::MAX,
			seq: 42,
			uptime_s: 2520,
			channel: 6,
			dropped: 1,
			overflow: 0,
			nets_known: [true, false, true, false],
		}
	}

	fn render(devices: Vec<Device>, broadcast: [Counters; MAX_NETS]) -> serde_json::Value {
		render_with(devices, broadcast, &StreamTable::new())
	}

	fn render_with(
		devices: Vec<Device>,
		broadcast: [Counters; MAX_NETS],
		streams: &StreamTable,
	) -> serde_json::Value {
		let mut buf = [0u8; REPORT_BUF];
		let len =
			write_report(&mut buf, &header(), devices.into_iter(), &broadcast, streams).unwrap();
		serde_json::from_slice(&buf[..len]).expect("valid JSON")
	}

	fn streams_of(devices: &[Device], streams: u8, from: Source) -> StreamTable {
		let mut t = StreamTable::new();
		for (slot, d) in t.entries.iter_mut().zip(devices) {
			*slot = Some(StreamCount {
				mac: d.mac,
				streams,
				from,
				learned: 0,
			});
		}
		t
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
		assert_eq!(v["boot"], u32::MAX);
		assert_eq!(v["seq"], 42);
		assert_eq!(v["channel"], 6);
		assert_eq!(v["dropped"], 1);
		assert_eq!(v["nets_known"], serde_json::json!([0, 2]));
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
	fn streams_only_when_known() {
		let known = device(1, true);
		let t = streams_of(&[known], 2, Source::Assoc);
		let v = render_with(vec![known, device(2, true)], [Counters::EMPTY; MAX_NETS], &t);
		let devices = v["devices"].as_array().unwrap();
		assert_eq!(
			(devices[0]["streams"].as_u64(), devices[0]["streams_from"].as_str()),
			(Some(2), Some("assoc"))
		);
		assert_eq!(devices[0]["rssi_max"], -41);
		assert!(devices[1].get("streams").is_none() && devices[1].get("streams_from").is_none());
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
		let streams = streams_of(&devices, 8, Source::Probe);
		let v = render_with(devices, [max; MAX_NETS], &streams);
		let devices = v["devices"].as_array().unwrap();
		assert_eq!(devices.len(), MAX_DEVICES);
		assert!(devices.iter().all(|d| d["streams"] == 8));
	}

	#[test]
	fn full_buffer_is_an_error() {
		let mut buf = [0u8; REPORT_BUF];
		let long = "x".repeat(REPORT_BUF);
		let h = ReportHeader {
			sensor: &long,
			..header()
		};
		let r = write_report(
			&mut buf,
			&h,
			core::iter::empty(),
			&[Counters::EMPTY; MAX_NETS],
			&StreamTable::new(),
		);
		assert_eq!(r, Err(BufferFull));
	}
}
