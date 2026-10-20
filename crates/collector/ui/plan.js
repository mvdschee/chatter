import { DEVICES } from "./data.js";
import { corner, inOpening, inside, labelAt, middleOf, toUnits, wallPieces } from "./geometry.js";
import { rand } from "./random.js";
import { resize } from "./view.js";

// The floor plan: the one saved through /v1/plan, else this example. It's
// made up, not anyone's house. Rooms are polygons in metres, origin top left,
// y down; openings are doorways on an edge (docs/floorplan.md). Turned into
// plan units, walls, floor and spots here; drawn in draw.js.

const EXAMPLE_PLAN = {
	rooms: [
		{ id: "living", name: "Living room", points: [[0, 0], [4.5, 0], [4.5, 3.6], [0, 3.6]] },
		{ id: "kitchen", name: "Kitchen", points: [[4.5, 0], [8.2, 0], [8.2, 3.6], [4.5, 3.6]] },
		{ id: "office", name: "Office", points: [[8.2, 0], [11.5, 0], [11.5, 3.6], [8.2, 3.6]] },
		{ id: "bedroom", name: "Bedroom", points: [[0, 3.6], [3.3, 3.6], [3.3, 6.5], [0, 6.5]] },
		{ id: "hall", name: "Hall", points: [[3.3, 3.6], [6, 3.6], [6, 6.5], [3.3, 6.5]] },
		{ id: "bathroom", name: "Bathroom", points: [[6, 3.6], [9.5, 3.6], [9.5, 6.5], [6, 6.5]] },
		{ id: "utility", name: "Utility", points: [[9.5, 3.6], [11.5, 3.6], [11.5, 6.5], [9.5, 6.5]] },
	],
	openings: [
		{ from: [4.5, 0], to: [4.5, 3.6], at: 0.6, width: 0.8 },
		{ from: [8.2, 0], to: [8.2, 3.6], at: 0.4, width: 0.8 },
		{ from: [0, 3.6], to: [3.3, 3.6], at: 0.5, width: 0.8 },
		{ from: [3.3, 3.6], to: [6, 3.6], at: 0.75, width: 0.8 },
		{ from: [3.3, 3.6], to: [3.3, 6.5], at: 0.5, width: 0.8 },
		{ from: [6, 3.6], to: [6, 6.5], at: 0.3, width: 0.8 },
		{ from: [9.5, 3.6], to: [9.5, 6.5], at: 0.5, width: 0.8 },
		{ from: [6, 6.5], to: [3.3, 6.5], at: 0.5, width: 1 },
	],
};

// Plan units; the view projects them into 3D. A plan in metres is scaled to
// fit 1000 by 600 inside the margin, so the look below holds for a flat or a
// house; PLAN then takes the plan's own shape (setPlan).
export const PLAN = { w: 1000, h: 600 };
export const WALL_H = 56;

// Where the router and the sensor stand until they can be placed by hand,
// when that's inside a room (plan units).
const ROUTER_AT = [410, 470];
const SENSOR_AT = [455, 520];
export const ROUTER = { x: ROUTER_AT[0], y: ROUTER_AT[1], name: "Router" };
export const SENSOR = { x: SENSOR_AT[0], y: SENSOR_AT[1], name: "Sensor" };

// Until devices can be placed by hand, each gets a spot on a grid over the
// plan's rooms, the same one on every load: spots shuffled once, handed out in
// MAC order. Set by setPlan.
export let SPOTS = [];

// Set by setPlan: the rooms (points in plan units, where the name goes), the
// walls as point-cloud curtains (dots along each wall, stacked to wall
// height), and the floor (a dot grid with a little jitter, so it reads as
// scanned, not drawn).
export let rooms = [];
export let wallDots = [];
export let floorDots = [];

// The plan as saved, in metres, or null before the first save (edit.js).
export let saved = null;

// Draws this plan from now on: a saved one or the example.
function setPlan(plan) {
	const p = toUnits(plan);
	Object.assign(PLAN, { w: p.w, h: p.h });
	const inPlan = (q) => p.rooms.some((r) => inside(q, r.points));
	rooms = p.rooms.map((r) => ({ ...r, label: labelAt(r.points) }));

	wallDots = [];
	const pieces = wallPieces(p.rooms);
	const corners = new Map(pieces.flat().map((c) => [corner(c), c]));
	const between = pieces.flatMap(([[x1, y1], [x2, y2]]) => {
		const n = Math.max(1, Math.round(Math.hypot(x2 - x1, y2 - y1) / 6));
		return Array.from({ length: n - 1 }, (_, k) => [x1 + ((x2 - x1) * (k + 1)) / n, y1 + ((y2 - y1) * (k + 1)) / n]);
	});
	for (const [x, y] of [...corners.values(), ...between]) {
		if (inOpening([x, y], p.openings)) continue;
		for (let z = 0; z <= WALL_H; z += 7) {
			wallDots.push({ x, y, z, top: z + 7 > WALL_H, phase: rand() * Math.PI * 2 });
		}
	}

	floorDots = [];
	for (let x = 50; x < PLAN.w - 40; x += 16) {
		for (let y = 50; y < PLAN.h - 40; y += 16) {
			const q = [x + (rand() - 0.5) * 4, y + (rand() - 0.5) * 4];
			if (inPlan(q)) floorDots.push({ x: q[0], y: q[1], a: 0.25 + rand() * 0.5 });
		}
	}

	// the router and the sensor where they stood, if that's in a room
	const [rx, ry] = inPlan(ROUTER_AT) ? ROUTER_AT : middleOf(p.rooms[0].points);
	const near = [rx + 45, ry + 50];
	const [sx, sy] = inPlan(SENSOR_AT) ? SENSOR_AT : inPlan(near) ? near : [rx, ry];
	Object.assign(ROUTER, { x: rx, y: ry });
	Object.assign(SENSOR, { x: sx, y: sy });

	SPOTS = [];
	for (let x = 100; x <= PLAN.w - 100; x += 114) {
		for (let y = 95; y <= PLAN.h - 90; y += 83) {
			if (inPlan([x, y]) && Math.hypot(x - rx, y - ry) > 70 && Math.hypot(x - sx, y - sy) > 50) SPOTS.push([x, y]);
		}
	}
	let seed = 11;
	for (let i = SPOTS.length - 1; i > 0; i--) {
		seed = (seed * 1103515245 + 12345) & 0x7fffffff;
		const j = seed % (i + 1);
		[SPOTS[i], SPOTS[j]] = [SPOTS[j], SPOTS[i]];
	}
	DEVICES.forEach((d, i) => ([d.x, d.y] = SPOTS[i % SPOTS.length] ?? [rx, ry]));
	resize();
}

// The saved plan, or the example until one with rooms is saved.
export async function loadPlan() {
	let plan = null;
	try {
		const res = await fetch("/v1/plan");
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		plan = await res.json();
	} catch (e) {
		console.error("plan:", e);
	}
	saved = plan;
	setPlan(plan?.rooms.length ? plan : EXAMPLE_PLAN);
}
