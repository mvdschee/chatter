// What the plan shows at this moment: live, a loop of the last minutes played
// faster; or history, the timeline's bucket at the playhead.

import { heat, score } from "./colour.js";
import { LOOP, TL, bucketTime, buckets, loadOverview, loadTimeline, loopHours, loopTimes, seriesAt, sum } from "./data.js";
import { hhmm, whenLabel } from "./format.js";
import { zoomed } from "./timeline.js";

export const MINUTE_MS = 6000; // how long each replayed minute lasts on screen

// "live" plays the loop of the last minutes; "history" shows the timeline's
// bucket at `cursor`, which is fractional while playing. Play loops over the
// stretch, a whole pass in about a minute at any range. Then the frame clock:
// when the live loop started, the ring went out, the last frame was drawn and
// a glitch ends.
export const play = { mode: "live", cursor: 0, playing: false, loopStart: 0, ringStart: 0, lastFrame: 0, glitchUntil: 0 };
export const PASS_S = 60;

// The hour of the day the plan is showing.
export function hourNow(now) {
	if (play.mode === "history") return TL.hours[Math.floor(play.cursor)] ?? null;
	return loopHours[Math.min(LOOP - 1, Math.floor((now - play.loopStart) / MINUTE_MS))] ?? null;
}

export const scoreNow = (d, now) => score(d, sum(current(d, now)), hourNow(now));

// The minute or bucket the plan shows: when it is, and a device's [tx, rx]
// bytes a minute then.
export function shownMoment(now) {
	if (play.mode === "history") {
		const i = Math.floor(play.cursor);
		const when = TL.bucketMs > 60_000 ? `${whenLabel(bucketTime(i))}–${hhmm(bucketTime(i + 1))}, average` : whenLabel(bucketTime(i));
		return { when, at: (d) => seriesAt(d, i) };
	}
	const i = Math.min(LOOP - 1, Math.floor((now - play.loopStart) / MINUTE_MS));
	return { when: loopTimes[i] ? `live · ${hhmm(loopTimes[i])}` : "live", at: (d) => d.minutes[i] };
}

// Traffic at this moment, worked out once a frame. History plays several
// buckets a second, so it eases toward them (on the log scale the colours
// use), or the plan would flash from bucket to bucket.
const EASE_MS = 300;
export function current(d, now) {
	if (d.curAt === now) return d.cur;
	const target = traffic(d, now);
	const a = play.mode === "history" && d.cur ? 1 - Math.exp(-(now - d.curAt) / EASE_MS) : 1;
	d.cur = a < 1 ? d.cur.map((v, k) => Math.expm1(Math.log1p(v) + (Math.log1p(target[k]) - Math.log1p(v)) * a)) : target;
	d.curAt = now;
	return d.cur;
}

// Live: eases from the previous minute into the current one over the first
// second. History: between two buckets while playing. Only real minutes and
// buckets are used.
function traffic(d, now) {
	if (play.mode === "history") {
		const i = Math.floor(play.cursor), j = Math.min(buckets() - 1, i + 1), u = play.cursor - i;
		const a = seriesAt(d, i), b = seriesAt(d, j);
		return [0, 1].map((k) => a[k] + (b[k] - a[k]) * u);
	}
	const pos = (now - play.loopStart) / MINUTE_MS;
	const i = Math.min(LOOP - 1, Math.floor(pos));
	const prev = d.minutes[(i + LOOP - 1) % LOOP];
	const cur = d.minutes[i];
	const u = Math.min(1, (pos - i) * (MINUTE_MS / 1000));
	return [0, 1].map((k) => prev[k] + (cur[k] - prev[k]) * u);
}

// The device's outlier minute is playing in the live loop.
export function outlierNow(d, now) {
	if (play.mode !== "live" || !d.outlier) return false;
	return heat(d.minutes[Math.min(LOOP - 1, Math.floor((now - play.loopStart) / MINUTE_MS))][0]) > 0.9;
}

// The loop came round: fetch the data again, so a new minute joins if the
// sensor sent one. A zoomed-in stretch doesn't move.
export function newMinute(now) {
	play.ringStart = now;
	loadOverview();
	if (!zoomed) loadTimeline();
}
