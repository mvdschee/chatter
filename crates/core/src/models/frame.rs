//! Decoding the 802.11 header the sniffer hands us. Fixed offsets only: this
//! runs in the radio driver's receive path for every frame.

use crate::models::watched::Watched;

const HEADER_LEN: usize = 24;
const ADDR1: usize = 4;
const ADDR2: usize = 10;
const ADDR3: usize = 16;
// sequence control: fragment number (bits 0-3), sequence number (bits 4-15)
const SEQ_CTRL: usize = 22;

// frame control, byte 0: type in bits 2-3, subtype in bits 4-7
const TYPE_MGMT: u8 = 0;
const TYPE_DATA: u8 = 2;
const SUBTYPE_PROBE_REQUEST: u8 = 4;
const SUBTYPE_PROBE_RESPONSE: u8 = 5;
const SUBTYPE_BEACON: u8 = 8;
// frame control, byte 1
const TO_DS: u8 = 0x01;
const FROM_DS: u8 = 0x02;
const RETRY: u8 = 0x08;

// beacon and probe response body: timestamp (8), interval (2), capabilities
// (2), then elements
const ANNOUNCEMENT_ELEMENTS: usize = HEADER_LEN + 12;
const ELEMENT_SSID: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mac(pub [u8; 6]);

impl Mac {
	/// Broadcast or multicast: the group bit of the first byte.
	pub fn is_group(&self) -> bool {
		self.0[0] & 0x01 != 0
	}

	fn at(data: &[u8], offset: usize) -> Mac {
		let mut mac = [0; 6];
		mac.copy_from_slice(&data[offset..offset + 6]);
		Mac(mac)
	}
}

/// Seen from the station: `Up` is the device sending, `Down` the router
/// sending to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
	Up,
	Down,
}

/// One frame from a watched network, about 14 bytes. When `station` is a
/// group address, it's a broadcast or multicast from the router to everyone.
/// `seq` is the raw sequence control: a retransmission repeats it exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
	pub station: Mac,
	pub net: u8,
	pub direction: Direction,
	pub len: u16,
	pub rssi: i8,
	pub retry: bool,
	pub seq: u16,
}

/// Applies the address table from docs/sensor.md. `None` for frames from
/// other networks, control frames, beacons, probes, WDS, and anything too
/// short. Probe requests and responses come from devices *looking* for a
/// network, mostly passing phones with random MACs, not from members of it.
pub fn decode(data: &[u8], len: u16, rssi: i8, watched: &Watched) -> Option<Frame> {
	if data.len() < HEADER_LEN {
		return None;
	}
	let kind = (data[0] >> 2) & 0x03;
	let subtype = data[0] >> 4;
	let flags = data[1];

	let (bssid, station, direction) = match (kind, flags & TO_DS != 0, flags & FROM_DS != 0) {
		(TYPE_DATA, true, false) => (Mac::at(data, ADDR1), Mac::at(data, ADDR2), Direction::Up),
		(TYPE_DATA, false, true) => (Mac::at(data, ADDR2), Mac::at(data, ADDR1), Direction::Down),
		(TYPE_MGMT, false, false)
			if !matches!(
				subtype,
				SUBTYPE_BEACON | SUBTYPE_PROBE_REQUEST | SUBTYPE_PROBE_RESPONSE
			) =>
		{
			let bssid = Mac::at(data, ADDR3);
			let (addr1, addr2) = (Mac::at(data, ADDR1), Mac::at(data, ADDR2));
			if addr2 == bssid {
				(bssid, addr1, Direction::Down)
			} else if addr1 == bssid {
				(bssid, addr2, Direction::Up)
			} else {
				return None;
			}
		},
		_ => return None,
	};

	Some(Frame {
		station,
		net: watched.net_of(&bssid)?,
		direction,
		len,
		rssi,
		retry: flags & RETRY != 0,
		seq: u16::from_le_bytes([data[SEQ_CTRL], data[SEQ_CTRL + 1]]),
	})
}

