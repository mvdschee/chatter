// Which devices the page shows (ui/gone.js), checked in node:
//   node --test crates/collector/ui-tests/*.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";
import { GONE_AFTER_S, onNetwork } from "../ui/gone.js";

const NOW = 1_700_000_000;
const DAY = 86400;
const device = (firstAgo, lastAgo) => ({ firstSeen: NOW - firstAgo, lastSeen: NOW - lastAgo });

test("a device goes a week after it was last seen, not before", () => {
	const [kept, gone] = [device(30 * DAY, GONE_AFTER_S), device(30 * DAY, GONE_AFTER_S + 1)];
	assert.deepEqual(onNetwork([kept, gone], NOW), [kept]);
});

test("going back in time brings a gone device back", () => {
	const old = device(40 * DAY, 30 * DAY);
	assert.deepEqual(onNetwork([old], NOW), []);
	assert.deepEqual(onNetwork([old], NOW - 24 * DAY), [old]);
});

test("a device isn't there before it was first heard", () => {
	const fresh = device(DAY, 0);
	assert.deepEqual(onNetwork([fresh], NOW), [fresh]);
	assert.deepEqual(onNetwork([fresh], NOW - DAY), [fresh]);
	assert.deepEqual(onNetwork([fresh], NOW - DAY - 1), []);
});

test("an old address and the new one that replaced it don't show together", () => {
	const [old, replaced] = [device(30 * DAY, 20 * DAY), device(20 * DAY, 0)];
	assert.deepEqual(onNetwork([old, replaced], NOW - 25 * DAY), [old]);
	assert.deepEqual(onNetwork([old, replaced], NOW), [replaced]);
});

test("a device with traffic in the bucket on show is shown, up to the longest bucket", () => {
	// heard only in that bucket, at any point in it
	const end = NOW, bucket = DAY;
	for (const into of [0, 1, bucket - 1]) {
		const t = end - bucket + into;
		const heard = { firstSeen: t, lastSeen: t };
		assert.deepEqual(onNetwork([heard], end), [heard]);
	}
});
