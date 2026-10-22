// Labels on a canvas that keep clear of each other (docs/ui.md): each label
// goes on the first of its spots that stays inside the canvas and clear of
// every label placed before it, else it's left out. The caller puts them in
// order of importance, so a label can only push out a less important one.

// A label's box around its baseline, for the 10 px monospace the canvases use.
export const ASCENT = 8, DESCENT = 2;
// kept free between two labels side by side, so two read as two
const GAP = 12;

// `labels`: `{ text, at, ... }`, `at` the spots to try in order, each
// `[x, y, align]`: y the baseline, x the left edge, or the right edge or the
// middle with align "right" or "center"; `left`, if given: the leftmost x
// it may start at (a graph's labels kept off its axis). `measure`: a text's
// width. Returns the placed ones as `{ text, x, y, w, ... }`, x their left edge.
export function placeLabels(labels, measure, bounds) {
	const placed = [];
	for (const { at, ...label } of labels) {
		const w = measure(label.text);
		const spot = at
			.map(([x, y, align]) => ({ x: align === "right" ? x - w : align === "center" ? x - w / 2 : x, y, w }))
			.find((b) => b.x >= (label.left ?? 0) && inside(b, bounds) && placed.every((p) => apart(p, b)));
		if (spot) placed.push({ ...label, ...spot });
	}
	return placed;
}

const inside = (b, { w, h }) => b.x >= 0 && b.x + b.w <= w && b.y - ASCENT >= 0 && b.y + DESCENT <= h;
const apart = (a, b) =>
	a.x + a.w + GAP <= b.x || b.x + b.w + GAP <= a.x || a.y + DESCENT <= b.y - ASCENT || b.y + DESCENT <= a.y - ASCENT;
