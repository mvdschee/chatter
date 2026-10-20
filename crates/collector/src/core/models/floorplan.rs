//! The floor plan: rooms as polygons in metres, the openings (doorways) on
//! their edges. Walls are the rooms' edges. In as `FloorPlanPayload`, the
//! JSON the page saves; checked into a `FloorPlan`, which is also how it's
//! stored and sent back. The underlay, an image to trace over, is a file of
//! its own; the plan keeps where it lies. The places of the router, the
//! sensors and the devices are in the plan too, so they move with it. See
//! `docs/floorplan.md`.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A corner, `[x, y]` in metres from the plan's top left, y down.
pub type Point = [f64; 2];

/// More corners than any room needs; keeps the crossing check small.
pub const MAX_CORNERS: usize = 100;
pub const MAX_ROOMS: usize = 100;
/// More devices than a home has, sensors included.
pub const MAX_PLACES: usize = 1000;
const MAX_ID: usize = 32;
const MAX_NAME: usize = 64;
/// A floor plan scan or a phone photo of one fits easily.
pub const MAX_UNDERLAY_BYTES: usize = 20 * 1024 * 1024;

/// Cross products this close to 0 (in m²) count as collinear: corners on a
/// 10 cm grid don't multiply out exactly in floats.
const EPS: f64 = 1e-9;

#[derive(Debug, Deserialize)]
pub struct FloorPlanPayload {
	pub rooms: Vec<Room>,
	#[serde(default)]
	pub openings: Vec<Opening>,
	#[serde(default)]
	pub underlay: Option<Underlay>,
	#[serde(default)]
	pub places: Places,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorPlan {
	pub rooms: Vec<Room>,
	pub openings: Vec<Opening>,
	/// Plans saved before underlays had none.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub underlay: Option<Underlay>,
	/// Plans saved before places had none.
	#[serde(default)]
	pub places: Places,
}

/// Where things stand on the plan, in metres. Kept for devices not seen
/// lately too, so one that's off for a while keeps its spot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Places {
	#[serde(default)]
	pub router: Option<Point>,
	/// By the sensor's name.
	#[serde(default)]
	pub sensors: BTreeMap<String, Point>,
	/// By MAC, lowercase with colons, as the reports have it.
	#[serde(default)]
	pub devices: BTreeMap<String, DevicePlace>,
}

/// A device stands somewhere, or moves around with someone: one of the two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevicePlace {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub at: Option<Point>,
	/// Who it moves around with (a laptop, a phone).
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub carried_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
	pub id: String,
	pub name: String,
	/// In order round the room; the last corner joins the first.
	pub points: Vec<Point>,
}

/// An opening in the edge from `from` to `to`, two neighbouring corners of a
/// room, in either order: named by its corners, so it's stored once for the
/// two rooms that share the edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opening {
	pub from: Point,
	pub to: Point,
	/// Where its middle is, from `from`, as a share of the edge's length.
	pub at: f64,
	/// In metres.
	pub width: f64,
}

/// Where the underlay image lies on the plan: its pixel (u, v) is at
/// `offset + turn(angle) * (u, v) / pixels_per_m`, in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
	/// Image pixels per metre, from two points and the distance between them.
	pub pixels_per_m: f64,
	/// Where the image's top left corner is.
	pub offset: Point,
	/// In radians, clockwise on screen (y is down): a scan that came out a
	/// little crooked, turned square.
	pub angle: f64,
}

/// The content type of an underlay image, from its first bytes: PNG, JPEG or
/// WebP. Nothing else, SVG above all: it can carry script, and the route
/// that stores it has no login.
pub fn underlay_type(bytes: &[u8]) -> Result<&'static str> {
	match bytes {
		[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..] => Ok("image/png"),
		[0xff, 0xd8, 0xff, ..] => Ok("image/jpeg"),
		[b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Ok("image/webp"),
		_ => Err(Error::Plan("an underlay is a PNG, JPEG or WebP image".into())),
	}
}

impl TryFrom<FloorPlanPayload> for FloorPlan {
	type Error = Error;

