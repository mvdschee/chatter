//! Reports waiting to be sent, oldest first, packed into one fixed buffer. When
//! it's full the oldest report goes, so after an outage the collector gets the
//! most recent minutes.

/// A FIFO of byte strings tagged with their `seq`. `BYTES` bounds the total
/// size, `MAX` the number of entries.
pub struct Outbox<const BYTES: usize, const MAX: usize> {
	bytes: [u8; BYTES],
	/// `(seq, len)` per entry, oldest first; entry `i` starts where `i - 1` ends.
	entries: [(u32, usize); MAX],
	count: usize,
	used: usize,
}

impl<const BYTES: usize, const MAX: usize> Outbox<BYTES, MAX> {
	pub const fn new() -> Self {
		Self {
			bytes: [0; BYTES],
			entries: [(0, 0); MAX],
			count: 0,
			used: 0,
		}
	}

	pub fn len(&self) -> usize {
		self.count
	}

	pub fn is_empty(&self) -> bool {
		self.count == 0
	}

	/// Appends a report, dropping the oldest until it fits. Returns how many
	/// were dropped. A report bigger than the whole buffer isn't stored.
	pub fn push(&mut self, seq: u32, report: &[u8]) -> Result<u32, TooBig> {
		if report.len() > BYTES {
			return Err(TooBig);
		}
		let mut dropped = 0;
		while self.count == MAX || self.used + report.len() > BYTES {
			self.pop_front();
			dropped += 1;
		}
		self.bytes[self.used..self.used + report.len()].copy_from_slice(report);
		self.entries[self.count] = (seq, report.len());
		self.count += 1;
		self.used += report.len();
		Ok(dropped)
	}

	/// The oldest report: `(seq, bytes)`.
	pub fn front(&self) -> Option<(u32, &[u8])> {
		let &(seq, len) = self.entries[..self.count].first()?;
		Some((seq, &self.bytes[..len]))
	}

	/// Removes the oldest report and moves the rest to the front.
	pub fn pop_front(&mut self) {
		if self.count == 0 {
			return;
		}
		let (_, len) = self.entries[0];
		self.bytes.copy_within(len..self.used, 0);
		self.entries.copy_within(1..self.count, 0);
		self.count -= 1;
		self.used -= len;
	}
}

impl<const BYTES: usize, const MAX: usize> Default for Outbox<BYTES, MAX> {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Debug, PartialEq, Eq)]
pub struct TooBig;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn first_in_first_out() {
		let mut o = Outbox::<64, 4>::new();
		o.push(1, b"one").unwrap();
		o.push(2, b"two!").unwrap();
		assert_eq!(o.front(), Some((1, &b"one"[..])));
		o.pop_front();
		assert_eq!(o.front(), Some((2, &b"two!"[..])));
		o.pop_front();
		assert_eq!(o.front(), None);
		assert!(o.is_empty());
	}

	#[test]
	fn full_bytes_drop_the_oldest() {
		let mut o = Outbox::<10, 8>::new();
		o.push(1, b"aaaa").unwrap();
		o.push(2, b"bbbb").unwrap();
		// 4 + 4 + 4 > 10: report 1 goes
		assert_eq!(o.push(3, b"cccc"), Ok(1));
		assert_eq!((o.len(), o.front()), (2, Some((2, &b"bbbb"[..]))));
		o.pop_front();
		assert_eq!(o.front(), Some((3, &b"cccc"[..])));
	}

	#[test]
	fn full_entries_drop_the_oldest() {
		let mut o = Outbox::<64, 2>::new();
		o.push(1, b"a").unwrap();
		o.push(2, b"b").unwrap();
		assert_eq!(o.push(3, b"c"), Ok(1));
		assert_eq!(o.front(), Some((2, &b"b"[..])));
	}

	#[test]
	fn a_report_as_big_as_the_buffer_replaces_everything() {
		let mut o = Outbox::<8, 4>::new();
		o.push(1, b"ab").unwrap();
		o.push(2, b"cd").unwrap();
		assert_eq!(o.push(3, b"12345678"), Ok(2));
		assert_eq!(o.front(), Some((3, &b"12345678"[..])));
		assert_eq!(o.push(4, b"123456789"), Err(TooBig));
		assert_eq!(o.len(), 1);
	}

	#[test]
	fn survives_many_rounds() {
		let mut o = Outbox::<16, 3>::new();
		for seq in 0..100u32 {
			let report = [seq as u8; 5];
			o.push(seq, &report).unwrap();
			if seq % 2 == 0 {
				o.pop_front();
			}
			// whatever is left must still be intact after all the shifting
			if let Some((front, bytes)) = o.front() {
				assert_eq!(bytes, &[front as u8; 5]);
			}
		}
	}
}
