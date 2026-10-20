// The floor plan's geometry (ui/geometry.js), checked in node:
//   node --test crates/collector/ui-tests/*.test.mjs
// Outside ui/ on purpose: the collector serves every file in there.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
	atLength, box, canAdd, corner, crossesItself, fanColumn, followImage, fromImage, frontAreas, inOpening, inside, labelAt, measure, middleOf, mm, roomAt, roomWidth, snap, stacks, toImage, toUnits, turnPlan, wallPieces,
} from "../ui/geometry.js";

// Made-up plans: a row of two rooms over a row of three, and an L.
const TWO_ROWS = {
	rooms: [
		{ id: "a", name: "A", points: [[0, 0], [5, 0], [5, 4], [0, 4]] },
		{ id: "b", name: "B", points: [[5, 0], [9, 0], [9, 4], [5, 4]] },
		{ id: "c", name: "C", points: [[0, 4], [3, 4], [3, 7], [0, 7]] },
		{ id: "d", name: "D", points: [[3, 4], [7, 4], [7, 7], [3, 7]] },
		{ id: "e", name: "E", points: [[7, 4], [9, 4], [9, 7], [7, 7]] },
	],
	openings: [{ from: [0, 4], to: [3, 4], at: 0.5, width: 0.8 }],
};
const L = { rooms: [{ id: "l", name: "L", points: [[0, 0], [2, 0], [2, 4], [6, 4], [6, 6], [0, 6]] }] };
const dist = (a, b) => Math.hypot(b[0] - a[0], b[1] - a[1]);
const length = (pieces) => pieces.reduce((s, [a, b]) => s + Math.hypot(b[0] - a[0], b[1] - a[1]), 0);

test("a plan fills its frame, whatever its shape", () => {
	const tall = { rooms: [{ id: "t", name: "T", points: [[0, 0], [6, 0], [6, 12], [0, 12]] }] };
	for (const plan of [TWO_ROWS, L, tall]) {
		const u = toUnits(plan);
		const pts = u.rooms.flatMap((r) => r.points);
		const near = (a, b) => Math.abs(a - b) < 1e-9;
		assert.ok(near(Math.min(...pts.map((p) => p[0])), 40) && near(Math.max(...pts.map((p) => p[0])), u.w - 40));
		assert.ok(near(Math.min(...pts.map((p) => p[1])), 40) && near(Math.max(...pts.map((p) => p[1])), u.h - 40));
		assert.ok((near(u.w, 1000) || near(u.h, 600)) && u.w <= 1000 + 1e-9 && u.h <= 600 + 1e-9);
	}
});

test("shared walls are drawn once, split at T-junctions", () => {
	const pieces = wallPieces(TWO_ROWS.rooms);
	// the outline, the middle line, one wall between the top rooms, two between the bottom ones
	assert.equal(length(pieces), 2 * (9 + 7) + 9 + 4 + 2 * 3);
	const keys = pieces.map(([a, b]) => [corner(a), corner(b)].sort().join(" "));
	assert.equal(new Set(keys).size, keys.length);
	// (3, 4) is on A's bottom edge, not a corner of A: three pieces meet there
	assert.equal(pieces.filter(([a, b]) => corner(a) === "3,4" || corner(b) === "3,4").length, 3);
});

test("an opening named on a short edge cuts the long one it lies on", () => {
	const { openings } = TWO_ROWS;
	assert.ok(inOpening([1.5, 4], openings));
	assert.ok(!inOpening([0.5, 4], openings));
	assert.ok(!inOpening([1.5, 4.1], openings));
});

test("labels and middles are inside their rooms, an L included", () => {
	for (const r of [...toUnits(TWO_ROWS).rooms, ...toUnits(L).rooms]) {
		assert.ok(inside(labelAt(r.points), r.points), `${r.name}'s label`);
		assert.ok(inside(middleOf(r.points), r.points), `${r.name}'s middle`);
	}
	assert.ok(!inside([4, 2], L.rooms[0].points), "the L's missing corner");
});

test("a pointer near a corner lands on that corner, the same array", () => {
	const c = [5, 4];
	const { at, how } = snap([5.06, 3.97], { corners: [[0, 0], c], reach: 0.1 });
	assert.equal(how, "corner");
	assert.equal(at, c);
	assert.equal(snap([5.2, 3.97], { corners: [c], reach: 0.1 }).how, "grid");
});

