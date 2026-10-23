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

// requests a device sends, and where their elements start: association
// request: capability (2), listen interval (2); reassociation: plus the
// current AP (6); probe request: none
const SUBTYPE_ASSOC_REQUEST: u8 = 0;
const SUBTYPE_REASSOC_REQUEST: u8 = 2;
const ASSOC_ELEMENTS: usize = HEADER_LEN + 4;
const REASSOC_ELEMENTS: usize = HEADER_LEN + 10;
const PROBE_ELEMENTS: usize = HEADER_LEN;
// HT Capabilities body: capability info (2), A-MPDU (1), then the MCS set:
// the receive bitmask, one byte per stream for MCS 0-31 (MCS 32 and the
// unequal-modulation ones come after, and aren't a stream each), and the
// Tx parameters at byte 12 of it
const ELEMENT_HT: u8 = 45;
const HT_RX_MASK: usize = 3;
const HT_TX_PARAMS: usize = 15;
// HE Capabilities: an extension element; body: extension id (1), MAC
// capabilities (6), PHY capabilities (11), then the Rx and Tx HE-MCS maps
// for up to 80 MHz, 2 bits per stream count, 0b11 for not supported
const ELEMENT_EXTENSION: u8 = 255;
const EXT_HE: u8 = 35;
const HE_RX_MAP: usize = 18;
const HE_TX_MAP: usize = 20;

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

/// What a device says about its streams in an association, reassociation or
/// probe request (docs/streams.md). `streams()` is the one number; the raw
/// parts are what the spike logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
	pub station: Mac,
	/// The access point it asks, or broadcast for most probe requests.
	pub bssid: Mac,
	pub subtype: u8,
	pub ht: Option<Ht>,
	pub he: Option<He>,
}

impl Capabilities {
	/// The larger of HT's and HE's receive streams; 0 when neither says, as an
	/// HT element can carry an empty receive mask.
	pub fn streams(&self) -> u8 {
		let ht = self.ht.map_or(0, |h| h.rx_streams);
		let he = self.he.map_or(0, |h| h.rx_streams);
		ht.max(he)
	}

