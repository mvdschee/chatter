//! Per-device counters for one report period. The table is fixed-size and
//! emptied by every report, so it holds the devices active within one period.

use crate::models::{
	frame::{Direction, Frame, Mac},
	watched::MAX_NETS,
};

pub const MAX_DEVICES: usize = 128;

const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];

/// Writes `bytes` as `"<value> <unit>"` into `buf` (`"512 B"`, `"1.5 KiB"`)
/// and returns the length; `&buf[..len]` is the text. Integer math only (the
/// C6 has no FPU), and one truncated decimal above bytes. The widest result,
/// `"1023.9 MiB"`, fits the 16 bytes.
pub fn format_bytes(bytes: u32, buf: &mut [u8; 16]) -> usize {
	let bytes = u64::from(bytes);
	let mut unit = 0;
	while unit < UNITS.len() - 1 && bytes >= 1 << (10 * (unit + 1)) {
		unit += 1;
	}
	let divisor = 1u64 << (10 * unit);

	let mut len = write_digits(bytes / divisor, buf);
	if unit > 0 {
		buf[len] = b'.';
		buf[len + 1] = b'0' + (bytes % divisor * 10 / divisor) as u8;
		len += 2;
	}
	buf[len] = b' ';
	len += 1;
	for &b in UNITS[unit].as_bytes() {
		buf[len] = b;
		len += 1;
	}
	len
}

