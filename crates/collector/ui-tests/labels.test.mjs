// Placing labels on a canvas (ui/labels.js), checked in node:
//   node --test crates/collector/ui-tests/*.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { ASCENT, DESCENT, placeLabels } from "../ui/labels.js";

// a monospace font: 6 px a character
const measure = (text) => text.length * 6;
const box = (l) => ({ left: l.x, right: l.x + l.w, top: l.y - ASCENT, bottom: l.y + DESCENT });
const overlap = (a, b) => Math.min(a.right, b.right) > Math.max(a.left, b.left) && Math.min(a.bottom, b.bottom) > Math.max(a.top, b.top);

let seed = 3;
const rand = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;

test("placed labels never overlap and stay on the canvas, at a phone's width and a wide screen's", () => {
	for (const w of [346, 1396]) {
		for (let round = 0; round < 200; round++) {
			const bounds = { w, h: 120 };
			// bunched up on purpose: ten bands and peaks within a fifth of the width
			const labels = Array.from({ length: 10 }, (_, i) => {
				const x = rand() * w * 0.2 + (round % 5) * w * 0.2, y = 10 + rand() * 110;
				return { text: `label ${i} ${"x".repeat(Math.floor(rand() * 20))}`, at: [[x, y, "center"], [x + 7, y + 3.5], [x - 7, y + 3.5, "right"], [x, y + 14, "center"]] };
			});
			const placed = placeLabels(labels, measure, bounds).map(box);
			for (const [i, a] of placed.entries()) {
				assert.ok(a.left >= 0 && a.right <= w && a.top >= 0 && a.bottom <= bounds.h, `label ${i} leaves the canvas`);
				for (const b of placed.slice(i + 1)) assert.ok(!overlap(a, b), "two labels overlap");
			}
		}
	}
});

test("the first label is placed at its first spot that fits", () => {
	const [a] = placeLabels([{ text: "abc", at: [[-5, 20], [10, 20]] }], measure, { w: 100, h: 50 });
	assert.deepEqual([a.x, a.y, a.w], [10, 20, 18]);
});

test("a label taken over goes to its next spot, else it's left out", () => {
	const first = { text: "port scan", at: [[50, 20]] };
	const moves = { text: "identify", at: [[52, 20], [200, 20, "right"]] };
	const stuck = { text: "peak", at: [[55, 20]] };
	const placed = placeLabels([first, moves, stuck], measure, { w: 300, h: 50 });
	assert.deepEqual(placed.map((l) => [l.text, l.x]), [["port scan", 50], ["identify", 200 - 48]]);
});

test("a label starts no further left than its `left`", () => {
	const placed = placeLabels([{ text: "peak", left: 70, at: [[60, 20, "center"], [75, 20]] }], measure, { w: 300, h: 50 });
	assert.deepEqual(placed.map((l) => l.x), [75]);
});

test("labels keep their own fields", () => {
	const [l] = placeLabels([{ text: "0", color: "red", at: [[40, 20, "right"]] }], measure, { w: 100, h: 50 });
	assert.equal(l.color, "red");
	assert.equal(l.x, 34);
});
