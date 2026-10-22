//! The collector's answer to a post, which is how it switches the sensor's
//! mode: an empty body (`204`) is "count", `identify <secs>\n` is "identify
//! for that long". The newline ends the number, so an answer cut short inside
//! it is unreadable rather than a shorter session.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
	Count,
	Identify {
		secs: u32,
	},
	/// Unreadable: the sensor keeps the mode it's in.
	CarryOn,
}

pub fn parse_answer(body: &[u8]) -> Answer {
	if body.is_empty() {
		return Answer::Count;
	}
	let Some(digits) = body.strip_prefix(b"identify ").and_then(|rest| rest.strip_suffix(b"\n"))
	else {
		return Answer::CarryOn;
	};
	// `parse` would also take a leading `+`
	if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
		return Answer::CarryOn;
	}
	// all digits, so only a number too big for a u32 fails
	match core::str::from_utf8(digits).ok().and_then(|s| s.parse().ok()) {
		Some(0) => Answer::Count,
		Some(secs) => Answer::Identify {
			secs,
		},
		None => Answer::CarryOn,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_body_is_count() {
		assert_eq!(parse_answer(b""), Answer::Count);
	}

	#[test]
	fn identify_with_seconds() {
		assert_eq!(
			parse_answer(b"identify 600\n"),
			Answer::Identify {
				secs: 600
			}
		);
		assert_eq!(parse_answer(b"identify 0\n"), Answer::Count);
	}

	#[test]
	fn cut_short_is_carry_on() {
		for body in [&b"identify 600"[..], b"identify 60", b"identify ", b"identify", b"ident"] {
			assert_eq!(parse_answer(body), Answer::CarryOn, "{:?}", core::str::from_utf8(body));
		}
	}

	#[test]
	fn number_too_big_or_not_a_number_is_carry_on() {
		assert_eq!(
			parse_answer(b"identify 4294967295\n"),
			Answer::Identify {
				secs: u32::MAX
			}
		);
		for body in [
			&b"identify 4294967296\n"[..],
			b"identify +600\n",
			b"identify -1\n",
			b"identify 6 0\n",
			b"identify \n",
			b"count\n",
		] {
			assert_eq!(parse_answer(body), Answer::CarryOn, "{:?}", core::str::from_utf8(body));
		}
	}
}
