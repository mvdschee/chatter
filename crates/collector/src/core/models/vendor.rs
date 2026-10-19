//! Who made a device, from its MAC's first three bytes: IEEE's MA-L registry
//! (`oui.csv`). nmap has its own copy, as old as the nmap package, so newer
//! devices show up without a vendor there.

use std::{
	collections::HashMap,
	sync::{Arc, RwLock},
};

use crate::{Error, Result, core::models::report::Mac};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Vendors(HashMap<String, String>);

/// The vendor list, shared by the scans and replaced when it's refreshed.
pub type SharedVendors = Arc<RwLock<Vendors>>;

impl Vendors {
	/// IEEE's CSV: `Registry,Assignment,Organization Name,Organization Address`,
	/// with the assignment as six hex digits.
	pub fn from_csv(text: &str) -> Result<Self> {
		let mut map = HashMap::new();
		for row in csv::Reader::from_reader(text.as_bytes()).records() {
			let row = row.map_err(|e| Error::Scan(format!("oui.csv: {e}")))?;
			if let (Some(prefix), Some(name)) = (row.get(1), row.get(2)) {
				map.insert(prefix.to_ascii_lowercase(), name.trim().to_string());
			}
		}
		Ok(Self(map))
	}

	pub fn len(&self) -> usize {
		self.0.len()
	}

	/// A MAC with the "locally administered" bit set is made up (phones'
	/// private addresses), so it has no vendor.
	pub fn lookup(&self, mac: &Mac) -> Option<&str> {
		let prefix: String = mac.split(':').take(3).collect();
		self.0.get(&prefix).map(String::as_str)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn looks_up_the_first_three_bytes() {
		let csv = "Registry,Assignment,Organization Name,Organization Address\n\
			MA-L,AABBCC,\"Example Devices, Inc.\",Somewhere 1\n\
			MA-L,00000C,Another Maker ,Elsewhere\n";
		let v = Vendors::from_csv(csv).unwrap();
		assert_eq!(v.len(), 2);
		assert_eq!(
			v.lookup(&Mac::new("AA:BB:CC:00:00:01").unwrap()),
			Some("Example Devices, Inc.")
		);
		assert_eq!(v.lookup(&Mac::new("00:00:0c:12:34:56").unwrap()), Some("Another Maker"));
		assert_eq!(v.lookup(&Mac::new("02:00:00:00:00:01").unwrap()), None);
	}
}
