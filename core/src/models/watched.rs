//! The networks we count. The SSIDs stay in the firmware's config; core only
//! keeps the BSSID learned for each `net` index.

use crate::models::frame::Mac;

pub const MAX_NETS: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Watched {
	bssids: [Option<Mac>; MAX_NETS],
}

impl Watched {
	pub fn net_of(&self, bssid: &Mac) -> Option<u8> {
		self.bssids.iter().position(|b| b.as_ref() == Some(bssid)).map(|net| net as u8)
	}

	/// Sets the BSSID for `net`. Returns `true` when that changed something.
	pub fn learn(&mut self, net: u8, bssid: Mac) -> bool {
		match self.bssids.get_mut(net as usize) {
			Some(slot) if *slot != Some(bssid) => {
				*slot = Some(bssid);
				true
			},
			_ => false,
		}
	}
}

/// Position of `ssid` in the comma-separated watch list.
pub fn net_for_ssid(ssid: &[u8], watch_list: &str) -> Option<u8> {
	watch_list
		.split(',')
		.take(MAX_NETS)
		.position(|name| name.trim().as_bytes() == ssid)
		.map(|net| net as u8)
}

#[cfg(test)]
mod tests {
	use super::*;

	const A: Mac = Mac([0x02, 0, 0, 0, 0, 0xaa]);
	const B: Mac = Mac([0x02, 0, 0, 0, 0, 0xbb]);

	#[test]
	fn learn_and_look_up() {
		let mut w = Watched::default();
		assert_eq!(w.net_of(&A), None);
		assert!(w.learn(1, A));
		assert!(!w.learn(1, A));
		assert_eq!(w.net_of(&A), Some(1));
		// the router changed BSSID
		assert!(w.learn(1, B));
		assert_eq!((w.net_of(&A), w.net_of(&B)), (None, Some(1)));
	}

	#[test]
	fn learn_ignores_out_of_range_nets() {
		let mut w = Watched::default();
		assert!(!w.learn(MAX_NETS as u8, A));
		assert_eq!(w, Watched::default());
	}

	#[test]
	fn ssid_position() {
		assert_eq!(net_for_ssid(b"home", "home,iot"), Some(0));
		assert_eq!(net_for_ssid(b"iot", "home, iot"), Some(1));
		assert_eq!(net_for_ssid(b"guest", "home,iot"), None);
		assert_eq!(net_for_ssid(b"", "home,iot"), None);
		assert_eq!(net_for_ssid(b"e", "a,b,c,d,e"), None);
	}
}