test("a line that nearly is square to the last one becomes square, in grid steps", () => {
	// the first line snaps to the axes
	assert.deepEqual(snap([3.04, 0.21], { last: [0, 0] }), { at: [3, 0], how: "square" });
	assert.deepEqual(snap([0.1, -2.47], { last: [0, 0] }), { at: [0, -2.5], how: "square" });
	// after a line at 30°, square means along it or at 120°
	const prev = [0, 0], last = [mm(Math.cos(Math.PI / 6) * 2), mm(Math.sin(Math.PI / 6) * 2)];
	const { at, how } = snap([last[0] - 0.5 + 0.03, last[1] + 0.866], { last, prev });
	assert.equal(how, "square");
	const turn = Math.atan2(at[1] - last[1], at[0] - last[0]) - Math.atan2(last[1] - prev[1], last[0] - prev[0]);
	assert.ok(Math.abs(Math.abs(turn) - Math.PI / 2) < 1e-3);
	assert.ok(Math.abs(Math.hypot(at[0] - last[0], at[1] - last[1]) - 1) < 1e-3);
	// level with a corner within reach beats the grid: a room closes square
	// even when its first corner is off the grid
	const chain = [[0.13, -0.1], [3.9, -0.1], [3.9, 3]];
	assert.deepEqual(snap([0.04, 3.02], { align: chain, last: chain[2], prev: chain[1], reach: 0.25 }), { at: [0.13, 3], how: "square" });
	assert.deepEqual(snap([0.04, 3.02], { align: chain, last: chain[2], prev: chain[1], reach: 0.05 }), { at: [0, 3], how: "square" });
	// leaning more than that: the grid, in line with a corner within reach
	assert.deepEqual(snap([7.13, 1.04], { align: [[7.15, -0.1]], reach: 0.1 }), { at: [7.15, 1], how: "grid" });
	assert.deepEqual(snap([7.13, 1.04], { align: [[7.15, -0.1]], reach: 0.01 }), { at: [7.1, 1], how: "grid" });
	assert.deepEqual(snap([1.03, 0.98], { last: [0, 0] }), { at: [1, 1], how: "grid" });
});

test("corners built from the grid or a typed length are exact", () => {
	// the invariant shared corners need: the same numbers, so the same corner() key
	let p = [0, 0];
	for (let i = 0; i < 42; i++) p = snap([p[0] + 0.1, p[1] + 0.003], { last: p }).at;
	assert.deepEqual(p, [4.2, 0]);
	assert.equal(corner(snap([0.30000000000000004, 0.7], {}).at), "0.3,0.7");
	assert.deepEqual(atLength([1.2, 0.4], [1.2, 9], 4.2), [1.2, 4.6]);
	assert.deepEqual(atLength([0, 0], [3, 4], 2.5), [1.5, 2]);
});

test("a corner that would cross the room drawn so far is refused", () => {
	const chain = [[0, 0], [4, 0], [4, 3]];
	assert.ok(canAdd(chain, [0, 3]));
	assert.ok(!canAdd(chain, [4, 0]), "a corner twice");
	assert.ok(!canAdd(chain, [4, 1]), "back along the last line");
	assert.ok(!canAdd(chain, [2, -1]), "across the first line");
	assert.ok(!canAdd(chain, [2, 0]), "onto the first line");
	assert.ok(canAdd([[0, 0], [4, 0]], [4, 0.5]));
});

test("a closed room may not cross itself", () => {
	assert.ok(!crossesItself([[0, 0], [4, 0], [4, 3], [0, 3]]));
	assert.ok(!crossesItself(L.rooms[0].points));
	assert.ok(crossesItself([[0, 0], [4, 0], [0, 3], [4, 3]]), "a bow tie");
	assert.ok(crossesItself([[0, 0], [4, 0], [2, 0]]), "folded flat");
	assert.ok(crossesItself([[0, 0], [4, 0], [4, 3], [2, 0], [0, 3]]), "a corner on another edge");
});

