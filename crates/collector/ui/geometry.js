// The floor plan's geometry: plain functions on points, no page, so they can
// be checked on their own (docs/floorplan.md).

// Plan units round the plan.
export const MARGIN = 40;

const EPS = 1e-6;
export const corner = ([x, y]) => `${x},${y}`;

// The plan from metres into plan units: its bounds fitted into 1000 by 600,
// the same scale both ways, with the margin round them; w and h are its frame,
// perM the plan units to a metre.
// The bounds take in placed things too, so a router in the garden stays on it.
export function toUnits(plan) {
	const placed = [plan.places?.router, ...Object.values(plan.places?.sensors ?? {}), ...Object.values(plan.places?.devices ?? {}).map((d) => d.at)];
	const all = [...plan.rooms.flatMap((r) => r.points), ...placed.filter(Boolean)];
	const xs = all.map((p) => p[0]);
	const ys = all.map((p) => p[1]);
	const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
	const s = Math.min((1000 - 2 * MARGIN) / (x1 - x0), (600 - 2 * MARGIN) / (y1 - y0));
	const u = ([x, y]) => [MARGIN + (x - x0) * s, MARGIN + (y - y0) * s];
	return {
		perM: s,
		w: (x1 - x0) * s + 2 * MARGIN,
		h: (y1 - y0) * s + 2 * MARGIN,
		rooms: plan.rooms.map((r) => ({ name: r.name, points: r.points.map(u) })),
		openings: (plan.openings ?? []).map((o) => ({ from: u(o.from), to: u(o.to), at: o.at, width: o.width * s, kind: o.kind ?? "door" })),
		places: movePlaces(plan.places, u),
	};
}

// The name of the room p is in, or undefined.
export const roomAt = (rooms, p) => rooms.find((r) => inside(p, r.points))?.name;

// The areas on the ground in front of the house (plan units): `groups` are
// [{ count, compact, ... }] in the order they go, left to right from `left`,
// from `top` down, each area keeping the rest of its group; a slot per thing,
// row by row. A compact area's things are dots without labels, so a crowd
// stays small next to the house; it takes the width the others leave, at
// least MIN_COLS wide. The others are labelled, up to LABELLED.rows deep. The
// strip may come out wider than `width`.
const LABELLED = { w: 110, h: 40, rows: 3 }, COMPACT = { w: 30, h: 30 };
const AREA_GAP = 30, MIN_COLS = 6;

export function frontAreas(groups, { left, top, width }) {
	const labelledCols = (count) => Math.ceil(count / Math.min(count, LABELLED.rows));
	const taken = groups.filter((g) => !g.compact).reduce((s, g) => s + labelledCols(g.count) * LABELLED.w + AREA_GAP, 0);
	const free = Math.max(MIN_COLS, Math.floor((width - taken) / COMPACT.w));
	let x = left;
	return groups.map(({ count, ...rest }) => {
		const slot = rest.compact ? COMPACT : LABELLED;
		const cols = rest.compact ? Math.min(count, free) : labelledCols(count);
		const pad = rest.compact ? slot.w / 2 : 18;
		const slots = Array.from({ length: count }, (_, i) => [x + pad + (i % cols) * slot.w, top + slot.h / 2 + Math.floor(i / cols) * slot.h]);
		const area = { ...rest, x, y: top, w: cols * slot.w, h: Math.ceil(count / cols) * slot.h, slots };
		x += area.w + AREA_GAP;
		return area;
	});
}

// Where c is along a→b: its distance from a along the line, and from the line.
function along(c, a, b) {
	const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
	const [dx, dy] = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
	const [px, py] = [c[0] - a[0], c[1] - a[1]];
	return { t: px * dx + py * dy, off: Math.abs(px * dy - py * dx), len };
}

// The walls: every room edge, split at each corner that lies on it, so rooms
// that share a wall, whole or in part, give the same pieces; each piece once.
export function wallPieces(rooms) {
	const corners = rooms.flatMap((r) => r.points);
	const pieces = new Map();
	for (const { points } of rooms) {
		points.forEach((a, i) => {
			const b = points[(i + 1) % points.length];
			const stops = corners
				.map((c) => ({ c, ...along(c, a, b) }))
				.filter(({ t, off, len }) => off < EPS && t > EPS && t < len - EPS)
				.sort((p, q) => p.t - q.t)
				.map(({ c }) => c);
			[a, ...stops, b].forEach((p, k, all) => {
				const q = all[k + 1];
				if (q && corner(p) !== corner(q)) pieces.set([corner(p), corner(q)].sort().join(" "), [p, q]);
			});
		});
	}
	return [...pieces.values()];
}

