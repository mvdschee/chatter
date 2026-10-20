// The floor plan's geometry (ui/geometry.js), checked in node:
//   node --test crates/collector/ui-tests/*.test.mjs
// Outside ui/ on purpose: the collector serves every file in there.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
	atLength, box, canAdd, corner, crossesItself, inOpening, inside, labelAt, middleOf, mm, roomWidth, snap, toUnits, wallPieces,
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