test("a room's width: a strip's is its width, a sliver's is near 0", () => {
	assert.ok(Math.abs(roomWidth([[0, 0], [8, 0], [8, 0.9], [0, 0.9]]) - 0.9 * 8 / 8.9) < 1e-9);
	assert.ok(Math.abs(roomWidth(box([0, 0], [4, 3])) - 12 / 7) < 1e-9);
	assert.ok(roomWidth([[0, 0], [8, 0], [8, 0.28]]) < 0.2, "the sliver a double-click made");
	assert.deepEqual(box([1, 2], [0, 5]), [[1, 2], [0, 2], [0, 5], [1, 5]]);
	assert.ok(!crossesItself(box([1, 2], [0, 5])));
});

test("the underlay: image and plan points map both ways", () => {
	const u = { pixels_per_m: 38.6, offset: [-1.3, 0.7], angle: 0.21 };
	for (const p of [[0, 0], [412, 97], [-15, 803]]) {
		const back = toImage(u, fromImage(u, p));
		assert.ok(dist(back, p) < 1e-9, `${p} came back as ${back}`);
	}
	assert.deepEqual(fromImage({ pixels_per_m: 50, offset: [2, 3], angle: 0 }, [100, 50]), [4, 4]);
});

test("measuring scales the image about the first point and squares a leaning wall", () => {
	const start = { pixels_per_m: 20, offset: [0.4, -0.2], angle: 0 };
	// the ends of a wall in the image, 3° off level, measured as 6.35 m
	const [ua, ub] = [[120, 300], [120 + 500 * Math.cos(0.052), 300 + 500 * Math.sin(0.052)]];
	const [a, b] = [fromImage(start, ua), fromImage(start, ub)];
	const u = measure(start, a, b, 6.35);
	const [na, nb] = [fromImage(u, ua), fromImage(u, ub)];
	assert.ok(dist(na, a) < 1e-9, "the first point stays");
	assert.ok(Math.abs(dist(na, nb) - 6.35) < 1e-9, "the wall is as long as typed");
	assert.ok(Math.abs(nb[1] - na[1]) < 1e-9, "and level");
	assert.ok(Math.abs(u.pixels_per_m - 500 / 6.35) < 1e-9);
	// measuring the same wall again changes nothing
	const again = measure(u, na, nb, 6.35);
	for (const k of ["pixels_per_m", "angle"]) assert.ok(Math.abs(again[k] - u[k]) < 1e-9, k);
	// an upright wall 4° off turns upright
	const up = measure(start, fromImage(start, [0, 0]), fromImage(start, [-400 * Math.sin(0.07), 400 * Math.cos(0.07)]), 5);
	const [p, q] = [fromImage(up, [0, 0]), fromImage(up, [-400 * Math.sin(0.07), 400 * Math.cos(0.07)])];
	assert.ok(Math.abs(q[0] - p[0]) < 1e-9 && q[1] > p[1]);
});

test("measuring a diagonal keeps the image's angle", () => {
	const start = { pixels_per_m: 20, offset: [0, 0], angle: 0.03 };
	const u = measure(start, fromImage(start, [0, 0]), fromImage(start, [300, 300]), 4);
	assert.equal(u.angle, 0.03);
	assert.ok(Math.abs(u.pixels_per_m - Math.hypot(300, 300) / 4) < 1e-9);
});

// Made up at random, as in docs/floorplan.md.
const PLACES = {
	router: [1.5, 0.4],
	sensors: { study: [3.1, 2.2] },
	devices: { "fa:42:31:98:3e:b5": { at: [0.8, 2] }, "c2:d7:2c:93:34:d2": { carried_by: "Robin" } },
};

test("turning the plan takes rooms, openings and the image round together", () => {
	const plan = { ...TWO_ROWS, underlay: { pixels_per_m: 41.3, offset: [-0.7, 0.4], angle: 2.9 }, places: PLACES };
	const right = turnPlan(plan, 1);
	// (1, 0) goes to (0, 1): clockwise on screen, y down
	assert.deepEqual(right.rooms[0].points[1], [0, 5]);
	// the image's pixels land where the turned plan says
	for (const p of [[0, 0], [530, 210]]) {
		const [x, y] = fromImage(plan.underlay, p);
		assert.ok(dist(fromImage(right.underlay, p), [-y, x]) < 1e-9);
	}
	assert.ok(Math.abs(right.underlay.angle) <= Math.PI, "the angle stays in what the collector takes");
	// a shared corner is still the same numbers in both rooms, and the doorway on its edge
	const [a, b] = right.rooms;
	assert.ok(a.points.some((p) => b.points.some((q) => corner(p) === corner(q))));
	// the doorway's middle, (1.5, 4), turned
	assert.ok(inOpening([-4, 1.5], right.openings));
	// four turns either way are no turn, and left undoes right
	let back = plan;
	for (let i = 0; i < 4; i++) back = turnPlan(back, 1);
	assert.deepEqual(back.rooms, plan.rooms);
	assert.deepEqual(back.places, plan.places, "and the places");
	assert.deepEqual(right.places.router, [-0.4, 1.5]);
	assert.deepEqual(right.places.devices["c2:d7:2c:93:34:d2"], { carried_by: "Robin" });
	assert.deepEqual(turnPlan(right, -1).rooms, plan.rooms);
	assert.ok(dist(turnPlan(right, -1).underlay.offset, plan.underlay.offset) < 1e-12);
	assert.equal(turnPlan({ rooms: [], openings: [] }, 1).underlay, null);
});