// The opening p is in, or undefined: within `reach` of its edge's line,
// within its width. A reach of a few mm takes in the neighbouring room's side
// of a shared wall, traced a little off the opening's.
export function openingAt(p, openings, reach = EPS) {
	return openings.find(({ from, to, at, width }) => {
		const { t, off, len } = along(p, from, to);
		return off < reach && Math.abs(t - at * len) < width / 2;
	});
}

// Whether an opening of this kind cuts a wall's row of dots at height z, in a
// wall `h` high with rows `row` apart: a doorway cuts them all; a window
// leaves the top two rows, so it reads as a hole with wall above, not a door;
// a half window leaves the bottom half too.
export function cutsRow(kind, z, h, row) {
	if (kind === "door") return true;
	if (z + 2 * row > h) return false;
	return kind === "window" || z >= h / 2;
}

// Whether p is inside the polygon (ray casting).
export function inside([x, y], points) {
	let odd = false;
	for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
		const [xi, yi] = points[i], [xj, yj] = points[j];
		if (yi > y !== yj > y && x < xi + ((y - yi) * (xj - xi)) / (yj - yi)) odd = !odd;
	}
	return odd;
}

// The widest stretch across the polygon at height y, [left, right].
function widestAt(y, points) {
	const xs = [];
	for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
		const [xi, yi] = points[i], [xj, yj] = points[j];
		if (yi > y !== yj > y) xs.push(xi + ((y - yi) * (xj - xi)) / (yj - yi));
	}
	xs.sort((a, b) => a - b);
	let best = [xs[0], xs[1]];
	for (let k = 2; k + 1 < xs.length; k += 2) if (xs[k + 1] - xs[k] > best[1] - best[0]) best = [xs[k], xs[k + 1]];
	return best;
}

// A room's name goes inset from its top left, on the widest stretch across it
// just below its top.
export function labelAt(points) {
	const ys = points.map((p) => p[1]);
	const y = Math.min(Math.min(...ys) + 30, (Math.min(...ys) + Math.max(...ys)) / 2);
	const [l, r] = widestAt(y, points);
	return [l + Math.min(20, (r - l) / 2), y];
}

// A point in the middle of a room.
export function middleOf(points) {
	const ys = points.map((p) => p[1]);
	const y = (Math.min(...ys) + Math.max(...ys)) / 2;
	const [l, r] = widestAt(y, points);
	return [(l + r) / 2, y];
}

// --- drawing a room (edit.js), in metres ---

// The grid corners snap to, and how far a line may lean and still snap
// square to the last one (or, for the first, to the axes).
export const GRID = 0.1;
const SQUARE = (8 * Math.PI) / 180;

// Rounded to the millimetre, so a corner built from the grid or a typed
// length is the same number, and the same corner() key, every time (+ 0
// turns -0 into 0).
export const mm = (v) => Math.round(v * 1000) / 1000 + 0;

const sub = (a, b) => [a[0] - b[0], a[1] - b[1]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1];
// Which side of a→b the point c is on: > 0 one side, < 0 the other.
const cross = (a, b, c) => {
	const [u, v] = [sub(b, a), sub(c, a)];
	return u[0] * v[1] - u[1] * v[0];
};
export const dist = (a, b) => Math.hypot(b[0] - a[0], b[1] - a[1]);

