//! Frames in, the devices heard out, strongest first.

use crate::models::{
	frame::{Direction, Frame},
	identify::{Heard, IdentifyTable, MAX_HEARD},
};

/// Adds one frame to its device. Only the device's own frames count: a
/// downlink frame's signal is the router's.
pub fn record(table: &mut IdentifyTable, frame: &Frame) {
	if frame.direction != Direction::Up {
		return;
	}
	if let Some(h) = table.heard[..table.len].iter_mut().find(|h| h.mac == frame.station) {
		h.rssi_max = h.rssi_max.max(frame.rssi);
		h.frames += 1;
		return;
	}

	let new = Heard {
		mac: frame.station,
		rssi_max: frame.rssi,
		frames: 1,
	};
	if table.len < MAX_HEARD {
		table.heard[table.len] = new;
		table.len += 1;
		return;
	}
	// full: the device next to the sensor must never be the one left out
	if let Some(weakest) = table.heard.iter_mut().min_by_key(|h| h.rssi_max)
		&& weakest.rssi_max < frame.rssi
	{
		*weakest = new;
	}
}

/// Takes every device out of the table, strongest first (more frames first
/// on a tie), leaving it empty for the next post.
pub fn drain_strongest(table: &mut IdentifyTable) -> impl Iterator<Item = Heard> + '_ {
	let heard = &mut table.heard[..table.len];
	heard.sort_unstable_by(|a, b| b.rssi_max.cmp(&a.rssi_max).then(b.frames.cmp(&a.frames)));
	table.len = 0;
	heard.iter().copied()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::models::frame::Mac;

	fn frame(last: u8, rssi: i8, direction: Direction) -> Frame {
		Frame {
			station: Mac([0x02, 0, 0, 0, 0, last]),
			net: 0,
			direction,
			len: 100,
			rssi,
			retry: false,
			seq: 0,
		}
	}

	fn macs(table: &mut IdentifyTable) -> Vec<u8> {
		drain_strongest(table).map(|h| h.mac.0[5]).collect()
	}

	#[test]
	fn strongest_signal_and_frames_per_device() {
		let mut t = IdentifyTable::new();
		record(&mut t, &frame(1, -60, Direction::Up));
		record(&mut t, &frame(1, -45, Direction::Up));
		record(&mut t, &frame(1, -50, Direction::Up));
		let heard: Vec<_> = drain_strongest(&mut t).collect();
		assert_eq!(heard.len(), 1);
		assert_eq!((heard[0].rssi_max, heard[0].frames), (-45, 3));
	}

	#[test]
	fn downlink_is_ignored() {
		let mut t = IdentifyTable::new();
		record(&mut t, &frame(1, -20, Direction::Down));
		record(&mut t, &frame(2, -70, Direction::Up));
		record(&mut t, &frame(2, -10, Direction::Down));
		let heard: Vec<_> = drain_strongest(&mut t).collect();
		assert_eq!((heard.len(), heard[0].rssi_max), (1, -70));
	}

	#[test]
	fn strongest_first_then_most_frames() {
		let mut t = IdentifyTable::new();
		record(&mut t, &frame(1, -70, Direction::Up));
		record(&mut t, &frame(2, -40, Direction::Up));
		record(&mut t, &frame(3, -55, Direction::Up));
		record(&mut t, &frame(4, -55, Direction::Up));
		record(&mut t, &frame(4, -55, Direction::Up));
		assert_eq!(macs(&mut t), [2, 4, 3, 1]);
	}

	#[test]
	fn a_strong_newcomer_gets_into_a_full_table() {
		let mut t = IdentifyTable::new();
		for i in 0..MAX_HEARD {
			record(&mut t, &frame(i as u8, -80, Direction::Up));
		}
		record(&mut t, &frame(0xfe, -90, Direction::Up)); // weaker: left out
		record(&mut t, &frame(0xff, -30, Direction::Up));
		let heard: Vec<_> = drain_strongest(&mut t).collect();
		assert_eq!(heard.len(), MAX_HEARD);
		assert_eq!((heard[0].mac.0[5], heard[0].rssi_max), (0xff, -30));
		assert!(heard.iter().all(|h| h.mac.0[5] != 0xfe));
	}

	#[test]
	fn drain_empties_the_table() {
		let mut t = IdentifyTable::new();
		record(&mut t, &frame(1, -50, Direction::Up));
		assert_eq!(drain_strongest(&mut t).count(), 1);
		assert_eq!(drain_strongest(&mut t).count(), 0);
	}
}