	fn try_from(payload: FloorPlanPayload) -> Result<Self> {
		if payload.rooms.len() > MAX_ROOMS {
			return Err(Error::Plan(format!("more than {MAX_ROOMS} rooms")));
		}
		let mut ids = HashSet::new();
		for room in &payload.rooms {
			check_room(room)?;
			if !ids.insert(room.id.as_str()) {
				return Err(Error::Plan(format!("two rooms with id {:?}", room.id)));
			}
		}
		for opening in &payload.openings {
			check_opening(opening, &payload.rooms)?;
		}
		if let Some(underlay) = &payload.underlay {
			check_underlay(underlay)?;
		}
		check_places(&payload.places)?;
		Ok(Self {
			rooms: payload.rooms,
			openings: payload.openings,
			underlay: payload.underlay,
			places: payload.places,
		})
	}
}

fn check_room(room: &Room) -> Result<()> {
	let bad = |why: &str| Err(Error::Plan(format!("room {:?}: {why}", room.name)));
	if room.id.is_empty() || room.id.len() > MAX_ID {
		return bad(&format!("an id is 1 to {MAX_ID} bytes"));
	}
	if room.name.trim().is_empty() || room.name.chars().count() > MAX_NAME {
		return bad(&format!("a name is 1 to {MAX_NAME} characters"));
	}
	let p = &room.points;
	if !(3..=MAX_CORNERS).contains(&p.len()) {
		return bad(&format!("a room has 3 to {MAX_CORNERS} corners"));
	}
	if p.iter().any(|[x, y]| !x.is_finite() || !y.is_finite()) {
		return bad("a corner isn't a number");
	}
	for i in 0..p.len() {
		if p[i + 1..].contains(&p[i]) {
			return bad("the same corner twice");
		}
	}
	if crosses_itself(p) {
		return bad("its edges cross");
	}
	Ok(())
}

fn check_opening(opening: &Opening, rooms: &[Room]) -> Result<()> {
	let Opening {
		from,
		to,
		at,
		width,
	} = *opening;
	let bad = |why: &str| Err(Error::Plan(format!("opening {from:?} to {to:?}: {why}")));
	if !rooms
		.iter()
		.any(|r| edges(&r.points).any(|(a, b)| (a, b) == (from, to) || (a, b) == (to, from)))
	{
		return bad("not an edge of a room");
	}
	let len = (to[0] - from[0]).hypot(to[1] - from[1]);
	if !(width > 0.0 && at * len - width / 2.0 >= -EPS && at * len + width / 2.0 <= len + EPS) {
		return bad("doesn't fit on its edge");
	}
	Ok(())
}

fn check_underlay(u: &Underlay) -> Result<()> {
	let finite = u.offset.iter().all(|v| v.is_finite()) && u.angle.is_finite();
	if !(u.pixels_per_m.is_finite() && u.pixels_per_m > 0.0 && finite) {
		return Err(Error::Plan(
			"underlay: its scale, offset and angle are numbers, the scale above 0".into(),
		));
	}
	if u.angle.abs() > std::f64::consts::PI {
		return Err(Error::Plan("underlay: an angle is between -π and π".into()));
	}
	Ok(())
}

fn check_places(places: &Places) -> Result<()> {
	let bad = |why: String| Err(Error::Plan(format!("places: {why}")));
	if places.sensors.len() + places.devices.len() > MAX_PLACES {
		return bad(format!("more than {MAX_PLACES}"));
	}
	let finite = |p: &Point| p.iter().all(|v| v.is_finite());
	if places.router.as_ref().is_some_and(|p| !finite(p)) {
		return bad("the router's place isn't a number".into());
	}
	for (name, at) in &places.sensors {
		if name.trim().is_empty() || name.chars().count() > MAX_NAME {
			return bad(format!("a sensor's name is 1 to {MAX_NAME} characters"));
		}
		if !finite(at) {
			return bad(format!("sensor {name:?}: its place isn't a number"));
		}
	}
	for (mac, place) in &places.devices {
		if !is_mac(mac) {
			return bad(format!("{mac:?} isn't a MAC like 0a:1b:2c:3d:4e:5f"));
		}
		match (&place.at, &place.carried_by) {
			(Some(at), None) if finite(at) => {},
			(None, Some(who)) if !who.trim().is_empty() && who.chars().count() <= MAX_NAME => {},
			(Some(_), None) => return bad(format!("{mac}: its place isn't a number")),
			(None, Some(_)) => {
				return bad(format!("{mac}: who carries it is 1 to {MAX_NAME} characters"));
			},
			_ => return bad(format!("{mac}: either a place or who carries it, one of the two")),
		}
	}
	Ok(())
}