// Where a pointer at p puts the next corner: an existing corner within
// `reach` metres, that same array, so shared corners stay identical; else,
// from the last corner, square to the last line (or the axes) when the line
// leans less than SQUARE from it, level with a corner in `align` within reach
// (so a room's last corner lines up with its first) or else a whole number of
// grid steps long; else the nearest grid point, each of x and y level with a
// corner's within reach (so a new room starts in line with a wall). `how`
// says which.
export function snap(p, { corners = [], align = corners, last = null, prev = null, reach = 0 }) {
	let near = null;
	for (const c of corners) if (dist(p, c) <= reach && (!near || dist(p, c) < dist(p, near))) near = c;
	if (near) return { at: near, how: "corner" };
	if (last && dist(p, last) > 0) {
		const along = prev && dist(prev, last) > 0 ? sub(last, prev).map((v) => v / dist(prev, last)) : [1, 0];
		const v = sub(p, last);
		for (const d of [along, [-along[1], along[0]]]) {
			const t = dot(v, d);
			if (Math.abs(cross([0, 0], d, v)) <= Math.hypot(...v) * Math.sin(SQUARE)) {
				let s = Math.round(t / GRID) * GRID, off = reach;
				for (const c of align) {
					const level = dot(sub(c, last), d);
					if (Math.abs(level - t) <= off) [s, off] = [level, Math.abs(level - t)];
				}
				return { at: [mm(last[0] + d[0] * s), mm(last[1] + d[1] * s)], how: "square" };
			}
		}
	}
	const level = (i) => {
		let v = mm(Math.round(p[i] / GRID) * GRID), off = reach;
		for (const c of align) if (Math.abs(c[i] - p[i]) <= off) [v, off] = [c[i], Math.abs(c[i] - p[i])];
		return v;
	};
	return { at: [level(0), level(1)], how: "grid" };
}

// The point `len` metres from `from` towards `toward`: a typed length.
export function atLength(from, toward, len) {
	const d = dist(from, toward);
	return [mm(from[0] + ((toward[0] - from[0]) * len) / d), mm(from[1] + ((toward[1] - from[1]) * len) / d)];
}

// The same rules the collector checks a room against on save
// (crates/collector/src/core/models/floorplan.rs): cross products this close
// to 0 count as collinear.
const EPS_M = 1e-9;

// Whether c, collinear with a→b, lies within the segment's box.
const within = (a, b, c) =>
	c[0] >= Math.min(a[0], b[0]) - EPS_M && c[0] <= Math.max(a[0], b[0]) + EPS_M &&
	c[1] >= Math.min(a[1], b[1]) - EPS_M && c[1] <= Math.max(a[1], b[1]) + EPS_M;

// Whether two segments have any point in common, ends included.
function meet([a, b], [c, d]) {
	const [d1, d2, d3, d4] = [cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d)];
	if (((d1 > EPS_M && d2 < -EPS_M) || (d1 < -EPS_M && d2 > EPS_M)) && ((d3 > EPS_M && d4 < -EPS_M) || (d3 < -EPS_M && d4 > EPS_M))) return true;
	return (Math.abs(d1) <= EPS_M && within(c, d, a)) || (Math.abs(d2) <= EPS_M && within(c, d, b)) ||
		(Math.abs(d3) <= EPS_M && within(a, b, c)) || (Math.abs(d4) <= EPS_M && within(a, b, d));
}

// Whether b→c turns straight back along a→b.
const foldsBack = (a, b, c) => Math.abs(cross(a, b, c)) <= EPS_M && dot(sub(b, a), sub(c, b)) < 0;

// Whether p can be the next corner of the corners drawn so far: not one of
// them, and the new line neither folds back along the last one nor meets an
// earlier one.
export function canAdd(chain, p) {
	const n = chain.length;
	if (chain.some((c) => c[0] === p[0] && c[1] === p[1])) return false;
	if (n >= 2 && foldsBack(chain[n - 2], chain[n - 1], p)) return false;
	for (let i = 0; i + 2 < n; i++) if (meet([chain[i], chain[i + 1]], [chain[n - 1], p])) return false;
	return true;
}

// Whether a closed room's edges cross or touch anywhere but neighbours at
// their shared corner, or two neighbours fold back over each other.
export function crossesItself(points) {
	const n = points.length;
	const e = points.map((a, i) => [a, points[(i + 1) % n]]);
	for (let i = 0; i < n; i++) {
		if (foldsBack(e[i][0], e[i][1], e[(i + 1) % n][1])) return true;
		for (let j = i + 2; j < n; j++) if (!(i === 0 && j === n - 1) && meet(e[i], e[j])) return true;
	}
	return false;
}

// How wide a room is, roughly: twice its area over its perimeter. For a long
// strip it's the strip's width; for a sliver, close to 0.
export function roomWidth(points) {
	let area = 0, around = 0;
	points.forEach((a, i) => {
		const b = points[(i + 1) % points.length];
		area += a[0] * b[1] - b[0] * a[1];
		around += dist(a, b);
	});
	return Math.abs(area) / around;
}

