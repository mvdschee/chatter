//! How many streams each member says it can use (docs/streams.md). Kept for
//! the whole boot: a device asks to associate once, while the counters are
//! emptied every period.

use crate::models::{counters::MAX_DEVICES, frame::Mac};

pub const MAX_KNOWN: usize = MAX_DEVICES;

/// Where a count came from. Ordered by trust: an association request is to
/// our network, while a probe request may leave elements out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
	Probe,
	/// An association or reassociation request.
	Assoc,
}

impl Source {
	pub fn name(self) -> &'static str {
		match self {
			Source::Probe => "probe",
			Source::Assoc => "assoc",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamCount {
	pub mac: Mac,
	/// Above 0: an unknown count isn't kept.
	pub streams: u8,
	pub from: Source,
	/// When it was learned, in learning order: the oldest goes first when
	/// the table is full.
	pub(crate) learned: u32,
}

#[derive(Debug)]
pub struct StreamTable {
	pub(crate) entries: [Option<StreamCount>; MAX_KNOWN],
	pub(crate) next: u32,
}

impl StreamTable {
	pub const fn new() -> Self {
		Self {
			entries: [None; MAX_KNOWN],
			next: 0,
		}
	}

	pub fn get(&self, mac: &Mac) -> Option<StreamCount> {
		self.entries.iter().flatten().find(|c| c.mac == *mac).copied()
	}
}

impl Default for StreamTable {
	fn default() -> Self {
		Self::new()
	}
}