/// Six lowercase hex pairs with colons, the way the reports write a MAC.
pub fn is_mac(s: &str) -> bool {
	s.len() == 17
		&& s.split(':').all(|pair| {
			pair.len() == 2
				&& pair.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
		})
}

/// The polygon's edges, the last corner back to the first included.
fn edges(p: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
	(0..p.len()).map(|i| (p[i], p[(i + 1) % p.len()]))
}

/// Whether a polygon with distinct corners isn't simple: two edges that
/// aren't neighbours meet anywhere, ends included, or two neighbours fold
/// back over each other.
fn crosses_itself(p: &[Point]) -> bool {
	let n = p.len();
	let e: Vec<_> = edges(p).collect();
	for i in 0..n {
		// neighbours share a corner; they only overlap when the second turns
		// straight back along the first
		let (a, b) = e[i];
		let c = e[(i + 1) % n].1;
		if cross(a, b, c).abs() <= EPS && dot(sub(b, a), sub(c, b)) < 0.0 {
			return true;
		}
		// with 3 edges, every pair are neighbours
		for j in i + 2..n {
			if i == 0 && j == n - 1 {
				continue;
			}
			if segments_meet(e[i], e[j]) {
				return true;
			}
		}
	}
	false
}

fn sub(a: Point, b: Point) -> Point {
	[a[0] - b[0], a[1] - b[1]]
}

fn dot(a: Point, b: Point) -> f64 {
	a[0] * b[0] + a[1] * b[1]
}

/// Which side of `a`→`b` the point `c` is on: > 0 one side, < 0 the other.
fn cross(a: Point, b: Point, c: Point) -> f64 {
	let (u, v) = (sub(b, a), sub(c, a));
	u[0] * v[1] - u[1] * v[0]
}

/// Whether `c`, collinear with `a`→`b`, lies within the segment's box.
fn within(a: Point, b: Point, c: Point) -> bool {
	c[0] >= a[0].min(b[0]) - EPS
		&& c[0] <= a[0].max(b[0]) + EPS
		&& c[1] >= a[1].min(b[1]) - EPS
		&& c[1] <= a[1].max(b[1]) + EPS
}