test("measuring again carries the traced rooms along with the image", () => {
	// measured from the wrong edge: 11% too big
	const wrong = { pixels_per_m: 50, offset: [0.3, -0.2], angle: 0 };
	const traced = { rooms: TWO_ROWS.rooms, openings: TWO_ROWS.openings, places: PLACES };
	const pixel = (p) => toImage(wrong, p);
	const right = measure(wrong, fromImage(wrong, [100, 100]), fromImage(wrong, [656, 100]), 10);
	const moved = followImage(traced, wrong, right);
	// every corner is still on the same image pixel, to the millimetre
	moved.rooms.forEach((r, i) => r.points.forEach((p, k) => {
		assert.ok(dist(toImage(right, p), pixel(traced.rooms[i].points[k])) < 0.001 * right.pixels_per_m);
	}));
	// lengths shrink with the scale: A's top wall was 5 m
	const [a0, a1] = moved.rooms[0].points;
	assert.ok(Math.abs(dist(a0, a1) - 5 * (50 / right.pixels_per_m)) < 0.002);
	// shared corners are still the same numbers
	const key = (p) => corner(p);
	assert.ok(moved.rooms[0].points.filter((p) => moved.rooms[1].points.some((q) => key(p) === key(q))).length === 2);
	assert.ok(Math.abs(moved.openings[0].width - 0.8 * (50 / right.pixels_per_m)) < 1e-9);
	// the places stay on their pixels too
	const near = (p, q) => dist(toImage(right, p), pixel(q)) < 0.001 * right.pixels_per_m;
	assert.ok(near(moved.places.router, PLACES.router));
	assert.ok(near(moved.places.sensors.study, PLACES.sensors.study));
	assert.ok(near(moved.places.devices["fa:42:31:98:3e:b5"].at, PLACES.devices["fa:42:31:98:3e:b5"].at));
	assert.equal(moved.places.devices["c2:d7:2c:93:34:d2"].carried_by, "Robin");
});

test("places come into plan units with the rooms, and one outside widens the frame", () => {
	const u = toUnits({ ...TWO_ROWS, places: PLACES });
	const at = (p) => u.rooms[0].points[0].map((v, i) => v + (p[i] - TWO_ROWS.rooms[0].points[0][i]) * ((u.rooms[0].points[1][0] - u.rooms[0].points[0][0]) / 5));
	assert.ok(dist(u.places.router, at(PLACES.router)) < 1e-9);
	assert.equal(u.places.devices["c2:d7:2c:93:34:d2"].carried_by, "Robin");
	// a place on a room's corner is that corner's point
	const onCorner = toUnits({ ...TWO_ROWS, places: { router: [5, 4] } });
	assert.deepEqual(onCorner.places.router, onCorner.rooms[0].points[2]);
	assert.equal(roomAt(u.rooms, u.places.router), roomAt(TWO_ROWS.rooms, PLACES.router));
	// a router in the garden stays inside the frame
	const garden = toUnits({ ...TWO_ROWS, places: { router: [12, 3] } });
	assert.ok(garden.places.router[0] <= garden.w - 40 + 1e-9);
	assert.equal(roomAt(garden.rooms, garden.places.router), undefined);
});

