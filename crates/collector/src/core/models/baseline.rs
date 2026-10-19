//! Each device's usual traffic per hour of the day, so the plan can colour a
//! device by how unusual a minute is for it rather than on one scale for
//! all. The median and the MAD of `ln(1 + bytes a minute)`, sent and received
//! added. See `docs/outliers.md`.

use std::collections::{BTreeMap, HashMap};

use chrono::{Local, TimeZone, Timelike};
use serde::Serialize;

/// The baselines cover this much before the newest report.
pub const WINDOW_S: i64 = 14 * 86_400;
/// An hour with fewer of the device's minutes uses its baseline over all
/// hours instead.
pub const MIN_HOUR_MINUTES: usize = 60;
/// A device with fewer minutes in all is still learning: a day's worth.
pub const LEARNING_MINUTES: usize = 1440;
/// The `hour` the baselines table stores a device's baseline over all hours
/// under.
pub const ALL_HOURS: usize = 24;

/// A report: one minute of the sensor listening.
#[derive(Debug, Clone, Copy)]
pub struct Minute {
	pub report: i64,
	pub at: i64,
	/// `hour_of(at)`
	pub hour: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
	pub median: f64,
	pub mad: f64,
	/// The device's minutes in this hour, also when the baseline is the
	/// fallback.
	pub n: usize,
}

#[derive(Debug)]
pub struct DeviceBaselines {
	pub mac: String,
	/// One per hour of the day.
	pub hours: [Baseline; 24],
	/// Over all hours: for a stretch longer than an hour, and for whether the
	/// device is still learning.
	pub all: Baseline,
}

/// A device with no spread at all (a MAD of 0) reaches red at this many
/// times its usual traffic. The MAD floor follows from it.
pub const RED_AT_TIMES_USUAL: f64 = 10.0;

/// How the page turns a device's bytes a minute into a score:
/// `k × (ln(1 + bytes) − median) / max(mad, floor)`, below 0 counting as 0.
/// White up to `white`, red from `red`. All of it comes from here, so the
/// floor is derived in one place.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ScoreScale {
	/// Makes the MAD comparable to a standard deviation (Iglewicz and
	/// Hoaglin).
	pub k: f64,
	pub floor: f64,
	pub white: f64,
	pub red: f64,
}

impl Default for ScoreScale {
	fn default() -> Self {
		let (k, red) = (0.6745, 3.5);
		Self {
			k,
			floor: k * RED_AT_TIMES_USUAL.ln() / red,
			white: 2.0,
			red,
		}
	}
}

/// A device's baselines as the page gets them.
#[derive(Debug, Clone, Serialize)]
pub struct UsualTraffic {
	/// `[median, mad]` per hour of the day, in the collector's time zone.
	pub hours: Vec<[f64; 2]>,
	/// `[median, mad]` over all hours.
	pub all: [f64; 2],
	/// Under a day of minutes so far: not coloured by its score yet.
	pub learning: bool,
}

/// The stored baselines (`(mac, hour, median, mad, n)` rows) per device. A
/// device without all 25 rows is left out: the page treats it as learning.
pub fn usual_traffic(rows: &[(String, i64, f64, f64, i64)]) -> HashMap<String, UsualTraffic> {
	// per device, by hour: `(median, mad, n)`
	type Slots = [Option<(f64, f64, i64)>; ALL_HOURS + 1];
	let mut per_device: HashMap<&str, Slots> = HashMap::new();
	for (mac, hour, median, mad, n) in rows {
		if let Some(slot) = usize::try_from(*hour).ok().filter(|&h| h <= ALL_HOURS) {
			per_device.entry(mac).or_insert([None; ALL_HOURS + 1])[slot] =
				Some((*median, *mad, *n));
		}
	}
	per_device
		.into_iter()
		.filter_map(|(mac, slots)| {
			let slots: Vec<(f64, f64, i64)> = slots.into_iter().collect::<Option<_>>()?;
			let (median, mad, n) = slots[ALL_HOURS];
			Some((
				mac.to_string(),
				UsualTraffic {
					hours: slots[..ALL_HOURS].iter().map(|&(m, d, _)| [m, d]).collect(),
					all: [median, mad],
					learning: (n as usize) < LEARNING_MINUTES,
				},
			))
		})
		.collect()
}

/// The hour of the day of `t`, in the collector's time zone: the same clock
/// as `CHATTER_PORT_SCAN_AT`.
pub fn hour_of(t: i64) -> usize {
	Local.timestamp_opt(t, 0).single().map_or(0, |d| d.hour() as usize)
}

