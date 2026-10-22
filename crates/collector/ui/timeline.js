// The timeline under the plan (docs/timeline.md): the graph of the network's
// traffic over a range, its playhead, and play, live and zoom.

import { heat, heatColor } from "./colour.js";
import { DEVICES, LOOP, PORT_SCANS, SESSIONS, TL, bucketOf, bucketTime, buckets, busiestAt, loadTimeline, peakAt, seriesAt, sum, totalAt } from "./data.js";
import { DAY_MONTH, MiB, fmtBytes, hhmm, whenLabel } from "./format.js";
import { MINUTE_MS, play } from "./playback.js";
import { dpr, reduced } from "./view.js";

// The ranges to pick from, the one picked, and a stretch zoomed in to on the
// graph (`{ from, to }`, unix seconds) or null.
export const RANGES = [["1h", "1 hour"], ["6h", "6 hours"], ["24h", "24 hours"], ["7d", "7 days"], ["30d", "30 days"]];
export let range = "24h";
export let zoomed = null;

// The heatmap: a row per room, a column per 5 minutes, drawn once per change
// into `heatCache`; each frame adds the playhead and the hover line.
const heatEl = document.getElementById("heat");
const hctx = heatEl.getContext("2d");
const heatCache = document.createElement("canvas");
const playBtn = document.getElementById("play");
const liveBtn = document.getElementById("live");
const peakBtn = document.getElementById("peak");
const atEl = document.getElementById("tl-at");
const GUTTER = 70, TOP = 22, AXIS = 16;
export let heatDirty = true, hover = null, dragging = false, peak = 0;
let graphMax = 1; // the y axis's top, bytes a minute
let picking = null; // shift-drag: the stretch being picked, [from, to] buckets

// The graph is drawn again on the next frame.
export function redrawGraph() {
	heatDirty = true;
}

function heatGeom() {
	const r = heatEl.getBoundingClientRect();
	return { w: r.width, h: r.height, cw: (r.width - GUTTER) / Math.max(1, buckets()), ph: r.height - TOP - AXIS };
}

const colX = (g, i) => GUTTER + i * g.cw;
const valY = (g, v) => TOP + g.ph * (1 - v / graphMax);

// A round amount at or above `v` bytes: 1, 2 or 5 times a power of ten, in
// the unit fmtBytes shows it in, so the axis reads 20 MiB, not 19.1.
function niceCeil(v) {
	const unit = v < 1024 ? 1 : v < MiB ? 1024 : v < 1024 * MiB ? MiB : 1024 * MiB;
	const x = Math.max(1, v / unit);
	const p = 10 ** Math.floor(Math.log10(x));
	return [1, 2, 5, 10].map((m) => m * p).find((n) => n >= x) * unit;
}

// The highest buckets, at least `apart` buckets from each other.
function hotspots(values, n, apart) {
	const picked = [];
	for (const i of values.map((_, i) => i).sort((a, b) => values[b] - values[a])) {
		if (values[i] <= 0 || picked.length === n) break;
		if (picked.every((j) => Math.abs(i - j) >= apart)) picked.push(i);
	}
	return picked;
}

// Ticks along the time axis: the smallest step that keeps labels apart, on
// local hours and days. Labels are times within a day, dates beyond.
const TICK_STEPS_MIN = [15, 30, 60, 180, 360, 720, 1440, 2880, 7200, 10_080];
function timeTicks(g) {
	const span = TL.to * 1000 - TL.from * 1000;
	const px = (g.w - GUTTER) / span; // pixels a millisecond
	const step = TICK_STEPS_MIN.find((m) => m * 60_000 * px >= 70) ?? TICK_STEPS_MIN.at(-1);
	const ticks = [];
	const t = new Date(TL.from * 1000);
	t.setSeconds(0, 0);
	t.setMinutes(Math.ceil(t.getMinutes() / Math.min(step, 60)) * Math.min(step, 60));
	for (; t < TL.to * 1000; t.setMinutes(t.getMinutes() + Math.min(step, 60))) {
		const minuteOfDay = t.getHours() * 60 + t.getMinutes();
		const dayNo = Math.floor((t - new Date(2000, 0, 1)) / 86_400_000);
		const on = step < 1440 ? minuteOfDay % step === 0 : minuteOfDay === 0 && dayNo % (step / 1440) === 0;
		if (on) ticks.push({ x: GUTTER + (t - TL.from * 1000) * px, label: step < 1440 ? hhmm(t) : DAY_MONTH.format(t) });
	}
	return ticks;
}