/// Whether two segments have any point in common, ends included.
fn segments_meet((a, b): (Point, Point), (c, d): (Point, Point)) -> bool {
	let (d1, d2) = (cross(c, d, a), cross(c, d, b));
	let (d3, d4) = (cross(a, b, c), cross(a, b, d));
	if ((d1 > EPS && d2 < -EPS) || (d1 < -EPS && d2 > EPS))
		&& ((d3 > EPS && d4 < -EPS) || (d3 < -EPS && d4 > EPS))
	{
		return true;
	}
	(d1.abs() <= EPS && within(c, d, a))
		|| (d2.abs() <= EPS && within(c, d, b))
		|| (d3.abs() <= EPS && within(a, b, c))
		|| (d4.abs() <= EPS && within(a, b, d))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn room(id: &str, points: &[Point]) -> Room {
		Room {
			id: id.to_string(),
			name: format!("Room {id}"),
			points: points.to_vec(),
		}
	}

	fn plan(rooms: Vec<Room>, openings: Vec<Opening>) -> Result<FloorPlan> {
		FloorPlan::try_from(FloorPlanPayload {
			rooms,
			openings,
			underlay: None,
			places: Places::default(),
		})
	}

	const SQUARE: [Point; 4] = [[0.0, 0.0], [3.7, 0.0], [3.7, 2.9], [0.0, 2.9]];
	// shares the edge (3.7, 0) to (3.7, 2.9) with SQUARE
	const NEXT_DOOR: [Point; 4] = [[3.7, 0.0], [6.3, 0.0], [6.3, 2.9], [3.7, 2.9]];

	#[test]
	fn simple_rooms_pass() {
		let l_shape = [[0.0, 0.0], [5.1, 0.0], [5.1, 1.3], [2.2, 1.3], [2.2, 4.4], [0.0, 4.4]];
		let triangle = [[0.0, 0.0], [2.0, 0.0], [1.0, 1.5]];
		assert!(
			plan(vec![room("a", &SQUARE), room("b", &l_shape), room("c", &triangle)], vec![])
				.is_ok()
		);
	}

	#[test]
	fn neighbouring_edges_at_their_corner_dont_count_as_crossing() {
		// a corner on a 10 cm grid, where the floats don't multiply out
		let grid = [[0.1, 0.1], [0.3, 0.1], [0.3, 0.7], [0.2, 0.7], [0.1, 0.7]];
		assert!(plan(vec![room("a", &grid)], vec![]).is_ok());
	}

	#[test]
	fn too_few_corners_fail() {
		assert!(plan(vec![room("a", &[[0.0, 0.0], [1.0, 0.0]])], vec![]).is_err());
	}

	#[test]
	fn a_bowtie_fails() {
		let bowtie = [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]];
		assert!(plan(vec![room("a", &bowtie)], vec![]).is_err());
	}

	#[test]
	fn a_corner_on_another_edge_fails() {
		// (1, 0) touches the edge from (0, 0) to (2, 0)
		let touching = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 0.0], [0.0, 2.0]];
		assert!(plan(vec![room("a", &touching)], vec![]).is_err());
	}

	#[test]
	fn an_edge_folding_back_fails() {
		// out to (3, 0) and straight back to (2, 0)
		let spike = [[0.0, 0.0], [3.0, 0.0], [2.0, 0.0], [2.0, 2.0]];
		assert!(plan(vec![room("a", &spike)], vec![]).is_err());
		let flat = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
		assert!(plan(vec![room("a", &flat)], vec![]).is_err());
	}

	#[test]
	fn the_same_corner_twice_fails() {
		let repeated = [[0.0, 0.0], [2.0, 0.0], [2.0, 0.0], [2.0, 2.0]];
		assert!(plan(vec![room("a", &repeated)], vec![]).is_err());
	}

	#[test]
	fn rooms_need_their_own_ids_and_a_name() {
		assert!(plan(vec![room("a", &SQUARE), room("a", &NEXT_DOOR)], vec![]).is_err());
		let mut nameless = room("a", &SQUARE);
		nameless.name = "  ".to_string();
		assert!(plan(vec![nameless], vec![]).is_err());
	}

	#[test]
	fn an_opening_sits_on_an_edge_in_either_direction_and_fits() {
		let rooms = || vec![room("a", &SQUARE), room("b", &NEXT_DOOR)];
		let door = |from, to, at, width| Opening {
			from,
			to,
			at,
			width,
		};
		// the shared edge, named both ways round
		assert!(plan(rooms(), vec![door([3.7, 0.0], [3.7, 2.9], 0.5, 0.9)]).is_ok());
		assert!(plan(rooms(), vec![door([3.7, 2.9], [3.7, 0.0], 0.5, 0.9)]).is_ok());
		// a diagonal is no edge
		assert!(plan(rooms(), vec![door([0.0, 0.0], [3.7, 2.9], 0.5, 0.9)]).is_err());
		// sticks out past the corner, or has no width
		assert!(plan(rooms(), vec![door([3.7, 0.0], [3.7, 2.9], 0.1, 0.9)]).is_err());
		assert!(plan(rooms(), vec![door([3.7, 0.0], [3.7, 2.9], 0.5, 0.0)]).is_err());
	}

	#[test]
	fn the_stored_plan_reads_back_the_same() {
		let saved = plan(vec![room("a", &SQUARE)], vec![]).unwrap();
		let json = serde_json::to_string(&saved).unwrap();
		assert_eq!(serde_json::from_str::<FloorPlan>(&json).unwrap(), saved);
	}

	#[test]
	fn an_underlay_is_kept_and_checked() {
		let with = |underlay| {
			FloorPlan::try_from(FloorPlanPayload {
				rooms: vec![room("a", &SQUARE)],
				openings: vec![],
				underlay: Some(underlay),
				places: Places::default(),
			})
		};
		let good = Underlay {
			pixels_per_m: 47.3,
			offset: [-0.6, 1.2],
			angle: 0.013,
		};
		let saved = with(good.clone()).unwrap();
		let json = serde_json::to_string(&saved).unwrap();
		assert_eq!(serde_json::from_str::<FloorPlan>(&json).unwrap().underlay, Some(good.clone()));
		for bad in [
			Underlay {
				pixels_per_m: 0.0,
				..good.clone()
			},
			Underlay {
				pixels_per_m: f64::INFINITY,
				..good.clone()
			},
			Underlay {
				offset: [f64::NAN, 0.0],
				..good.clone()
			},
			Underlay {
				angle: 4.0,
				..good.clone()
			},
		] {
			assert!(with(bad).is_err());
		}
	}

	#[test]
	fn a_plan_saved_before_underlays_still_reads() {
		let json =
			r#"{"rooms":[{"id":"a","name":"A","points":[[0,0],[1,0],[1,1]]}],"openings":[]}"#;
		assert_eq!(serde_json::from_str::<FloorPlan>(json).unwrap().underlay, None);
	}

	// made up at random; the doc's example
	const STANDS: &str = "fa:42:31:98:3e:b5";
	const CARRIED: &str = "c2:d7:2c:93:34:d2";

	fn with_places(json: &str) -> Result<FloorPlan> {
		let places: Places = serde_json::from_str(json).unwrap();
		FloorPlan::try_from(FloorPlanPayload {
			rooms: vec![room("a", &SQUARE)],
			openings: vec![],
			underlay: None,
			places,
		})
	}

	#[test]
	fn places_are_kept_and_read_back() {
		let json = format!(
			r#"{{"router":[1.5,0.4],"sensors":{{"study":[3.1,2.2]}},
			"devices":{{"{STANDS}":{{"at":[0.8,2.0]}},"{CARRIED}":{{"carried_by":"Robin"}}}}}}"#
		);
		let saved = with_places(&json).unwrap();
		let back: FloorPlan =
			serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
		assert_eq!(back, saved);
		assert_eq!(back.places.devices[CARRIED].carried_by.as_deref(), Some("Robin"));
		assert_eq!(back.places.router, Some([1.5, 0.4]));
	}

	#[test]
	fn a_plan_saved_before_places_reads_with_none() {
		let json =
			r#"{"rooms":[{"id":"a","name":"A","points":[[0,0],[1,0],[1,1]]}],"openings":[]}"#;
		assert_eq!(serde_json::from_str::<FloorPlan>(json).unwrap().places, Places::default());
	}

	#[test]
	fn bad_places_fail() {
		for json in [
			// a device both placed and carried, or neither
			format!(r#"{{"devices":{{"{STANDS}":{{"at":[1,1],"carried_by":"Robin"}}}}}}"#),
			format!(r#"{{"devices":{{"{STANDS}":{{}}}}}}"#),
			// not a MAC the way the reports write them
			r#"{"devices":{"FA:42:31:98:3E:B5":{"at":[1,1]}}}"#.to_string(),
			r#"{"devices":{"fa-42-31-98-3e-b5":{"at":[1,1]}}}"#.to_string(),
			// nobody carrying it
			format!(r#"{{"devices":{{"{CARRIED}":{{"carried_by":"  "}}}}}}"#),
			r#"{"sensors":{"":[1,1]}}"#.to_string(),
		] {
			assert!(with_places(&json).is_err(), "{json}");
		}
	}

	#[test]
	fn only_png_jpeg_and_webp_are_underlays() {
		assert_eq!(underlay_type(b"\x89PNG\r\n\x1a\n rest").unwrap(), "image/png");
		assert_eq!(underlay_type(b"\xff\xd8\xff\xe0 rest").unwrap(), "image/jpeg");
		assert_eq!(underlay_type(b"RIFF\x10\0\0\0WEBPVP8 ").unwrap(), "image/webp");
		assert!(underlay_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\">").is_err());
		assert!(underlay_type(b"GIF89a").is_err());
		assert!(underlay_type(b"").is_err());
	}
}
