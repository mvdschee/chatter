// The side panel: the device list, a device, the Timeline's events (a sensor's
// panel is in sensors.js), and what they show refreshed when the data changes.

import { score, scoreColor } from "./colour.js";
import { DEVICES, EVENTS, LOOP, TL, bucketOf, loopHours, sum } from "./data.js";
import { DATE_TIME, fmtBytes, hhmm } from "./format.js";
import { hourNow, play, shownMoment } from "./playback.js";
import { openSensor, openSensorName } from "./sensors.js";
import { RANGES, range, redrawGraph, seek, zoomed } from "./timeline.js";
import { dpr } from "./view.js";

export const side = document.getElementById("side");
const devicesBtn = document.getElementById("devices-btn");
const eventsBtn = document.getElementById("events-btn");
export let panel = null; // null, "list", "device", "events" or "sensor"
// The device the panel shows, also marked on the plan.
export let selected = null;

// The open device panel's traffic, usual range and bars follow the plan's
// moment.
export function updateMoment(now) {
	if (!selected || panel !== "device") return;
	const m = shownMoment(now);
	const [tx, rx] = m.at(selected);
	const text = {
		"p-when": m.when,
		"p-sent": `${fmtBytes(tx)}/min`,
		"p-received": `${fmtBytes(rx)}/min`,
		"p-total": `${fmtBytes(tx + rx)}/min`,
		"p-usual": usualText(selected, hourNow(now)),
	};
	for (const [id, t] of Object.entries(text)) {
		const el = document.getElementById(id);
		if (el && el.textContent !== t) el.textContent = t;
	}
	if (sparkKey(selected) !== sparkShown) drawSpark(selected);
}

// The device's usual bytes a minute at that hour: the middle half of its
// minutes, from the median and the MAD on the log scale.
function usualText(d, hour) {
	if (!d.usual) return "not worked out yet";
	if (d.usual.learning) return "learning: under a day of data";
	const [median, mad] = hour == null ? d.usual.all : d.usual.hours[hour];
	const [lo, hi] = [median - mad, median + mad].map((v) => fmtBytes(Math.expm1(Math.max(0, v))));
	const when = hour == null ? "over all hours" : `between ${String(hour).padStart(2, "0")}:00 and ${String((hour + 1) % 24).padStart(2, "0")}:00`;
	return `${lo === hi ? `about ${lo}` : `${lo}–${hi}`}/min ${when}`;
}

// New data: the graph and the open panel show it.
export function onData() {
	redrawGraph();
	renderEventsBtn();
	if (panel === "list") renderList();
	if (panel === "events") renderEvents();
	if (panel === "sensor") openSensor(openSensorName);
}

export function showPanel(which) {
	panel = which;
	if (which !== "device") selected = null;
	side.classList.toggle("open", which !== null);
	devicesBtn.setAttribute("aria-expanded", String(which === "list"));
	eventsBtn.setAttribute("aria-expanded", String(which === "events"));
}

export function closePanel() {
	selected = null;
	showPanel(null);
}

function openList() {
	selected = null;
	renderList();
	showPanel("list");
}

function openEvents() {
	selected = null;
	renderEvents();
	showPanel("events");
}

export const CLOSE = `<button class="close" id="close" aria-label="Close">×</button>`;

export function wireClose() {
	document.getElementById("close").addEventListener("click", closePanel);
}

function renderList() {
	const rows = DEVICES.map((d) => {
		const [tx, rx] = d.minutes[LOOP - 1];
		return { d, total: tx + rx };
	}).sort((a, b) => b.total - a.total);
	side.innerHTML = `${CLOSE}<div class="eyebrow">Devices, by the last minute</div><ul class="list">${rows
		.map(({ d, total }) => {
			const color = d.outlier ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : scoreColor(score(d, total, loopHours[LOOP - 1]));
			const rate = d.locationOnly ? "location only" : fmtBytes(total);
			return `<li><button data-id="${d.id}"><span class="swatch" style="background:${color}"></span><span>${d.name}</span><span class="rate">${rate}</span></button></li>`;
		})
		.join("")}</ul><p class="note">Or click a device on the plan.</p>`;
	wireClose();
	side.querySelectorAll("button[data-id]").forEach((b) => b.addEventListener("click", () => select(DEVICES.find((d) => d.id === b.dataset.id))));
}