/// `(bssid, ssid)` if this frame announces a network: a beacon, or a probe
/// response. While connected, the C6's driver passes few beacons from other
/// access points, but it does pass their probe responses, which carry the same
/// SSID element. Hidden networks give an empty or zeroed SSID.
pub fn announcement(data: &[u8]) -> Option<(Mac, &[u8])> {
	if data.len() < ANNOUNCEMENT_ELEMENTS {
		return None;
	}
	let kind = (data[0] >> 2) & 0x03;
	let subtype = data[0] >> 4;
	if kind != TYPE_MGMT || !matches!(subtype, SUBTYPE_BEACON | SUBTYPE_PROBE_RESPONSE) {
		return None;
	}

	let mut at = ANNOUNCEMENT_ELEMENTS;
	while at + 2 <= data.len() {
		let (id, len) = (data[at], data[at + 1] as usize);
		let body = data.get(at + 2..at + 2 + len)?;
		if id == ELEMENT_SSID {
			return Some((Mac::at(data, ADDR3), body));
		}
		at += 2 + len;
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	const BSSID: Mac = Mac([0x02, 0, 0, 0, 0, 0xaa]);
	const OTHER_BSSID: Mac = Mac([0x02, 0, 0, 0, 0, 0xbb]);
	const STATION: Mac = Mac([0x02, 0, 0, 0, 0, 0x01]);
	const PEER: Mac = Mac([0x02, 0, 0, 0, 0, 0x02]);
	const BROADCAST: Mac = Mac([0xff; 6]);

	const DATA: u8 = TYPE_DATA << 2;
	const QOS_DATA: u8 = (TYPE_DATA << 2) | (8 << 4);
	const PROBE_REQUEST: u8 = (TYPE_MGMT << 2) | (4 << 4);
	const PROBE_RESPONSE: u8 = (TYPE_MGMT << 2) | (5 << 4);
	const ASSOC_REQUEST: u8 = TYPE_MGMT << 2;
	const ACTION: u8 = (TYPE_MGMT << 2) | (13 << 4);
	const BEACON: u8 = (TYPE_MGMT << 2) | (SUBTYPE_BEACON << 4);
	const ACK: u8 = (1 << 2) | (13 << 4);

	fn header(fc0: u8, fc1: u8, a1: Mac, a2: Mac, a3: Mac) -> [u8; HEADER_LEN] {
		let mut h = [0; HEADER_LEN];
		h[0] = fc0;
		h[1] = fc1;
		h[ADDR1..ADDR1 + 6].copy_from_slice(&a1.0);
		h[ADDR2..ADDR2 + 6].copy_from_slice(&a2.0);
		h[ADDR3..ADDR3 + 6].copy_from_slice(&a3.0);
		h
	}

	fn watched() -> Watched {
		let mut w = Watched::default();
		w.learn(1, BSSID);
		w
	}

	fn decode_header(h: &[u8]) -> Option<Frame> {
		decode(h, 100, -40, &watched())
	}

	#[test]
	fn frame_stays_small() {
		// it's copied through the sniffer queue for every frame
		assert!(core::mem::size_of::<Frame>() <= 14);
	}

	#[test]
	fn upload_is_addr2() {
		let f = decode_header(&header(DATA, TO_DS, BSSID, STATION, PEER)).unwrap();
		assert_eq!((f.station, f.direction, f.net), (STATION, Direction::Up, 1));
		assert_eq!((f.len, f.rssi, f.retry), (100, -40, false));
	}

	#[test]
	fn download_is_addr1() {
		let f = decode_header(&header(QOS_DATA, FROM_DS, STATION, BSSID, PEER)).unwrap();
		assert_eq!((f.station, f.direction), (STATION, Direction::Down));
	}

	#[test]
	fn group_download_keeps_the_group_address() {
		let f = decode_header(&header(DATA, FROM_DS, BROADCAST, BSSID, PEER)).unwrap();
		assert!(f.station.is_group());
		assert_eq!(f.direction, Direction::Down);
	}

	#[test]
	fn management_both_ways() {
		let down = decode_header(&header(ACTION, 0, STATION, BSSID, BSSID)).unwrap();
		assert_eq!((down.station, down.direction), (STATION, Direction::Down));
		let up = decode_header(&header(ASSOC_REQUEST, 0, BSSID, STATION, BSSID)).unwrap();
		assert_eq!((up.station, up.direction), (STATION, Direction::Up));
	}

	#[test]
	fn sequence_control() {
		let mut h = header(DATA, TO_DS, BSSID, STATION, PEER);
		// sequence number 0x123, fragment 4
		h[SEQ_CTRL..SEQ_CTRL + 2].copy_from_slice(&0x1234u16.to_le_bytes());
		assert_eq!(decode_header(&h).unwrap().seq, 0x1234);
	}

	#[test]
	fn retry_bit() {
		let f = decode_header(&header(DATA, TO_DS | RETRY, BSSID, STATION, PEER)).unwrap();
		assert!(f.retry);
	}

	#[test]
	fn ignored_frames() {
		// another network
		assert!(decode_header(&header(DATA, TO_DS, OTHER_BSSID, STATION, PEER)).is_none());
		// WDS / mesh
		assert!(decode_header(&header(DATA, TO_DS | FROM_DS, BSSID, STATION, PEER)).is_none());
		// control
		assert!(decode_header(&header(ACK, 0, STATION, BSSID, BSSID)).is_none());
		// beacons go through `announcement()`
		assert!(decode_header(&header(BEACON, 0, BROADCAST, BSSID, BSSID)).is_none());
		// probes: a device looking for networks, not a member of ours
		assert!(decode_header(&header(PROBE_REQUEST, 0, BROADCAST, STATION, BROADCAST)).is_none());
		assert!(decode_header(&header(PROBE_REQUEST, 0, BSSID, STATION, BSSID)).is_none());
		assert!(decode_header(&header(PROBE_RESPONSE, 0, STATION, BSSID, BSSID)).is_none());
		// too short (MIMO length-only)
		assert!(decode_header(&header(DATA, TO_DS, BSSID, STATION, PEER)[..20]).is_none());
	}

	fn announcing(fc0: u8, a1: Mac, ssid: &[u8]) -> Vec<u8> {
		let mut f = header(fc0, 0, a1, BSSID, BSSID).to_vec();
		f.extend_from_slice(&[0; 12]);
		// a non-SSID element first, to check the walk
		f.extend_from_slice(&[1, 2, 0x82, 0x84]);
		f.extend_from_slice(&[ELEMENT_SSID, ssid.len() as u8]);
		f.extend_from_slice(ssid);
		f
	}

	#[test]
	fn beacon_announces() {
		let f = announcing(BEACON, BROADCAST, b"home");
		assert_eq!(announcement(&f), Some((BSSID, &b"home"[..])));
	}

	#[test]
	fn probe_response_announces() {
		let f = announcing(PROBE_RESPONSE, STATION, b"iot");
		assert_eq!(announcement(&f), Some((BSSID, &b"iot"[..])));
	}

	#[test]
	fn other_frames_and_truncation_announce_nothing() {
		// same layout, but a probe request comes from a device, not a network
		assert!(announcement(&announcing(PROBE_REQUEST, BSSID, b"home")).is_none());
		assert!(announcement(&announcing(ACTION, STATION, b"home")).is_none());
		let full = announcing(BEACON, BROADCAST, b"home");
		assert!(announcement(&full[..full.len() - 1]).is_none());
	}
}