// The stretch as a graph: the network's average a minute per bucket as a line
// with the area under it, a faint band up to each bucket's busiest minute (so
// a short spike survives a long range), coloured by how close it comes to the
// peak. Drawn once per change into `heatCache`; each frame adds the playhead,
// the hover readout and a shift-drag selection.
function renderHeat() {
	const g = heatGeom();
	heatCache.width = Math.round(g.w * dpr);
	heatCache.height = Math.round(g.h * dpr);
	const c = heatCache.getContext("2d");
	c.setTransform(dpr, 0, 0, dpr, 0, 0);
	c.clearRect(0, 0, g.w, g.h);
	const n = buckets();

	const averages = TL.total.map(([a]) => a);
	const peaks = TL.total.map(([, p]) => p);
	const best = Math.max(0, ...peaks);
	peak = peaks.indexOf(best);
	graphMax = niceCeil(best * 1.15);

	// the y axis: gridlines with their amount
	c.font = "10px ui-monospace, Menlo, monospace";
	for (const f of [0, 0.5, 1]) {
		const v = graphMax * f, y = valY(g, v);
		c.fillStyle = "rgba(127,231,242,0.08)";
		c.fillRect(GUTTER, Math.round(y), g.w - GUTTER, 1);
		c.fillStyle = "rgba(92,122,134,0.9)";
		const text = f ? `${fmtBytes(v)}/min` : "0";
		c.fillText(text, GUTTER - 8 - c.measureText(text).width, y + 3.5);
	}

	// the port scans: a band over their span, labelled, red with alerts
	for (const p of PORT_SCANS) {
		const from = Math.max(0, bucketOf(p.from)), to = Math.min(n, bucketOf(p.to));
		if (to <= 0 || from >= n) continue;
		const x1 = colX(g, from), x2 = Math.max(x1 + 2, colX(g, to));
		c.fillStyle = "rgba(127,231,242,0.07)";
		c.fillRect(x1, TOP, x2 - x1, g.ph);
		c.fillStyle = p.alerts ? "rgba(255,59,59,0.9)" : "rgba(127,231,242,0.55)";
		c.fillRect(x1, TOP - 2, x2 - x1, 1);
		const text = p.alerts ? `port scan · ${p.alerts} alert${p.alerts > 1 ? "s" : ""}` : "port scan";
		c.fillText(text, Math.min(x1, g.w - c.measureText(text).width), TOP - 5);
	}

	// identify sessions: a muted band, the sensor didn't count during them
	for (const s of SESSIONS) {
		const from = Math.max(0, bucketOf(s.from)), to = Math.min(n, bucketOf(s.to));
		if (to <= 0 || from >= n) continue;
		const x1 = colX(g, from), x2 = Math.max(x1 + 2, colX(g, to));
		c.fillStyle = "rgba(92,122,134,0.12)";
		c.fillRect(x1, TOP, x2 - x1, g.ph);
		c.fillStyle = "rgba(92,122,134,0.9)";
		c.fillRect(x1, TOP - 2, x2 - x1, 1);
		const text = `identify · ${s.sensor}`;
		c.fillText(text, Math.min(x1, g.w - c.measureText(text).width), TOP - 5);
	}

	if (best > 0) {
		const width = Math.max(1, g.cw - 0.4);
		for (let i = 0; i < n; i++) {
			const ya = valY(g, averages[i]), yp = valY(g, peaks[i]);
			// the peak band, faint, then the average's area
			c.fillStyle = heatColor(peaks[i] / best, 0.12);
			c.fillRect(colX(g, i), yp, width, ya - yp);
			c.fillStyle = heatColor(averages[i] / best, 0.18 + 0.5 * (averages[i] / best));
			c.fillRect(colX(g, i), ya, width, TOP + g.ph - ya);
		}
		c.strokeStyle = "rgba(232,244,246,0.85)";
		c.lineWidth = 1.2;
		c.beginPath();
		averages.forEach((v, i) => (i ? c.lineTo : c.moveTo).call(c, colX(g, i + 0.5), valY(g, v)));
		c.stroke();
	}

	// time along the bottom
	for (const t of timeTicks(g)) {
		c.fillStyle = "rgba(127,231,242,0.18)";
		c.fillRect(t.x, g.h - AXIS, 1, 5);
		if (t.x > GUTTER + 12 && t.x < g.w - 30) {
			c.fillStyle = "rgba(92,122,134,0.9)";
			c.fillText(t.label, t.x - c.measureText(t.label).width / 2, g.h - 2);
		}
	}

	// the hotspots: the busiest minutes, a dot at the peak with when and how much
	for (const i of hotspots(peaks, 3, Math.max(1, Math.round(n / 12)))) {
		const x = colX(g, i + 0.5), y = valY(g, peaks[i]);
		c.fillStyle = heatColor(peaks[i] / best);
		c.beginPath();
		c.arc(x, y, 2.5, 0, Math.PI * 2);
		c.fill();
		const text = `${whenLabel(bucketTime(i))} · ${fmtBytes(peaks[i])}/min`;
		const w = c.measureText(text).width;
		c.fillStyle = "rgba(232,244,246,0.9)";
		c.fillText(text, Math.max(GUTTER, Math.min(x - w / 2, g.w - w)), Math.max(10, y - 7));
	}

	// a mark above the line: the outlier
	if (DEVICES.some((d) => d.outlier && d.minutes.some((m) => heat(m[0]) > 0.9))) {
		c.fillStyle = "rgba(255,59,59,0.9)";
		c.fillRect(colX(g, n - 1), 4, 2, 6);
	}

	const top = n ? busiestAt(peak) : null;
	peakBtn.textContent = best > 0 ? `peak ${whenLabel(bucketTime(peak))} · ${fmtBytes(best)}/min${top ? ` · most: ${top.short}` : ""}` : "";
	heatDirty = false;
}