export function select(d) {
	if (!d) return closePanel();
	selected = d;
	const ports = !d.ports
		? `<span class="note">not port-scanned yet</span>`
		: d.ports.length
			? d.ports.map((p) => `<span class="chip" title="${p.product ?? ""}">${p.port}/${p.protocol}${p.service ? ` ${p.service}` : ""}</span>`).join("")
			: `<span class="note">none open</span>`;
	const scanned = d.portScannedAt ? `<p class="note">port scan of ${DATE_TIME.format(new Date(d.portScannedAt * 1000))}</p>` : "";
	const dns = d.dns.length ? d.dns.map((n) => `<span class="chip">${n}</span>`).join("") : `<span class="note">no lookups seen</span>`;
	const alert = d.outlier ? `<div class="alert">Sent ${fmtBytes(d.outlier.bytes)} in one minute, about ${d.outlier.times}× its usual for this hour. Email sent.</div>` : "";
	side.innerHTML = `
		${CLOSE}
		<button class="back" id="back">← All devices</button>
		${alert}
		<h2 class="name">${d.name}</h2>
		<div class="where">${d.room}${d.locationOnly ? " · location only: its data is multi-stream, the sensors can't count it" : ""}</div>

		<div class="eyebrow" id="p-when"></div>
		<dl class="kv">
			<dt>sent</dt><dd id="p-sent"></dd>
			<dt>received</dt><dd id="p-received"></dd>
			<dt>total</dt><dd id="p-total"></dd>
			<dt>usually</dt><dd id="p-usual"></dd>
		</dl>

		<div class="eyebrow">Traffic, ${zoomed ? "the stretch zoomed in to" : `last ${RANGES.find(([r]) => r === range)[1]}`}</div>
		<canvas class="spark" id="spark"></canvas>

		<div class="eyebrow">Open ports</div>
		<div class="chips">${ports}</div>
		${scanned}

		<div class="eyebrow">Looked up</div>
		<div class="chips">${dns}</div>

		<div class="eyebrow">Device</div>
		<dl class="kv">
			<dt>MAC</dt><dd>${d.mac}</dd>
			<dt>IP</dt><dd>${d.ip ?? "not in a sweep yet"}</dd>
			<dt>hostname</dt><dd>${d.hostname ?? "-"}</dd>
			<dt>vendor</dt><dd>${d.vendor}</dd>
			<dt>signal</dt><dd>${d.rssi ?? "-"} dBm, best in 24 h</dd>
			<dt>first seen</dt><dd>${DATE_TIME.format(new Date(d.firstSeen * 1000))}</dd>
		</dl>`;
	document.getElementById("back").addEventListener("click", openList);
	wireClose();
	showPanel("device");
	drawSpark(d);
	updateMoment(performance.now());
	// a click on a bar takes the timeline there
	document.getElementById("spark").addEventListener("click", (ev) => {
		const { per, bars } = sparkBars(d);
		const r = ev.currentTarget.getBoundingClientRect();
		seek(Math.floor(((ev.clientX - r.left) / r.width) * bars.length) * per);
	});
}

// The device's traffic over the shown stretch in at most 48 bars, each the
// average of `per` buckets.
const sparkPer = (n) => Math.max(1, Math.ceil(n / 48));
function sparkBars(d) {
	const all = TL.series.get(d.mac) ?? [];
	const per = sparkPer(all.length);
	const bars = Array.from({ length: Math.ceil(all.length / per) }, (_, i) => {
		const part = all.slice(i * per, i * per + per);
		return part.reduce((t, x) => t + sum(x), 0) / Math.max(1, part.length);
	});
	return { per, bars };
}