test("the areas in front of the house: below it, apart, a slot each, in order", () => {
	const u = toUnits(TWO_ROWS);
	const frame = { left: 40, top: u.h - 40 + 50, width: u.w - 80 };
	const cases = [
		[{ count: 60, compact: true }, { count: 3 }, { count: 1 }],
		[{ count: 1, compact: true }],
		[{ count: 7 }],
		[{ count: 200, compact: true }, ...Array.from({ length: 12 }, () => ({ count: 2 }))],
	];
	for (const groups of cases) {
		const areas = frontAreas(groups.map((g, i) => ({ ...g, name: `g${i}` })), frame);
		assert.equal(areas.length, groups.length);
		areas.forEach((a, i) => {
			assert.equal(a.name, `g${i}`);
			assert.equal(a.slots.length, groups[i].count);
			assert.ok(a.y > Math.max(...u.rooms.flatMap((r) => r.points.map((p) => p[1]))), "past the house");
			if (!a.compact) assert.ok(Math.ceil(a.h / 40) <= 3, "labelled areas stay shallow");
			for (const [x, y] of a.slots) {
				assert.ok(x > a.x && x < a.x + a.w && y > a.y && y < a.y + a.h, "slot in its area");
				assert.ok(!u.rooms.some((r) => inside([x, y], r.points)), "slot in no room");
			}
			const b = areas[i + 1];
			if (b) assert.ok(a.x + a.w < b.x, "areas apart, left to right");
			// slots row by row, no two the same
			assert.equal(new Set(a.slots.map(corner)).size, a.slots.length);
			a.slots.forEach((s, k) => k && assert.ok(s[1] > a.slots[k - 1][1] || (s[1] === a.slots[k - 1][1] && s[0] > a.slots[k - 1][0])));
		});
	}
	// a crowd waiting next to two people fits the house's width
	const crowd = frontAreas([{ count: 60, compact: true }, { count: 3 }, { count: 2 }], frame);
	assert.ok(crowd.at(-1).x + crowd.at(-1).w <= frame.left + frame.width);
	// the same groups, the same slots: nothing reshuffles between overviews
	assert.deepEqual(frontAreas([{ count: 7, compact: true }, { count: 2 }], frame), frontAreas([{ count: 7, compact: true }, { count: 2 }], frame));
});

test("things stack when their names would overlap, through a chain too", () => {
	// a and b overlap, b and c overlap, d stands apart, e overlaps a
	const boxes = [[0, 0, 100, 16], [90, 5, 190, 21], [180, 10, 280, 26], [0, 200, 100, 216], [50, -10, 150, 6]];
	assert.deepEqual(stacks(boxes), [[0, 1, 2, 4], [3]]);
	// boxes that only touch don't stack
	assert.deepEqual(stacks([[0, 0, 10, 10], [10, 0, 20, 10]]), [[0], [1]]);
	assert.deepEqual(stacks([]), []);
	// two stacks apart, whose marks would overlap, become one
	const four = [[0, 0, 50, 16], [40, 0, 90, 16], [0, 30, 50, 46], [40, 30, 90, 46]];
	assert.deepEqual(stacks(four), [[0, 1], [2, 3]]);
	assert.deepEqual(stacks(four, (g) => [0, four[g[0]][1], 120, four[g[0]][1] + 35]), [[0, 1, 2, 3]]);
});

test("a fanned stack's rows: right of its spots, apart, centred on them", () => {
	const spots = [[100, 50], [140, 60], [120, 40], [110, 58]];
	const { rows, left } = fanColumn(spots, { gap: 40, row: 18 });
	assert.equal(left, false);
	assert.equal(rows.length, spots.length);
	for (const [x] of rows) assert.equal(x, 180);
	rows.forEach((r, i) => i && assert.equal(r[1] - rows[i - 1][1], 18));
	const mid = (ps) => ps.reduce((s, p) => s + p[1], 0) / ps.length;
	assert.ok(Math.abs(mid(rows) - mid(spots)) < 1e-9);
	// rows as name boxes don't stack with each other
	assert.equal(stacks(rows.map(([x, y]) => [x - 6, y - 8, x + 200, y + 8])).length, rows.length);
	// near the right edge it goes left, its names too; near the top it shifts down
	const edge = fanColumn([[900, 5], [920, 8], [910, 12]], { nameW: 150, w: 1000, h: 600 });
	assert.ok(edge.left && edge.rows.every(([x]) => x === 860 && x - 150 >= 0));
	assert.ok(edge.rows.every(([, y]) => y - 9 >= 0));
});
