//! Frames in, per-device counters out.

use crate::models::{
	counters::{Counters, Device, DeviceTable},
	frame::Frame,
	watched::MAX_NETS,
};

/// Adds one frame to its device, or to the network's broadcast total.
pub fn record(table: &mut DeviceTable, frame: &Frame) {
	if frame.station.is_group() {
		if let Some(total) = table.broadcast.get_mut(frame.net as usize) {
			total.add(frame);
		}
		return;
	}

	let mut free = None;
	for (i, slot) in table.devices.iter_mut().enumerate() {
		match slot {
			Some(device) if device.mac == frame.station && device.net == frame.net => {
				device.counters.add(frame);
				return;
			},
			None if free.is_none() => free = Some(i),
			_ => {},
		}
	}

	match free {
		Some(i) => {
			let mut counters = Counters::default();
			counters.add(frame);
			table.devices[i] = Some(Device {
				mac: frame.station,
				net: frame.net,
				counters,
			});
		},
		None => table.overflow += 1,
	}
}

/// Takes every device out of the table, leaving it empty for the next period.
pub fn drain(table: &mut DeviceTable) -> impl Iterator<Item = Device> + '_ {
	table.devices.iter_mut().filter_map(Option::take)
}

/// Takes the broadcast totals and the overflow count, resetting both.
pub fn take_totals(table: &mut DeviceTable) -> ([Counters; MAX_NETS], u32) {
	let broadcast = core::mem::take(&mut table.broadcast);
	(broadcast, core::mem::take(&mut table.overflow))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::models::{
		counters::MAX_DEVICES,
		frame::{Direction, Mac},
	};

	fn frame(last: u8, net: u8, direction: Direction) -> Frame {
		Frame {
			station: Mac([0x02, 0, 0, 0, 0, last]),
			net,
			direction,
			len: 100,
			rssi: -50,
			retry: false,
		}
	}

	#[test]
	fn same_device_same_slot() {
		let mut t = DeviceTable::new();
		record(&mut t, &frame(1, 0, Direction::Up));
		record(&mut t, &frame(1, 0, Direction::Down));
		record(&mut t, &frame(2, 0, Direction::Up));
		let devices: Vec<_> = drain(&mut t).collect();
		assert_eq!(devices.len(), 2);
		assert_eq!((devices[0].counters.tx_frames, devices[0].counters.rx_frames), (1, 1));
	}

	#[test]
	fn same_mac_on_two_networks_is_two_devices() {
		let mut t = DeviceTable::new();
		record(&mut t, &frame(1, 0, Direction::Up));
		record(&mut t, &frame(1, 1, Direction::Up));
		assert_eq!(drain(&mut t).count(), 2);
	}

	#[test]
	fn broadcast_goes_to_the_network_total() {
		let mut t = DeviceTable::new();
		let mut f = frame(0, 1, Direction::Down);
		f.station = Mac([0xff; 6]);
		record(&mut t, &f);
		assert_eq!(drain(&mut t).count(), 0);
		let (broadcast, _) = take_totals(&mut t);
		assert_eq!((broadcast[1].rx_frames, broadcast[1].rx_bytes), (1, 100));
	}

	#[test]
	fn full_table_counts_overflow() {
		let mut t = DeviceTable::new();
		for i in 0..MAX_DEVICES {
			let mut f = frame(0, 0, Direction::Up);
			f.station.0[4] = (i >> 8) as u8;
			f.station.0[5] = i as u8;
			record(&mut t, &f);
		}
		record(&mut t, &frame(0xff, 1, Direction::Up));
		assert_eq!(take_totals(&mut t).1, 1);
	}

	#[test]
	fn drain_and_take_totals_reset_the_table() {
		let mut t = DeviceTable::new();
		record(&mut t, &frame(1, 0, Direction::Up));
		assert_eq!(drain(&mut t).count(), 1);
		assert_eq!(drain(&mut t).count(), 0);
		take_totals(&mut t);
		assert_eq!(take_totals(&mut t), ([Counters::default(); MAX_NETS], 0));
	}
}
