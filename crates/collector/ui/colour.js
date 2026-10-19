// Colour. Traffic on one absolute scale, white is low, red is high, on a
// log scale: sizes, particles and the graph. A device's score against its own
// normal: the colour of the plan, the list and the panel.

import { SCALE } from "./data.js";
import { MiB } from "./format.js";

const LOW = 1024;
const HIGH = 5 * MiB;

export function heat(bytes) {
	if (bytes <= LOW) return 0;
	return Math.min(1, Math.log(bytes / LOW) / Math.log(HIGH / LOW));
}

const WHITE = [232, 244, 246];
const PALE = [255, 180, 160];
const RED = [255, 59, 59];

export function heatColor(t, alpha = 1) {
	const [a, b, u] = t < 0.5 ? [WHITE, PALE, t / 0.5] : [PALE, RED, (t - 0.5) / 0.5];
	const c = a.map((v, i) => Math.round(v + (b[i] - v) * u));
	return `rgba(${c[0]},${c[1]},${c[2]},${alpha})`;
}

// Colour by a device's own normal (docs/outliers.md): how unusual its bytes a
// minute are for it at that hour of the day. Size and particles stay on
// traffic, through `heat`.
//
// The score: 0 at its usual or below, `SCALE.red` at red. `hour` null: against
// all hours. Null while the device is still learning.
export function score(d, bytes, hour) {
	const u = d.usual;
	if (!u || u.learning || !SCALE) return null;
	const [median, mad] = hour == null ? u.all : u.hours[hour];
	return Math.max(0, (SCALE.k * (Math.log1p(bytes) - median)) / Math.max(mad, SCALE.floor));
}

// White up to `SCALE.white`, then ramping to red at `SCALE.red`; learning is
// a neutral grey.
const LEARNING = [143, 166, 175];
// How far a score is from white (0) to red (1).
export const redness = (s) => (s == null ? 0 : Math.min(1, Math.max(0, (s - SCALE.white) / (SCALE.red - SCALE.white))));
export function scoreColor(s, alpha = 1) {
	if (s == null) return `rgba(${LEARNING.join(",")},${alpha})`;
	return heatColor(redness(s), alpha);
}
