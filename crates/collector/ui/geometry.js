// The floor plan's geometry: plain functions on points, no page, so they can
// be checked on their own (docs/floorplan.md).

// Plan units round the plan.
export const MARGIN = 40;

const EPS = 1e-6;
export const corner = ([x, y]) => `${x},${y}`;

// The plan from metres into plan units: its bounds fitted into 1000 by 600,
// the same scale both ways, with the margin round them; w and h are its frame.
export function toUnits(plan) {
	const xs = plan.rooms.flatMap((r) => r.points.map((p) => p[0]));
	const ys = plan.rooms.flatMap((r) => r.points.map((p) => p[1]));
	const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
	const s = Math.min((1000 - 2 * MARGIN) / (x1 - x0), (600 - 2 * MARGIN) / (y1 - y0));
	const u = ([x, y]) => [MARGIN + (x - x0) * s, MARGIN + (y - y0) * s];
	return {
		w: (x1 - x0) * s + 2 * MARGIN,
		h: (y1 - y0) * s + 2 * MARGIN,
		rooms: plan.rooms.map((r) => ({ name: r.name, points: r.points.map(u) })),
		openings: (plan.openings ?? []).map((o) => ({ from: u(o.from), to: u(o.to), at: o.at, width: o.width * s })),
	};
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

// Whether p is in a doorway: on its edge's line, within its width.
export function inOpening(p, openings) {
	return openings.some(({ from, to, at, width }) => {
		const { t, off, len } = along(p, from, to);
		return off < EPS && Math.abs(t - at * len) < width / 2;
	});
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