// The rectangle with corners a and c, in order round it.
export const box = (a, c) => [a, [c[0], a[1]], c, [a[0], c[1]]];

// --- moving a corner (edit.js, chunk 5) ---

// Whether an opening fits on its edge, as the collector checks it.
export function fitsEdge({ from, to, at, width }) {
	const len = dist(from, to);
	return width > 0 && at * len - width / 2 >= -EPS_M && at * len + width / 2 <= len + EPS_M;
}

// The rooms and openings with the corner `from` moved to `to`, in every room
// that has it and at the ends of every opening. Matched by value: shared
// corners are the same numbers, not always the same array. A corner that only
// lies on another room's wall leaves that room alone. { why } instead when a
// room it's in would be refused or a doorway no longer fits: two of its
// corners on one spot, walls that cross, narrower than minWidth.
export function moveCorner({ rooms, openings }, from, to, minWidth = 0) {
	const is = (p) => p[0] === from[0] && p[1] === from[1];
	const put = (p) => (is(p) ? [to[0], to[1]] : p);
	const moved = rooms.map((r) => (r.points.some(is) ? { ...r, points: r.points.map(put) } : r));
	for (const r of moved.filter((r, i) => r !== rooms[i])) {
		if (new Set(r.points.map(corner)).size < r.points.length) return { why: `${r.name}: two of its corners would meet.` };
		if (crossesItself(r.points)) return { why: `${r.name}: its walls would cross.` };
		if (roomWidth(r.points) < minWidth) return { why: `${r.name}: too narrow for a room.` };
	}
	const ends = openings.map((o) => (is(o.from) || is(o.to) ? { ...o, from: put(o.from), to: put(o.to) } : o));
	if (ends.some((o, i) => o !== openings[i] && !fitsEdge(o))) return { why: "A doorway wouldn't fit on its wall." };
	return { rooms: moved, openings: ends };
}

// --- doorways (edit.js, chunk 6) ---

// The room edge nearest p within reach, as { from, to, t }: t is how far
// along it p falls, in metres from `from`. Where edges lie on one line (a
// small room against a long one's wall), the shortest wins, so a doorway
// between the two stays on the wall they share and goes with the small room.
export function nearestEdge(rooms, p, reach) {
	let best = null;
	for (const { points } of rooms) {
		points.forEach((a, i) => {
			const b = points[(i + 1) % points.length];
			const { t, len } = along(p, a, b);
			const s = Math.max(0, Math.min(len, t));
			const d = dist(p, [a[0] + ((b[0] - a[0]) * s) / len, a[1] + ((b[1] - a[1]) * s) / len]);
			if (d > reach) return;
			if (!best || d < best.d - 1e-3 || (d < best.d + 1e-3 && len < best.len)) best = { from: a, to: b, t: s, d, len };
		});
	}
	return best && { from: best.from, to: best.to, t: best.t };
}

// A doorway (or window, by `kind`) `width` wide with its middle at t metres
// along the edge from → to, slid along so it fits and no wider than the edge.
export function doorAt({ from, to, kind = "door" }, t, width) {
	const len = dist(from, to);
	const w = mm(Math.min(width, len));
	const mid = Math.max(w / 2, Math.min(len - w / 2, t));
	return { from: [...from], to: [...to], at: mid / len, width: w, kind };
}

// A doorway's two ends, in metres from its edge's `from`.
export function doorSpan({ from, to, at, width }) {
	const mid = at * dist(from, to);
	return [mid - width / 2, mid + width / 2];
}

// The doorway with one end (0 the one nearer `from`, 1 the other) moved to
// t metres along its edge, the other end staying; its width in `step`s, at
// least one.
export function resizeDoor(o, end, t, step) {
	const len = dist(o.from, o.to);
	const fixed = doorSpan(o)[1 - end];
	const room = end ? len - fixed : fixed;
	const w = Math.min(Math.max(step, Math.round((end ? t - fixed : fixed - t) / step) * step), room);
	return { ...o, at: (fixed + (end ? w / 2 : -w / 2)) / len, width: mm(w) };
}