	pub fn is_probe(&self) -> bool {
		self.subtype == SUBTYPE_PROBE_REQUEST
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ht {
	/// Streams in the receive MCS bitmask.
	pub rx_streams: u8,
	/// Raw: bit 0 Tx set defined, bit 1 Tx not equal to Rx, bits 2-3 Tx
	/// streams minus one.
	pub tx_params: u8,
	/// Raw capability info bits 2-3: 0 static (one receive chain), 1
	/// dynamic, 3 off.
	pub sm_power_save: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct He {
	pub rx_streams: u8,
	pub tx_streams: u8,
	/// The element's body length: early drafts had a shorter PHY field.
	pub len: u8,
}

/// `Some` for every association, reassociation and probe request, with
/// whichever of HT and HE it carries. The sniffer's data ends in the FCS, so
/// an element running past the end ends the walk and keeps what was found.
pub fn capabilities(data: &[u8]) -> Option<Capabilities> {
	if data.len() < HEADER_LEN || (data[0] >> 2) & 0x03 != TYPE_MGMT {
		return None;
	}
	let subtype = data[0] >> 4;
	let start = match subtype {
		SUBTYPE_ASSOC_REQUEST => ASSOC_ELEMENTS,
		SUBTYPE_REASSOC_REQUEST => REASSOC_ELEMENTS,
		SUBTYPE_PROBE_REQUEST => PROBE_ELEMENTS,
		_ => return None,
	};
	let mut caps = Capabilities {
		station: Mac::at(data, ADDR2),
		bssid: Mac::at(data, ADDR3),
		subtype,
		ht: None,
		he: None,
	};

	let mut at = start;
	while at + 2 <= data.len() {
		let (id, len) = (data[at], data[at + 1] as usize);
		let Some(body) = data.get(at + 2..at + 2 + len) else {
			break;
		};
		match id {
			ELEMENT_HT if len > HT_TX_PARAMS => {
				caps.ht = Some(Ht {
					rx_streams: body[HT_RX_MASK..HT_RX_MASK + 4].iter().filter(|b| **b != 0).count()
						as u8,
					tx_params: body[HT_TX_PARAMS],
					sm_power_save: (body[0] >> 2) & 0x03,
				});
			},
			ELEMENT_EXTENSION if len > HE_TX_MAP + 1 && body[0] == EXT_HE => {
				caps.he = Some(He {
					rx_streams: he_streams(body, HE_RX_MAP),
					tx_streams: he_streams(body, HE_TX_MAP),
					len: len as u8,
				});
			},
			_ => {},
		}
		at += 2 + len;
	}
	Some(caps)
}

fn he_streams(body: &[u8], at: usize) -> u8 {
	let map = u16::from_le_bytes([body[at], body[at + 1]]);
	(0..8).filter(|n| (map >> (2 * n)) & 0b11 != 0b11).count() as u8
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

	const REASSOC_REQUEST: u8 = (TYPE_MGMT << 2) | (2 << 4);

	/// HT Capabilities with the given receive bitmask's first five bytes.
	fn ht(info: u16, rx_mask: [u8; 5], tx_params: u8) -> Vec<u8> {
		let mut body = vec![0; 26];
		body[..2].copy_from_slice(&info.to_le_bytes());
		body[HT_RX_MASK..HT_RX_MASK + 5].copy_from_slice(&rx_mask);
		body[HT_TX_PARAMS] = tx_params;
		[vec![ELEMENT_HT, 26], body].concat()
	}

	fn he(rx_map: u16, tx_map: u16, len: u8) -> Vec<u8> {
		let mut body = vec![0; len as usize];
		body[0] = EXT_HE;
		if len as usize > HE_TX_MAP + 1 {
			body[HE_RX_MAP..HE_RX_MAP + 2].copy_from_slice(&rx_map.to_le_bytes());
			body[HE_TX_MAP..HE_TX_MAP + 2].copy_from_slice(&tx_map.to_le_bytes());
		}
		[vec![ELEMENT_EXTENSION, len], body].concat()
	}

	/// A request with its fixed fields zeroed, then `elements`, then an FCS.
	fn request(fc0: u8, a1: Mac, a3: Mac, fixed: usize, elements: &[Vec<u8>]) -> Vec<u8> {
		let mut f = header(fc0, 0, a1, STATION, a3).to_vec();
		f.extend(vec![0; fixed]);
		// supported rates first, to check the walk
		f.extend_from_slice(&[1, 2, 0x82, 0x84]);
		for e in elements {
			f.extend_from_slice(e);
		}
		f.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
		f
	}

	#[test]
	fn association_request_with_two_ht_streams() {
		let f = request(ASSOC_REQUEST, BSSID, BSSID, 4, &[ht(0x000c, [0xff, 0xff, 0, 0, 0], 0)]);
		let c = capabilities(&f).unwrap();
		assert_eq!((c.station, c.bssid, c.subtype), (STATION, BSSID, 0));
		assert_eq!(
			c.ht,
			Some(Ht {
				rx_streams: 2,
				tx_params: 0,
				sm_power_save: 3
			})
		);
		assert_eq!(c.he, None);
	}

	#[test]
	fn reassociation_and_probe_requests() {
		let r = request(REASSOC_REQUEST, BSSID, BSSID, 10, &[ht(0, [0xff, 0, 0, 0, 0], 0)]);
		assert_eq!(capabilities(&r).unwrap().ht.unwrap().rx_streams, 1);
		let p = request(PROBE_REQUEST, BROADCAST, BROADCAST, 0, &[ht(0, [0xff; 5], 0)]);
		let c = capabilities(&p).unwrap();
		assert_eq!((c.bssid, c.subtype, c.ht.unwrap().rx_streams), (BROADCAST, 4, 4));
	}

	#[test]
	fn mcs_32_is_not_a_stream() {
		let f = request(ASSOC_REQUEST, BSSID, BSSID, 4, &[ht(0, [0xff, 0, 0, 0, 0x01], 0)]);
		assert_eq!(capabilities(&f).unwrap().ht.unwrap().rx_streams, 1);
	}

	#[test]
	fn ht_raw_parts() {
		// static SM power save; Tx defined, not equal, 2 streams
		let f = request(ASSOC_REQUEST, BSSID, BSSID, 4, &[ht(0x0000, [0xff; 5], 0b0111)]);
		let h = capabilities(&f).unwrap().ht.unwrap();
		assert_eq!((h.sm_power_save, h.tx_params), (0, 0b0111));
	}

	#[test]
	fn he_streams_from_the_maps() {
		// Rx: 1 and 2 streams at MCS 0-11, the rest not supported; Tx: 1
		let elements = [ht(0, [0xff, 0xff, 0, 0, 0], 0), he(0xfffa, 0xfffe, 22)];
		let c = capabilities(&request(ASSOC_REQUEST, BSSID, BSSID, 4, &elements)).unwrap();
		assert_eq!(
			c.he,
			Some(He {
				rx_streams: 2,
				tx_streams: 1,
				len: 22
			})
		);
		assert_eq!(c.ht.unwrap().rx_streams, 2);
	}

	#[test]
	fn a_short_he_element_is_left_out() {
		let elements = [he(0, 0, 12), ht(0, [0xff, 0, 0, 0, 0], 0)];
		let c = capabilities(&request(ASSOC_REQUEST, BSSID, BSSID, 4, &elements)).unwrap();
		assert_eq!((c.he, c.ht.unwrap().rx_streams), (None, 1));
	}

	#[test]
	fn an_empty_element_doesnt_end_the_walk() {
		let elements = [vec![221, 0], ht(0, [0xff, 0xff, 0xff, 0, 0], 0)];
		let c = capabilities(&request(ASSOC_REQUEST, BSSID, BSSID, 4, &elements)).unwrap();
		assert_eq!(c.ht.unwrap().rx_streams, 3);
	}

	#[test]
	fn an_element_cut_short_keeps_what_was_found() {
		// a vendor element claiming more than is left, then the FCS
		let elements = [ht(0, [0xff, 0xff, 0, 0, 0], 0), vec![221, 40, 0, 0x50, 0xf2]];
		let c = capabilities(&request(ASSOC_REQUEST, BSSID, BSSID, 4, &elements)).unwrap();
		assert_eq!(c.ht.unwrap().rx_streams, 2);
	}

	#[test]
	fn a_request_without_either() {
		let c = capabilities(&request(PROBE_REQUEST, BROADCAST, BROADCAST, 0, &[])).unwrap();
		assert_eq!((c.ht, c.he, c.streams()), (None, None, 0));
	}

	#[test]
	fn streams_is_the_larger_rx_count() {
		let streams = |elements: &[Vec<u8>]| {
			capabilities(&request(ASSOC_REQUEST, BSSID, BSSID, 4, elements)).unwrap().streams()
		};
		assert_eq!(streams(&[ht(0, [0xff, 0, 0, 0, 0], 0), he(0xfffa, 0xfffe, 22)]), 2);
		assert_eq!(streams(&[ht(0, [0xff, 0xff, 0xff, 0, 0], 0), he(0xfffe, 0xfffe, 22)]), 3);
		assert_eq!(streams(&[he(0xfffe, 0xfffe, 22)]), 1);
		// an empty receive mask says nothing
		assert_eq!(streams(&[ht(0, [0; 5], 0)]), 0);
	}

	#[test]
	fn other_frames_have_no_capabilities() {
		let elements = [ht(0, [0xff; 5], 0)];
		assert!(capabilities(&request(PROBE_RESPONSE, STATION, BSSID, 12, &elements)).is_none());
		assert!(capabilities(&request(BEACON, BROADCAST, BSSID, 12, &elements)).is_none());
		assert!(capabilities(&request(ACTION, BSSID, BSSID, 0, &elements)).is_none());
		assert!(capabilities(&request(DATA, BSSID, BSSID, 0, &elements)).is_none());
		assert!(capabilities(&header(ASSOC_REQUEST, 0, BSSID, STATION, BSSID)[..20]).is_none());
	}
}