/// `value` is below 1024 here, so at most four digits.
fn write_digits(mut value: u64, buf: &mut [u8; 16]) -> usize {
	let mut digits = [0u8; 4];
	let mut n = 0;
	loop {
		digits[n] = b'0' + (value % 10) as u8;
		value /= 10;
		n += 1;
		if value == 0 {
			break;
		}
	}
	for (i, d) in digits[..n].iter().rev().enumerate() {
		buf[i] = *d;
	}
	n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counters {
	pub tx_frames: u32,
	pub tx_bytes: u32,
	pub rx_frames: u32,
	pub rx_bytes: u32,
	/// Retransmissions seen. Only those whose original was also seen are left
	/// out of the frame and byte counts.
	pub retries: u32,
	/// Strongest signal from the device itself (uplink only); `i8::MIN` if none.
	pub rssi_max: i8,
}

impl Default for Counters {
	fn default() -> Self {
		Self::EMPTY
	}
}

impl Counters {
	pub const EMPTY: Self = Self {
		tx_frames: 0,
		tx_bytes: 0,
		rx_frames: 0,
		rx_bytes: 0,
		retries: 0,
		rssi_max: i8::MIN,
	};

	/// Every retry counts toward `retries`. Frames and bytes skip `duplicate`s:
	/// retries of a frame that was already counted.
	pub fn add(&mut self, frame: &Frame, duplicate: bool) {
		if frame.retry {
			self.retries += 1;
		}
		if duplicate {
			return;
		}
		let len = u32::from(frame.len);
		match frame.direction {
			Direction::Up => {
				self.tx_frames += 1;
				self.tx_bytes += len;
				self.rssi_max = self.rssi_max.max(frame.rssi);
			},
			Direction::Down => {
				self.rx_frames += 1;
				self.rx_bytes += len;
			},
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
	pub mac: Mac,
	pub net: u8,
	pub counters: Counters,
	/// Last sequence control seen per direction (`Up`, `Down`), to spot
	/// retries of frames that were already counted.
	last_seq: [Option<u16>; 2],
}

impl Device {
	pub fn new(mac: Mac, net: u8) -> Self {
		Self {
			mac,
			net,
			counters: Counters::EMPTY,
			last_seq: [None; 2],
		}
	}

	/// Counts `frame`. A retry only adds bytes when its original was missed,
	/// that is, when its sequence control differs from the last one seen.
	pub fn add(&mut self, frame: &Frame) {
		let last = &mut self.last_seq[frame.direction as usize];
		let duplicate = frame.retry && *last == Some(frame.seq);
		*last = Some(frame.seq);
		self.counters.add(frame, duplicate);
	}
}

#[derive(Debug)]
pub struct DeviceTable {
	pub devices: [Option<Device>; MAX_DEVICES],
	/// Broadcast and multicast from the router, per network.
	pub broadcast: [Counters; MAX_NETS],
	/// Frames from devices that didn't fit in the table.
	pub overflow: u32,
}

impl DeviceTable {
	pub const fn new() -> Self {
		Self {
			devices: [None; MAX_DEVICES],
			broadcast: [Counters::EMPTY; MAX_NETS],
			overflow: 0,
		}
	}
}

impl Default for DeviceTable {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn frame(direction: Direction, len: u16, rssi: i8, retry: bool) -> Frame {
		Frame {
			station: Mac([0x02, 0, 0, 0, 0, 1]),
			net: 0,
			direction,
			len,
			rssi,
			retry,
			seq: 0,
		}
	}

	#[test]
	fn up_and_down() {
		let mut c = Counters::default();
		c.add(&frame(Direction::Up, 100, -50, false), false);
		c.add(&frame(Direction::Up, 50, -40, false), false);
		c.add(&frame(Direction::Down, 1500, -10, false), false);
		assert_eq!((c.tx_frames, c.tx_bytes, c.rx_frames, c.rx_bytes), (2, 150, 1, 1500));
		// downlink RSSI is the router's signal, not the device's
		assert_eq!(c.rssi_max, -40);
	}

	#[test]
	fn duplicates_only_count_as_retries() {
		let mut c = Counters::default();
		c.add(&frame(Direction::Up, 100, -50, true), true);
		assert_eq!((c.retries, c.tx_frames, c.tx_bytes, c.rssi_max), (1, 0, 0, i8::MIN));
		// a retry whose original was missed still carries the bytes
		c.add(&frame(Direction::Up, 100, -50, true), false);
		assert_eq!((c.retries, c.tx_frames, c.tx_bytes), (2, 1, 100));
	}

	fn seq_frame(direction: Direction, retry: bool, seq: u16) -> Frame {
		Frame {
			seq,
			..frame(direction, 100, -50, retry)
		}
	}

	#[test]
	fn device_skips_retries_it_already_counted() {
		let mut d = Device::new(Mac([0x02, 0, 0, 0, 0, 1]), 0);
		d.add(&seq_frame(Direction::Down, false, 10));
		d.add(&seq_frame(Direction::Down, true, 10)); // retry of a counted frame
		d.add(&seq_frame(Direction::Down, true, 11)); // retry, original missed
		d.add(&seq_frame(Direction::Down, true, 11)); // second retry of it
		assert_eq!((d.counters.rx_frames, d.counters.rx_bytes, d.counters.retries), (2, 200, 3));
	}

	#[test]
	fn directions_keep_their_own_sequence() {
		let mut d = Device::new(Mac([0x02, 0, 0, 0, 0, 1]), 0);
		d.add(&seq_frame(Direction::Up, false, 5));
		// same number the other way is a different frame
		d.add(&seq_frame(Direction::Down, true, 5));
		assert_eq!((d.counters.tx_frames, d.counters.rx_frames), (1, 1));
	}

	fn fmt(bytes: u32) -> String {
		let mut buf = [0u8; 16];
		let len = format_bytes(bytes, &mut buf);
		std::str::from_utf8(&buf[..len]).unwrap().to_string()
	}

	#[test]
	fn formats_bytes_and_units() {
		assert_eq!(fmt(0), "0 B");
		assert_eq!(fmt(512), "512 B");
		assert_eq!(fmt(1023), "1023 B");
		assert_eq!(fmt(1024), "1.0 KiB");
		assert_eq!(fmt(1536), "1.5 KiB");
		// truncated, not rounded
		assert_eq!(fmt(2047), "1.9 KiB");
		assert_eq!(fmt(1 << 20), "1.0 MiB");
		assert_eq!(fmt((1 << 30) - 1), "1023.9 MiB");
		assert_eq!(fmt(u32::MAX), "3.9 GiB");
	}
}