// What the bars show: the stretch, and the bar the plan is at (the last one
// live). It's redrawn when this changes.
function sparkKey(d) {
	const per = sparkPer(TL.series.get(d.mac)?.length ?? 0);
	return `${TL.from}:${TL.to}:${play.mode === "live" ? "live" : Math.floor(Math.floor(play.cursor) / per)}`;
}
let sparkShown = "";

function drawSpark(d) {
	const el = document.getElementById("spark");
	const r = el.getBoundingClientRect();
	el.width = r.width * dpr;
	el.height = r.height * dpr;
	const c = el.getContext("2d");
	c.scale(dpr, dpr);
	sparkShown = sparkKey(d);
	// each bar coloured by its score: against its hour when a bar is one
	// bucket of up to an hour, else against all hours
	const { per, bars } = sparkBars(d);
	const hour = (i) => (per === 1 ? TL.hours[i] : null);
	const at = play.mode === "live" ? bars.length - 1 : Math.min(bars.length - 1, Math.floor(Math.floor(play.cursor) / per));
	const max = Math.max(1, ...bars);
	const bw = r.width / bars.length, top = 8;
	bars.forEach((v, i) => {
		const hgt = (v / max) * (r.height - top);
		c.fillStyle = scoreColor(score(d, v, hour(i)));
		c.globalAlpha = i === at ? 1 : 0.55;
		c.fillRect(i * bw + 1, r.height - hgt, Math.max(1, bw - 2), hgt);
	});
	// the plan's bar: a marker above it, like the timeline's playhead
	if (at >= 0) {
		const x = at * bw + bw / 2;
		c.globalAlpha = 1;
		c.fillStyle = play.mode === "live" ? "#7fe7f2" : "#e8f4f6";
		c.beginPath();
		c.moveTo(x - 4, 0);
		c.lineTo(x + 4.5, 0);
		c.lineTo(x, 5);
		c.fill();
	}
	if (d.locationOnly) {
		c.globalAlpha = 1;
		c.fillStyle = "#5c7a86";
		c.font = "12px ui-monospace, Menlo, monospace";
		c.fillText("no counts: multi-stream device", 0, r.height / 2);
	}
}

// The events, newest first. A click opens the event's device, or replays its
// moment on the plan.
function renderEvents() {
	const sorted = [...EVENTS].sort((a, b) => b.time - a.time);
	const rows = sorted.map((e, i) =>
		`<li><button data-event="${i}"><time>${hhmm(e.time)}</time><span class="${e.hot ? "hot" : ""}">${e.text}</span></button></li>`);
	const empty = rows.length ? "" : `<p class="note">No events yet.</p>`;
	side.innerHTML = `${CLOSE}<div class="eyebrow">Timeline</div><ul class="list events">${rows.join("")}</ul>${empty}`;
	wireClose();
	side.querySelectorAll("button[data-event]").forEach((b) => b.addEventListener("click", () => {
		const e = sorted[Number(b.dataset.event)];
		const d = e.device && DEVICES.find((x) => x.id === e.device);
		if (d) return select(d);
		play.playing = false;
		seek(Math.round(bucketOf(e.from ?? e.time)));
	}));
}

// The button counts the day's alerts.
function renderEventsBtn() {
	const hot = EVENTS.filter((e) => e.hot).length;
	eventsBtn.innerHTML = `Timeline${hot ? `<span class="badge">${hot}</span>` : ""}`;
}

devicesBtn.addEventListener("click", () => (panel === "list" ? closePanel() : openList()));
eventsBtn.addEventListener("click", () => (panel === "events" ? closePanel() : openEvents()));

document.addEventListener("keydown", (ev) => {
	if (ev.key === "Escape" && panel) closePanel();
});