export function drawTimeline(now) {
	if (heatDirty) renderHeat();
	const g = heatGeom();
	if (heatEl.width !== Math.round(g.w * dpr)) {
		heatEl.width = Math.round(g.w * dpr);
		heatEl.height = Math.round(g.h * dpr);
	}
	hctx.setTransform(1, 0, 0, 1, 0, 0);
	hctx.clearRect(0, 0, heatEl.width, heatEl.height);
	hctx.drawImage(heatCache, 0, 0);
	hctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	hctx.font = "10px ui-monospace, Menlo, monospace";
	if (!buckets()) return;

	// shift-drag: the stretch being picked
	if (picking) {
		const [a, b] = [Math.min(...picking), Math.max(...picking) + 1];
		hctx.fillStyle = "rgba(127,231,242,0.15)";
		hctx.fillRect(colX(g, a), TOP, colX(g, b) - colX(g, a), g.ph);
	}

	// hover: a line, and the time, the average and the peak under the pointer
	if (hover !== null && !dragging) {
		const x = colX(g, hover + 0.5);
		hctx.fillStyle = "rgba(232,244,246,0.25)";
		hctx.fillRect(x, TOP, 1, g.ph);
		const text = `${whenLabel(bucketTime(hover))} · ${fmtBytes(totalAt(hover))}/min, peak ${fmtBytes(peakAt(hover))}`;
		const w = hctx.measureText(text).width;
		hctx.fillStyle = "rgba(4,10,14,0.85)";
		hctx.fillRect(Math.min(x + 6, g.w - w - 8) - 3, TOP + 2, w + 6, 14);
		hctx.fillStyle = "#e8f4f6";
		hctx.fillText(text, Math.min(x + 6, g.w - w - 8), TOP + 12);
	}

	// the playhead: a line with a marker at the top
	const x = colX(g, play.cursor + 0.5);
	const live = play.mode === "live";
	const glow = live && !reduced ? 0.6 + 0.4 * Math.sin(now / 300) : 1;
	hctx.fillStyle = live ? `rgba(127,231,242,${glow.toFixed(2)})` : "#e8f4f6";
	hctx.fillRect(x - 0.5, TOP - 4, 1.5, g.ph + 4);
	hctx.beginPath();
	hctx.moveTo(x - 4, TOP - 6);
	hctx.lineTo(x + 4.5, TOP - 6);
	hctx.lineTo(x, TOP - 1);
	hctx.fill();

	if (live) {
		const left = Math.ceil((MINUTE_MS * LOOP - (now - play.loopStart)) / 1000);
		atEl.innerHTML = `<b>live</b> · replaying the last ${LOOP} minutes · next minute in ${left} s (played ${60_000 / MINUTE_MS}× faster)`;
	} else {
		const i = Math.floor(play.cursor);
		const d = busiestAt(i);
		atEl.innerHTML = `<b>${whenLabel(bucketTime(i))}</b> · ${fmtBytes(totalAt(i))}/min`
			+ (d ? ` · most: ${d.short}, ${fmtBytes(sum(seriesAt(d, i)))}/min` : "");
	}
}

