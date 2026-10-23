//! Requests in, a stream count per member out (docs/streams.md).

use crate::models::{
	counters::DeviceTable,
	frame::Capabilities,
	streams::{Source, StreamCount, StreamTable},
	watched::Watched,
};

/// Learns from one request; returns the count when it changed.
///
/// An association or reassociation request to a watched network makes its
/// sender a member, and replaces what was known. A probe request only counts
/// from a member (counted this period, or known already), and only until an
/// association request is heard; between probes the larger count stays, as
/// a probe may leave elements out. When the table is full, the oldest probe
/// count goes first, then the oldest association count; a probe never pushes
/// out an association count.
pub fn learn(
	table: &mut StreamTable,
	caps: &Capabilities,
	watched: &Watched,
	devices: &DeviceTable,
) -> Option<StreamCount> {
	let streams = caps.streams();
	if streams == 0 {
		return None;
	}
	let mac = caps.station;
	let at = table.entries.iter().position(|e| e.is_some_and(|c| c.mac == mac));
	let from = if caps.is_probe() {
		let counted = devices.devices.iter().flatten().any(|d| d.mac == mac);
		if at.is_none() && !counted {
			return None;
		}
		Source::Probe
	} else {
		watched.net_of(&caps.bssid)?;
		Source::Assoc
	};

	let slot = match at {
		Some(i) => {
			let old = table.entries[i]?;
			let keep = match (old.from, from) {
				(Source::Assoc, Source::Probe) => true,
				(Source::Probe, Source::Probe) => old.streams >= streams,
				(Source::Assoc, Source::Assoc) => old.streams == streams,
				(Source::Probe, Source::Assoc) => false,
			};
			if keep {
				return None;
			}
			i
		},
		None => match table.entries.iter().position(Option::is_none) {
			Some(i) => i,
			None => {
				let (i, oldest) = table
					.entries
					.iter()
					.enumerate()
					.filter_map(|(i, e)| e.map(|c| (i, c)))
					.min_by_key(|(_, c)| (c.from, c.learned))?;
				if oldest.from > from {
					return None;
				}
				i
			},
		},
	};

	let count = StreamCount {
		mac,
		streams,
		from,
		learned: table.next,
	};
	table.next = table.next.wrapping_add(1);
	table.entries[slot] = Some(count);
	Some(count)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		handlers::aggregate,
		models::{
			frame::{Direction, Frame, He, Ht, Mac},
			streams::MAX_KNOWN,
		},
	};

	const BSSID: Mac = Mac([0x02, 0, 0, 0, 0, 0xaa]);
	const NEIGHBOUR: Mac = Mac([0x02, 0, 0, 0, 0, 0xbb]);
	const BROADCAST: Mac = Mac([0xff; 6]);
	const ASSOC: u8 = 0;
	const REASSOC: u8 = 2;
	const PROBE: u8 = 4;

	fn station(last: u8) -> Mac {
		Mac([0x0e, 0x51, 0x7a, 0, 0, last])
	}

	fn watched() -> Watched {
		let mut w = Watched::default();
		w.learn(0, BSSID);
		w
	}

	fn caps(last: u8, subtype: u8, bssid: Mac, ht: u8, he: Option<u8>) -> Capabilities {
		Capabilities {
			station: station(last),
			bssid,
			subtype,
			ht: Some(Ht {
				rx_streams: ht,
				tx_params: 0,
				sm_power_save: 3,
			}),
			he: he.map(|n| He {
				rx_streams: n,
				tx_streams: n,
				len: 26,
			}),
		}
	}

	/// A device table where `members` sent a frame this period.
	fn counted(members: &[u8]) -> DeviceTable {
		let mut t = DeviceTable::new();
		for &last in members {
			aggregate::record(
				&mut t,
				&Frame {
					station: station(last),
					net: 0,
					direction: Direction::Up,
					len: 100,
					rssi: -50,
					retry: false,
					seq: 0,
				},
			);
		}
		t
	}

	fn known(t: &StreamTable, last: u8) -> Option<(u8, Source)> {
		t.get(&station(last)).map(|c| (c.streams, c.from))
	}

	#[test]
	fn association_to_our_network_is_learned() {
		let mut t = StreamTable::new();
		let none = counted(&[]);
		assert!(learn(&mut t, &caps(1, ASSOC, BSSID, 2, Some(2)), &watched(), &none).is_some());
		assert!(learn(&mut t, &caps(2, REASSOC, BSSID, 1, None), &watched(), &none).is_some());
		assert_eq!(known(&t, 1), Some((2, Source::Assoc)));
		assert_eq!(known(&t, 2), Some((1, Source::Assoc)));
	}

	#[test]
	fn association_to_a_neighbour_is_ignored() {
		let mut t = StreamTable::new();
		let devices = counted(&[1]);
		assert!(learn(&mut t, &caps(1, ASSOC, NEIGHBOUR, 2, None), &watched(), &devices).is_none());
		assert_eq!(known(&t, 1), None);
	}

	#[test]
	fn an_unknown_count_is_ignored() {
		let mut t = StreamTable::new();
		assert!(
			learn(&mut t, &caps(1, ASSOC, BSSID, 0, None), &watched(), &counted(&[])).is_none()
		);
		assert_eq!(known(&t, 1), None);
	}

	#[test]
	fn probes_only_count_from_members() {
		let mut t = StreamTable::new();
		let devices = counted(&[1]);
		assert!(learn(&mut t, &caps(1, PROBE, BROADCAST, 2, None), &watched(), &devices).is_some());
		// a passer-by
		assert!(learn(&mut t, &caps(2, PROBE, BROADCAST, 2, None), &watched(), &devices).is_none());
		assert_eq!((known(&t, 1), known(&t, 2)), (Some((2, Source::Probe)), None));
	}

	#[test]
	fn a_known_member_needs_no_frame_this_period() {
		let mut t = StreamTable::new();
		learn(&mut t, &caps(1, PROBE, BROADCAST, 1, None), &watched(), &counted(&[1]));
		assert!(
			learn(&mut t, &caps(1, PROBE, BROADCAST, 2, None), &watched(), &counted(&[])).is_some()
		);
		assert_eq!(known(&t, 1), Some((2, Source::Probe)));
	}

	#[test]
	fn association_beats_probes() {
		let mut t = StreamTable::new();
		let devices = counted(&[1]);
		learn(&mut t, &caps(1, PROBE, BROADCAST, 3, None), &watched(), &devices);
		// the association request says less, and is still the one to trust
		assert!(learn(&mut t, &caps(1, ASSOC, BSSID, 2, None), &watched(), &devices).is_some());
		assert!(learn(&mut t, &caps(1, PROBE, BROADCAST, 3, None), &watched(), &devices).is_none());
		assert_eq!(known(&t, 1), Some((2, Source::Assoc)));
		// a later association replaces it
		assert!(learn(&mut t, &caps(1, ASSOC, BSSID, 1, None), &watched(), &devices).is_some());
		assert_eq!(known(&t, 1), Some((1, Source::Assoc)));
		// the same again is no change
		assert!(learn(&mut t, &caps(1, REASSOC, BSSID, 1, None), &watched(), &devices).is_none());
	}

	#[test]
	fn between_probes_the_larger_stays() {
		let mut t = StreamTable::new();
		let devices = counted(&[1]);
		learn(&mut t, &caps(1, PROBE, BROADCAST, 2, None), &watched(), &devices);
		// a trimmed probe, then one with HE
		assert!(learn(&mut t, &caps(1, PROBE, BROADCAST, 1, None), &watched(), &devices).is_none());
		assert!(
			learn(&mut t, &caps(1, PROBE, BROADCAST, 1, Some(3)), &watched(), &devices).is_some()
		);
		assert_eq!(known(&t, 1), Some((3, Source::Probe)));
	}

	#[test]
	fn a_full_table_drops_probes_first_then_the_oldest() {
		let mut t = StreamTable::new();
		let none = counted(&[]);
		// one probe count, the rest association counts
		learn(&mut t, &caps(0, PROBE, BROADCAST, 2, None), &watched(), &counted(&[0]));
		for last in 1..MAX_KNOWN as u8 {
			learn(&mut t, &caps(last, ASSOC, BSSID, 1, None), &watched(), &none);
		}
		let new = MAX_KNOWN as u8;

		// a probe never pushes out an association count
		assert!(
			learn(&mut t, &caps(new, PROBE, BROADCAST, 2, None), &watched(), &counted(&[new]))
				.is_some()
		);
		assert_eq!((known(&t, 0), known(&t, new)), (None, Some((2, Source::Probe))));
		assert!(
			learn(
				&mut t,
				&caps(new + 1, PROBE, BROADCAST, 2, None),
				&watched(),
				&counted(&[new + 1])
			)
			.is_some()
		);
		assert_eq!(known(&t, new), None);
		// now every count is from an association request
		learn(&mut t, &caps(new + 1, ASSOC, BSSID, 1, None), &watched(), &none);
		let probe = caps(new + 2, PROBE, BROADCAST, 2, None);
		assert!(learn(&mut t, &probe, &watched(), &counted(&[new + 2])).is_none());

		// an association count pushes out the oldest one
		assert!(learn(&mut t, &caps(new + 3, ASSOC, BSSID, 2, None), &watched(), &none).is_some());
		assert_eq!((known(&t, 1), known(&t, 2)), (None, Some((1, Source::Assoc))));
	}
}