/// Every device's baselines from the `minutes` in the window, the bytes it was
/// `heard` with per report (`(report, mac, bytes)`), and the port `scans`
/// (`(started_at, finished_at)`).
///
/// A minute the device wasn't heard in counts as 0 bytes, from the first
/// minute it was heard on: before that it wasn't on the network, and zeros
/// there would make a new device look idle. The minutes during a port scan
/// are left out, as it makes every device busy.
pub fn baselines(
	minutes: &[Minute],
	heard: &[(i64, String, i64)],
	scans: &[(i64, i64)],
) -> Vec<DeviceBaselines> {
	let at: HashMap<i64, i64> = minutes.iter().map(|m| (m.report, m.at)).collect();
	let mut devices: BTreeMap<&str, (i64, HashMap<i64, i64>)> = BTreeMap::new();
	for (report, mac, bytes) in heard {
		let Some(&t) = at.get(report) else {
			continue;
		};
		let (first, per_report) = devices.entry(mac).or_insert((t, HashMap::new()));
		*first = (*first).min(t);
		*per_report.entry(*report).or_default() += bytes;
	}

	let usable: Vec<&Minute> = minutes
		.iter()
		.filter(|m| !scans.iter().any(|&(from, to)| m.at >= from && m.at <= to))
		.collect();

	devices
		.into_iter()
		.filter_map(|(mac, (first, per_report))| {
			let mut by_hour: [Vec<f64>; 24] = Default::default();
			for m in usable.iter().filter(|m| m.at >= first) {
				let bytes = per_report.get(&m.report).copied().unwrap_or(0);
				by_hour[m.hour % 24].push((bytes.max(0) as f64).ln_1p());
			}
			let mut all: Vec<f64> = by_hour.iter().flatten().copied().collect();
			if all.is_empty() {
				return None;
			}
			let overall = summarise(&mut all);
			let hours = by_hour.map(|mut values| {
				let n = values.len();
				if n >= MIN_HOUR_MINUTES {
					summarise(&mut values)
				} else {
					Baseline {
						n,
						..overall
					}
				}
			});
			Some(DeviceBaselines {
				mac: mac.to_string(),
				hours,
				all: overall,
			})
		})
		.collect()
}

/// The median and the MAD of `values` (not empty), which it sorts.
fn summarise(values: &mut [f64]) -> Baseline {
	values.sort_by(f64::total_cmp);
	let median = median_of_sorted(values);
	let mut deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
	deviations.sort_by(f64::total_cmp);
	Baseline {
		median,
		mad: median_of_sorted(&deviations),
		n: values.len(),
	}
}