const rangesEl = document.getElementById("ranges");
const zoomOutBtn = document.getElementById("zoom-out-tl");

export function renderControls() {
	playBtn.textContent = play.playing ? "Pause" : "Play";
	liveBtn.setAttribute("aria-pressed", String(play.mode === "live"));
	rangesEl.innerHTML = RANGES.map(([r, label]) =>
		`<button class="tl-btn range" data-range="${r}" aria-pressed="${!zoomed && r === range}" title="the last ${label}">${r}</button>`).join("");
	rangesEl.querySelectorAll("button").forEach((b) => b.addEventListener("click", () => setRange(b.dataset.range)));
	zoomOutBtn.hidden = !zoomed;
}

function setRange(r) {
	range = r;
	zoomed = null;
	play.playing = false;
	loadTimeline();
	renderControls();
}

// Zoom in to buckets a to b (shift-drag), or back out to the range.
function zoomTo(a, b) {
	if (b - a < 2) return;
	zoomed = { from: Math.floor(bucketTime(a) / 1000), to: Math.ceil(bucketTime(b + 1) / 1000) };
	play.playing = false;
	play.mode = "history";
	play.cursor = 0;
	loadTimeline();
	renderControls();
}

function zoomOut() {
	if (!zoomed) return;
	zoomed = null;
	loadTimeline();
	renderControls();
}

export function seek(i) {
	play.mode = "history";
	play.cursor = Math.max(0, Math.min(buckets() - 1, i));
	renderControls();
}

function goLive() {
	play.mode = "live";
	play.playing = false;
	if (zoomed) zoomOut();
	play.cursor = buckets() - 1;
	renderControls();
}

// Play from the playhead; from the start if it's live or at the end.
function togglePlay() {
	if (!play.playing && (play.mode === "live" || play.cursor >= buckets() - 2)) seek(0);
	play.playing = !play.playing;
	renderControls();
}

function bucketAt(ev) {
	const r = heatEl.getBoundingClientRect();
	const n = Math.max(1, buckets());
	return Math.max(0, Math.min(n - 1, Math.floor((ev.clientX - r.left - GUTTER) / ((r.width - GUTTER) / n))));
}

// Drag: replay a moment. Shift-drag: pick a stretch to zoom in to.
heatEl.addEventListener("pointerdown", (ev) => {
	heatEl.setPointerCapture(ev.pointerId);
	if (ev.shiftKey) {
		picking = [bucketAt(ev), bucketAt(ev)];
		return;
	}
	dragging = true;
	seek(bucketAt(ev));
});
heatEl.addEventListener("pointermove", (ev) => {
	hover = bucketAt(ev);
	if (picking) picking[1] = hover;
	else if (dragging) seek(hover);
});
heatEl.addEventListener("pointerup", () => {
	if (picking) zoomTo(Math.min(...picking), Math.max(...picking));
	picking = null;
	dragging = false;
});
heatEl.addEventListener("pointerleave", () => (hover = null));
heatEl.addEventListener("dblclick", zoomOut);
heatEl.addEventListener("keydown", (ev) => {
	const by = ev.shiftKey ? 12 : 1;
	if (ev.key === "ArrowLeft") seek(Math.round(play.cursor) - by);
	else if (ev.key === "ArrowRight") seek(Math.round(play.cursor) + by);
	else if (ev.key === "End") goLive();
	else if (ev.key === " ") togglePlay();
	else if (ev.key === "Escape") zoomOut();
	else return;
	ev.preventDefault();
});
playBtn.addEventListener("click", togglePlay);
liveBtn.addEventListener("click", goLive);
zoomOutBtn.addEventListener("click", zoomOut);
peakBtn.addEventListener("click", () => {
	play.playing = false;
	seek(peak);
});
