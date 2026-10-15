//! Per-device counters for one report period. The table is fixed-size and
//! emptied by every report, so it holds the devices active within one period.

use crate::models::{
	frame::{Direction, Frame, Mac},
	watched::MAX_NETS,
};

pub const MAX_DEVICES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counters {
	pub tx_frames: u32,
	pub tx_bytes: u32,
	pub rx_frames: u32,
	pub rx_bytes: u32,
	/// Retransmissions; not in the frame and byte counts.
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

	pub fn add(&mut self, frame: &Frame) {
		if frame.retry {
			self.retries += 1;
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
		}
	}

	#[test]
	fn up_and_down() {
		let mut c = Counters::default();
		c.add(&frame(Direction::Up, 100, -50, false));
		c.add(&frame(Direction::Up, 50, -40, false));
		c.add(&frame(Direction::Down, 1500, -10, false));
		assert_eq!((c.tx_frames, c.tx_bytes, c.rx_frames, c.rx_bytes), (2, 150, 1, 1500));
		// downlink RSSI is the router's signal, not the device's
		assert_eq!(c.rssi_max, -40);
	}

	#[test]
	fn retries_are_counted_apart() {
		let mut c = Counters::default();
		c.add(&frame(Direction::Up, 100, -50, true));
		assert_eq!((c.retries, c.tx_frames, c.tx_bytes, c.rssi_max), (1, 0, 0, i8::MIN));
	}
}
