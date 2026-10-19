//! The floor plan: rooms as polygons in metres, the openings (doorways) on
//! their edges. Walls are the rooms' edges. In as `FloorPlanPayload`, the
//! JSON the page saves; checked into a `FloorPlan`, which is also how it's
//! stored and sent back. See `docs/floorplan.md`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A corner, `[x, y]` in metres from the plan's top left, y down.
pub type Point = [f64; 2];

/// More corners than any room needs; keeps the crossing check small.
pub const MAX_CORNERS: usize = 100;
pub const MAX_ROOMS: usize = 100;
const MAX_ID: usize = 32;
const MAX_NAME: usize = 64;

/// Cross products this close to 0 (in m²) count as collinear: corners on a
/// 10 cm grid don't multiply out exactly in floats.
const EPS: f64 = 1e-9;

#[derive(Debug, Deserialize)]
pub struct FloorPlanPayload {
	pub rooms: Vec<Room>,
	#[serde(default)]
	pub openings: Vec<Opening>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorPlan {
	pub rooms: Vec<Room>,
	pub openings: Vec<Opening>,
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
		Ok(Self {
			rooms: payload.rooms,
			openings: payload.openings,
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
}