// Whether two doorways overlap: on one line, their stretches sharing more
// than a point.
export function doorsOverlap(a, b) {
	const ends = (o) => doorSpan(o).map((s) => {
		const k = s / dist(o.from, o.to);
		return [o.from[0] + (o.to[0] - o.from[0]) * k, o.from[1] + (o.to[1] - o.from[1]) * k];
	});
	const [a0, a1] = ends(a), [b0, b1] = ends(b);
	const off = 1e-6 * dist(a0, a1);
	if (Math.abs(cross(a0, a1, b0)) > off || Math.abs(cross(a0, a1, b1)) > off) return false;
	const u = sub(a1, a0);
	const [s0, s1] = [dot(sub(b0, a0), u), dot(sub(b1, a0), u)].sort((x, y) => x - y);
	return s1 > EPS && s0 < dot(u, u) - EPS;
}

// --- the underlay, an image to trace over (edit.js) ---

// An underlay { pixels_per_m, offset, angle } puts the image's pixel (u, v)
// at offset + turn(angle) * (u, v) / pixels_per_m, in metres (the same as
// the collector's Underlay).
export function fromImage({ pixels_per_m: k, offset, angle }, [u, v]) {
	const [c, s] = [Math.cos(angle), Math.sin(angle)];
	return [offset[0] + (c * u - s * v) / k, offset[1] + (s * u + c * v) / k];
}

export function toImage({ pixels_per_m: k, offset, angle }, [x, y]) {
	const [c, s] = [Math.cos(angle), Math.sin(angle)];
	const [dx, dy] = [x - offset[0], y - offset[1]];
	return [(c * dx + s * dy) * k, (-s * dx + c * dy) * k];
}

// How far a measured wall may lean in the image and still be turned square.
const STRAIGHTEN = (10 * Math.PI) / 180;

// The underlay after measuring: a and b, two points on the plan where the
// image shows the ends of a wall `metres` long. The image scales so they're
// that far apart, about a, which stays where it is. A wall leaning less than
// STRAIGHTEN from level or upright in the image turns square, so it lines up
// with the grid and the snapping; a steeper one leaves the angle as it was.
export function measure(underlay, a, b, metres) {
	const [ua, ub] = [toImage(underlay, a), toImage(underlay, b)];
	const pixels = dist(ua, ub);
	const lean = Math.atan2(ub[1] - ua[1], ub[0] - ua[0]);
	const square = Math.round(lean / (Math.PI / 2)) * (Math.PI / 2);
	const angle = Math.abs(lean - square) <= STRAIGHTEN ? square - lean : underlay.angle;
	const turned = { pixels_per_m: pixels / metres, offset: [0, 0], angle };
	const at = fromImage(turned, ua);
	return { ...turned, offset: [a[0] - at[0], a[1] - at[1]] };
}

// The places (router, sensors, devices standing somewhere) with each point
// moved by f; who carries what stays as it is.
export function movePlaces(places, f) {
	if (!places) return places;
	const devices = Object.fromEntries(Object.entries(places.devices ?? {}).map(([mac, d]) => [mac, d.at ? { ...d, at: f(d.at) } : d]));
	const sensors = Object.fromEntries(Object.entries(places.sensors ?? {}).map(([name, at]) => [name, f(at)]));
	return { ...places, router: places.router && f(places.router), sensors, devices };
}

// The plan a quarter turn round the origin, clockwise on screen (y is down)
// for dir 1, the other way for -1: rooms, openings, places and underlay
// together, so what's traced stays on the image. Corners stay exact (a swap
// and a sign), so shared ones stay the same numbers.
export function turnPlan({ rooms, openings, underlay, places }, dir) {
	const quarter = ([x, y]) => (dir > 0 ? [-y, x] : [y, -x]);
	const turn = (p) => quarter(p).map(mm);
	let turned = null;
	if (underlay) {
		let angle = underlay.angle + (dir * Math.PI) / 2;
		if (angle > Math.PI) angle -= 2 * Math.PI;
		if (angle < -Math.PI) angle += 2 * Math.PI;
		turned = { ...underlay, offset: quarter(underlay.offset), angle };
	}
	return {
		rooms: rooms.map((r) => ({ ...r, points: r.points.map(turn) })),
		openings: openings.map((o) => ({ ...o, from: turn(o.from), to: turn(o.to) })),
		underlay: turned,
		places: movePlaces(places, turn),
	};
}