fn median_of_sorted(values: &[f64]) -> f64 {
	let mid = values.len() / 2;
	if values.len().is_multiple_of(2) {
		(values[mid - 1] + values[mid]) / 2.0
	} else {
		values[mid]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const DEVICE: &str = "ea:28:ba:6f:02:24";
	const OTHER: &str = "5e:43:4c:48:00:1c";
	const START: i64 = 1_700_000_000;

	/// `count` minutes from `START`, the hour going round every 60.
	fn minutes(count: usize) -> Vec<Minute> {
		(0..count)
			.map(|i| Minute {
				report: i as i64 + 1,
				at: START + 60 * i as i64,
				hour: (i / 60) % 24,
			})
			.collect()
	}

	fn heard(
		minutes: &[Minute],
		mac: &str,
		bytes: impl Fn(usize) -> Option<i64>,
	) -> Vec<(i64, String, i64)> {
		minutes
			.iter()
			.enumerate()
			.filter_map(|(i, m)| bytes(i).map(|b| (m.report, mac.to_string(), b)))
			.collect()
	}

	fn close(a: f64, b: f64) -> bool {
		(a - b).abs() < 1e-9
	}

	fn device<'a>(all: &'a [DeviceBaselines], mac: &str) -> &'a DeviceBaselines {
		all.iter().find(|d| d.mac == mac).unwrap()
	}

	#[test]
	fn a_steady_device_has_no_spread() {
		let m = minutes(2 * 1440);
		let b = baselines(&m, &heard(&m, DEVICE, |_| Some(10_000)), &[]);
		for hour in b[0].hours {
			assert!(close(hour.median, 10_001f64.ln()));
			assert_eq!((hour.mad, hour.n), (0.0, 120));
		}
	}

	#[test]
	fn the_minutes_it_was_not_heard_in_count_as_zero() {
		// heard once an hour: an idle device, not one that sends 50 KiB
		let m = minutes(2 * 1440);
		let b = baselines(&m, &heard(&m, DEVICE, |i| (i % 60 == 0).then_some(50_000)), &[]);
		assert_eq!(b[0].hours[0].median, 0.0);
		assert_eq!(b[0].hours[0].n, 120);
	}

	#[test]
	fn one_spike_does_not_move_the_median() {
		let m = minutes(2 * 1440);
		let quiet = baselines(&m, &heard(&m, DEVICE, |_| Some(1_000)), &[]);
		let spike = baselines(
			&m,
			&heard(&m, DEVICE, |i| {
				Some(if i == 30 {
					10_000_000
				} else {
					1_000
				})
			}),
			&[],
		);
		assert_eq!(spike[0].hours[0].median, quiet[0].hours[0].median);
	}

	#[test]
	fn zeros_start_at_the_first_minute_it_was_heard() {
		// another device has been there for two days; this one for the last hour
		let m = minutes(2 * 1440);
		let mut h = heard(&m, OTHER, |_| Some(1_000));
		h.extend(heard(&m, DEVICE, |i| (i >= 2 * 1440 - 60).then_some(5_000)));
		let b = baselines(&m, &h, &[]);
		let hours = device(&b, DEVICE).hours;
		assert_eq!(hours.iter().map(|h| h.n).sum::<usize>(), 60);
		assert!(close(hours[23].median, 5_001f64.ln()), "not a median of 0 from before it came");
	}

	#[test]
	fn a_thin_hour_falls_back_to_all_hours() {
		// two hours: the first heard for 60 minutes, the second for 59
		let m = minutes(119);
		let b = baselines(
			&m,
			&heard(&m, DEVICE, |i| {
				Some(if i < 60 {
					1_000
				} else {
					100_000
				})
			}),
			&[],
		);
		let hours = b[0].hours;
		assert!(close(hours[0].median, 1_001f64.ln()) && hours[0].n == 60, "60 minutes is enough");
		assert_eq!(hours[1].n, 59);
		assert!(close(hours[1].median, 1_001f64.ln()), "59 is not: the median of all 119");
		assert_eq!(
			hours[2],
			Baseline {
				n: 0,
				..hours[1]
			}
		);
		assert_eq!(
			b[0].all,
			Baseline {
				n: 119,
				..hours[1]
			}
		);
	}

	#[test]
	fn port_scan_minutes_are_left_out() {
		let m = minutes(2 * 1440);
		let busy_from = START + 60 * 600;
		// longer than a real one, so that it would move the median
		let busy_to = busy_from + 70 * 60;
		let without = baselines(&m, &heard(&m, DEVICE, |_| Some(1_000)), &[]);
		let during = |i: usize| {
			let t = START + 60 * i as i64;
			Some(if (busy_from..=busy_to).contains(&t) {
				1_000_000
			} else {
				1_000
			})
		};
		let with = baselines(&m, &heard(&m, DEVICE, during), &[(busy_from, busy_to)]);
		for (w, o) in with[0].hours.iter().zip(without[0].hours) {
			assert_eq!((w.median, w.mad), (o.median, o.mad));
		}
		assert_eq!(
			(with[0].hours[10].n, with[0].hours[11].n),
			(60, 109),
			"its 71 minutes are gone"
		);
		let not_left_out = baselines(&m, &heard(&m, DEVICE, during), &[]);
		assert_ne!(not_left_out[0].hours[10].median, without[0].hours[10].median);
	}

	/// What the page computes, as `ScoreScale` describes it.
	fn score(bytes: f64, median: f64, mad: f64) -> f64 {
		let s = ScoreScale::default();
		(s.k * (bytes.ln_1p() - median) / mad.max(s.floor)).max(0.0)
	}

	#[test]
	fn the_floor_makes_ten_times_usual_red() {
		let s = ScoreScale::default();
		let usual = 20_000f64;
		let median = usual.ln_1p();
		assert_eq!(score(usual, median, 0.0), 0.0);
		assert_eq!(score(usual / 4.0, median, 0.0), 0.0, "quieter than usual isn't unusual");
		assert!(score(2.0 * usual, median, 0.0) < s.white, "twice usual stays white");
		assert!((score(10.0 * usual, median, 0.0) - s.red).abs() < 1e-3, "ten times is red");
		assert!(score(10.0 * usual, median, 1.0) < s.red, "a spread above the floor counts");
	}

	#[test]
	fn under_a_day_of_minutes_is_learning() {
		// stored rows with `n` minutes in all
		let rows = |mac: &'static str, n: i64| {
			(0..=24).map(move |h| {
				(
					mac.to_string(),
					h,
					1.0,
					0.5,
					if h == 24 {
						n
					} else {
						n / 24
					},
				)
			})
		};
		let mut all: Vec<_> = rows(DEVICE, 1439).chain(rows(OTHER, 1440)).collect();
		all.extend(rows("c6:61:9e:67:da:88", 5000).filter(|r| r.1 != 24));
		let usual = usual_traffic(&all);
		assert!(usual[DEVICE].learning);
		assert!(!usual[OTHER].learning);
		assert_eq!((usual[OTHER].hours.len(), usual[OTHER].all), (24, [1.0, 0.5]));
		assert!(!usual.contains_key("c6:61:9e:67:da:88"), "no all-hours row: left out");
	}
}
