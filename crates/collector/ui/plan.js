import { DEVICES, SENSORS } from "./data.js";
import { MARGIN, corner, cutsRow, frontAreas, inside, labelAt, middleOf, openingAt, roomAt, toUnits, wallPieces } from "./geometry.js";
import { rand } from "./random.js";
import { resize } from "./view.js";

// The floor plan: the one saved through /v1/plan, else this example. It's
// made up, not anyone's house. Rooms are polygons in metres, origin top left,
// y down; openings are doorways and windows on an edge (docs/floorplan.md). Turned into
// plan units, walls, floor and spots here; drawn in draw.js.

// How far off an opening's line, in metres, a wall's dots still go for it:
// rooms traced side by side give their shared wall twice, a few mm apart, and
// a doorway has to cut both. A wall is about ten times this thick.
const DOOR_REACH = 0.01;

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
		{ from: [0, 0], to: [4.5, 0], at: 0.5, width: 2.4, kind: "window" },
		{ from: [4.5, 0], to: [8.2, 0], at: 0.5, width: 1.4, kind: "half" },
		{ from: [8.2, 0], to: [11.5, 0], at: 0.5, width: 1.2, kind: "half" },
		{ from: [0, 3.6], to: [0, 6.5], at: 0.5, width: 1.2, kind: "half" },
		{ from: [11.5, 3.6], to: [11.5, 6.5], at: 0.5, width: 0.8, kind: "half" },
	],
};

// Plan units; the view projects them into 3D. A plan in metres is scaled to
// fit 1000 by 600 inside the margin, so the look below holds for a flat or a
// house; PLAN then takes the plan's own shape (setPlan).
export const PLAN = { w: 1000, h: 600 };
export const WALL_H = 56;
// How far apart a wall's rows of dots are, plan units.
const ROW = 7;

// Where the router stands: its place, else the middle of the first room
// (the traffic's arcs need it somewhere). Set by arrange.
export const ROUTER = { x: 0, y: 0, name: "Router" };

// Where each sensor stands, by name: its place, else a slot with the devices
// waiting. Set by arrange.
export const sensorAt = new Map();

// The areas in front of the house: the things still waiting to be placed,
// then one per person with the devices they carry (frontAreas). Set by
// arrange.
export let areas = [];

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
		const opening = openingAt([x, y], p.openings, DOOR_REACH * p.perM);
		for (let z = 0; z <= WALL_H; z += ROW) {
			if (opening && cutsRow(opening.kind, z, WALL_H, ROW)) continue;
			// the wall fades as it rises: the row a window leaves under the
			// top is drawn as clear as the bottom row, or it can't be seen
			const lintel = Boolean(opening) && z + ROW <= WALL_H && z + 2 * ROW > WALL_H;
			wallDots.push({ x, y, z, top: z + ROW > WALL_H, lintel, phase: rand() * Math.PI * 2 });
		}
	}

	floorDots = [];
	for (let x = 50; x < p.w - 40; x += 16) {
		for (let y = 50; y < p.h - 40; y += 16) {
			const q = [x + (rand() - 0.5) * 4, y + (rand() - 0.5) * 4];
			if (inPlan(q)) floorDots.push({ x: q[0], y: q[1], a: 0.25 + rand() * 0.5 });
		}
	}

	[house, places] = [{ w: p.w, h: p.h }, p.places];
	arrange();
}

// The house's frame and the places in plan units, from setPlan.
let house = null, places = null;

// Puts the router, the sensors and the devices where they go: placed ones
// where they stand, carried ones in their person's area, the rest waiting.
// Called when the plan or the devices change. The frame grows to take in the
// areas, and the view refits when it does.
export function arrange() {
	if (!house) return;
	const at = (p) => ({ x: p[0], y: p[1] });
	Object.assign(ROUTER, at(places?.router ?? middleOf(rooms[0].points)));

	const waiting = [], carried = new Map();
	for (const d of DEVICES) {
		const place = places?.devices?.[d.mac];
		if (place?.at) Object.assign(d, at(place.at), { room: roomAt(rooms, place.at) ?? "placed outside the rooms", waiting: false });
		else if (place?.carried_by) carried.set(place.carried_by, [...(carried.get(place.carried_by) ?? []), d]);
		else waiting.push(d);
	}
	// waiting sensors last, so their labels don't cover the area's name
	sensorAt.clear();
	for (const s of SENSORS) {
		const p = places?.sensors?.[s.name];
		if (p) sensorAt.set(s.name, at(p));
		else waiting.push({ sensor: s.name });
	}
	const groups = [
		...(waiting.length ? [{ name: "Waiting to be placed", room: "waiting to be placed", compact: true, things: waiting }] : []),
		...[...carried.keys()].sort().map((name) => ({ name, room: `carried by ${name}`, things: carried.get(name) })),
	];
	areas = frontAreas(groups.map((g) => ({ name: g.name, compact: g.compact, count: g.things.length })),
		{ left: MARGIN, top: house.h - MARGIN + 50, width: house.w - 2 * MARGIN });
	groups.forEach((g, i) => g.things.forEach((t, k) => {
		const spot = at(areas[i].slots[k]);
		if (t.sensor) sensorAt.set(t.sensor, spot);
		else Object.assign(t, spot, { room: g.room, waiting: Boolean(g.compact) });
	}));

	const w = Math.max(house.w, ...areas.map((a) => a.x + a.w + MARGIN));
	const h = Math.max(house.h, ...areas.map((a) => a.y + a.h + MARGIN));
	if (w !== PLAN.w || h !== PLAN.h) {
		Object.assign(PLAN, { w, h });
		resize();
	}
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