// Rooms, openings and places traced over the underlay `from`, moved to where the
// same image pixels are under `to`: measuring again carries what's traced
// along. Corners are rounded to the millimetre again, the same way for every
// room, so shared ones stay shared.
export function followImage({ rooms, openings, places }, from, to) {
	const move = (p) => fromImage(to, toImage(from, p)).map(mm);
	const ratio = from.pixels_per_m / to.pixels_per_m;
	return {
		rooms: rooms.map((r) => ({ ...r, points: r.points.map(move) })),
		openings: openings.map((o) => ({ ...o, from: move(o.from), to: move(o.to), width: o.width * ratio })),
		places: movePlaces(places, move),
	};
}

// --- stacks: things close together on screen (docs/floorplan.md, chunk 8) ---

// Which things stack: `boxes` are their [x0, y0, x1, y1] on screen, dot and
// name. `mark(group)` is the box of what's drawn for a stack (its number
// and name, at its first thing's spot); the other things in it are only
// spots, so only what's drawn counts: a thing alone by its box, a stack by
// its mark. In order, each thing stands alone unless its box overlaps what
// is drawn already; then it joins that, and a stack whose mark now
// overlaps more takes that in too. So a row of things just apart doesn't
// chain into one stack. Groups of indexes, each in order, in the order of
// their first.
export function stacks(boxes, mark = (g) => boxes[g[0]]) {
	const over = (a, b) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
	let drawn = [];
	boxes.forEach((b, i) => {
		let s = { group: [i], box: b };
		for (;;) {
			const hit = drawn.filter((d) => over(d.box, s.box));
			if (!hit.length) break;
			drawn = drawn.filter((d) => !hit.includes(d));
			const group = [...hit.flatMap((d) => d.group), ...s.group].sort((x, y) => x - y);
			s = { group, box: mark(group) };
		}
		drawn.push(s);
	});
	return drawn.map((d) => d.group).sort((a, b) => a[0] - b[0]);
}

// Where a fanned stack's rows go: a column `gap` right of its rightmost
// spot, `row` apart, centred on their middle height; names `nameW` wide
// that would run past `w` put it left of the leftmost spot instead (`left`:
// names go left of their dots), and it shifts to stay within `top` to `h`.
export function fanColumn(spots, { gap = 40, row = 18, nameW = 0, w = Infinity, top = 0, h = Infinity } = {}) {
	const xs = spots.map((p) => p[0]);
	const left = Math.max(...xs) + gap + nameW > w && Math.min(...xs) - gap - nameW >= 0;
	const x = left ? Math.min(...xs) - gap : Math.max(...xs) + gap;
	const half = ((spots.length - 1) / 2) * row;
	const mid = Math.min(Math.max(spots.reduce((s, p) => s + p[1], 0) / spots.length, top + half + row), h - half - row);
	return { left, rows: spots.map((_, i) => [x, mid - half + i * row]) };
}

// How big the 3D plan is drawn and where: the largest scale at which its
// box, `bw` by `bh` at scale 1, stays `pad` inside the canvas, `w` by `h`,
// and `gap` clear of each of `clear` ({ x, y, w, h }, the controls over
// it). Centred across; of the heights tried, the one that lets it be
// largest, the nearest the middle on a tie. { fit, x, y }: the scale and
// the box's centre.
export function fitFrame(bw, bh, { w, h, clear = [], pad = 16, gap = 12 }) {
	const x = w / 2, hw = bw / 2, hh = bh / 2;
	const fitAt = (y) => {
		let f = Math.min((x - pad) / hw, (w - pad - x) / hw, (y - pad) / hh, (h - pad - y) / hh);
		// clear of a box once apart from it across or down
		for (const r of clear) {
			const [x0, x1, y0, y1] = [r.x - gap, r.x + r.w + gap, r.y - gap, r.y + r.h + gap];
			const fx = x0 >= x ? (x0 - x) / hw : x1 <= x ? (x - x1) / hw : 0;
			const fy = y0 >= y ? (y0 - y) / hh : y1 <= y ? (y - y1) / hh : 0;
			f = Math.min(f, Math.max(fx, fy));
		}
		return Math.max(0, f);
	};
	let best = { fit: fitAt(h / 2), x, y: h / 2 };
	for (let k = 1; k <= 20; k++) {
		for (const y of [h / 2 - (k * h) / 40, h / 2 + (k * h) / 40]) {
			const fit = fitAt(y);
			if (fit > best.fit + 1e-9) best = { fit, x, y };
		}
	}
	return best;
}
