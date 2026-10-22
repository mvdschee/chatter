// The sensors: their state, one status item for all of them in the top bar,
// their list, and a sensor's panel with the heartbeat.

import { SENSORS } from "./data.js";
import { DATE_TIME, esc, fmtAgo, fmtDuration, hhmm } from "./format.js";
import { showMode } from "./identify.js";
import { CLOSE, closePanel, panel, showPanel, side, wireClose } from "./panel.js";
import { dpr } from "./view.js";

// The firmware reports once a minute. Live: a report within two and a half
// of those; late: within ten minutes; else offline.
const REPORT_PERIOD_S = 60;
export function sensorState(s) {
	const age = Date.now() / 1000 - s.last_seen;
	return age <= 2.5 * REPORT_PERIOD_S ? "live" : age <= 600 ? "late" : "offline";
}
export const STATE_COLOR = { live: "#7fe7f2", late: "#f2c14e", offline: "#ff3b3b" };
const STATE_WORD = { live: "online", late: "late", offline: "offline" };

const statusBtn = document.getElementById("sensors-btn");
const statusSub = document.getElementById("sensors-sub");

// The status item: the worst state and how many are in it ("1 offline"),
// else how many are online ("2/2 online"). Every tick.
export function renderSensorStatus() {
	const states = SENSORS.map(sensorState);
	const worst = ["offline", "late"].find((x) => states.includes(x));
	const n = states.filter((x) => x === worst).length;
	statusBtn.querySelector(".dot").className = `dot ${worst ?? (SENSORS.length ? "live" : "")}`;
	statusSub.textContent = !SENSORS.length ? "none yet" : worst ? `${n} ${worst}` : `${SENSORS.length}/${SENSORS.length} online`;
}

// One sensor goes straight to its panel; more to the list first.
statusBtn.addEventListener("click", () => {
	if (panel === "sensor" || panel === "sensors") closePanel();
	else if (SENSORS.length === 1) openSensor(SENSORS[0].name);
	else openSensors();
});

const ago = (s) => fmtAgo(Date.now() / 1000 - s.last_seen);

export function openSensors() {
	side.innerHTML = `${CLOSE}<div class="eyebrow">Sensors</div><ul class="list">${SENSORS.map((s) =>
		`<li><button data-sensor="${esc(s.name)}"><span class="dot ${sensorState(s)}"></span><span>${esc(s.name)}</span>`
		+ `<span class="rate" data-ago="${esc(s.name)}">${ago(s)}</span></button></li>`).join("")}</ul>`;
	wireClose();
	side.querySelectorAll("[data-sensor]").forEach((b) => b.addEventListener("click", () => openSensor(b.dataset.sensor)));
	showPanel("sensors");
}

// The status item, the list's ages and dots, and the open panel's line, every
// tick.
export function updateSensorAges() {
	renderSensorStatus();
	for (const el of side.querySelectorAll("[data-ago]")) {
		const s = SENSORS.find((x) => x.name === el.dataset.ago);
		if (!s) continue;
		el.textContent = ago(s);
		el.parentElement.querySelector(".dot").className = `dot ${sensorState(s)}`;
	}
	const line = document.getElementById("sensor-ago");
	const s = SENSORS.find((x) => x.name === openSensorName);
	if (line && s) line.innerHTML = `<span class="dot ${sensorState(s)}"></span> ${STATE_WORD[sensorState(s)]} · last report ${ago(s)}`;
}

export let openSensorName = null;

export function openSensor(name) {
	const s = SENSORS.find((x) => x.name === name);
	if (!s) return;
	openSensorName = name;
	const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
	const back = SENSORS.length > 1 ? `<button class="back" id="sensors-back">← All sensors</button>` : "";
	side.innerHTML = `
		${CLOSE}
		${back}
		<div class="eyebrow">Sensor</div>
		<h2 class="name">${s.name}</h2>
		<div class="where" id="sensor-ago"></div>

		<div class="eyebrow">Heartbeat</div>
		<div class="beat-label"><span>last 60 minutes, a tick a minute</span><span>now</span></div>
		<canvas class="beat" id="beat-hour"></canvas>
		<div class="beat-label"><span>last 24 hours, a block per half hour</span></div>
		<canvas class="beat" id="beat-day"></canvas>
		<div class="beat-label"><span>24 h ago</span><span>12 h ago</span><span>now</span></div>
		<p class="note" id="beat-summary"></p>

		<div class="eyebrow">Newest report</div>
		<dl class="kv">
			<dt>at</dt><dd>${DATE_TIME.format(new Date(s.last_seen * 1000))}</dd>
			<dt>number</dt><dd>${s.seq}</dd>
			<dt>uptime</dt><dd>${fmtDuration(s.uptime_s)}</dd>
			<dt>channel</dt><dd>${s.channel || "not connected yet"}</dd>
			<dt>networks</dt><dd>${plural(s.nets_known.length, "watched network")} known</dd>
			<dt>devices</dt><dd>${s.devices} heard</dd>
			<dt>dropped</dt><dd>${s.dropped} frames</dd>
			<dt>overflow</dt><dd>${s.overflow} devices</dd>
		</dl>

		<div class="eyebrow">Mode</div>
		<div class="mode" id="sensor-mode"></div>`;
	wireClose();
	document.getElementById("sensors-back")?.addEventListener("click", openSensors);
	showPanel("sensor");
	updateSensorAges();
	drawHeartbeat(s);
	showMode(name);
}

// A sensor's heartbeat as status bars (docs/timeline.md): the last hour a
// tick a minute, the last day a block per half hour. Red marks a report that
// should have come and didn't: one expected a period after the one before,
// when the next came over half a period late (or hasn't yet). A tick is red
// if it holds one; a block is cyan without one, amber with some reports, red
// with none. Grey is before the sensor's first report. Reports drift a second
// now and then, so a minute can pass without one while none is missing: that
// tick stays cyan, unless the sensor is overdue: then every minute after its
// last report is red. Hovering says what's behind a tick or a block.
const BEAT_COLOR = { full: "rgba(127,231,242,0.85)", partial: "rgba(242,193,78,0.9)", none: "rgba(255,59,59,0.85)", before: "rgba(92,122,134,0.25)" };

// When the missing reports were due, from report times in order.
function missedTimes(beats, now) {
	const P = REPORT_PERIOD_S;
	const missed = [];
	beats.forEach((p, i) => {
		const next = beats[i + 1] ?? now;
		for (let t = p + P; t < next - P / 2; t += P) missed.push(t);
	});
	return missed;
}

function beatSlots(s, count, slotS) {
	const now = Date.now() / 1000;
	const start = now - count * slotS;
	const missed = missedTimes(s.beats, now);
	// reports only reach back 24 hours: a first one near that start means it
	// ran before, so nothing is "before its first report"
	const first = s.beats[0] ?? Infinity;
	const ranBefore = s.beats.length > 0 && first - (now - 86_400) < 180;
	const last = s.beats.at(-1);
	const overdue = last !== undefined && now - last > 1.5 * REPORT_PERIOD_S;
	const inSlot = (from, to) => (t) => t >= from && t < to;
	return Array.from({ length: count }, (_, k) => {
		const from = start + k * slotS, to = from + slotS;
		const n = s.beats.filter(inSlot(from, to)).length;
		const m = missed.filter(inSlot(from, to)).length;
		const silent = m > 0 || (overdue && n === 0 && from >= last);
		const state = !ranBefore && to <= first ? "before" : !silent ? "full" : n > 0 ? "partial" : "none";
		return { from, to, n, missed: m, expected: slotS / REPORT_PERIOD_S, state };
	});
}

// The last 24 hours by the clock: the share of the reports that came, and the
// missing ones, as the strips count them.
function beatSummary(s) {
	const now = Date.now() / 1000, since = now - 86_400;
	const got = s.beats.filter((t) => t >= since).length;
	const missing = missedTimes(s.beats, now).filter((t) => t >= since).length;
	return { share: got + missing ? (100 * got) / (got + missing) : 0, missing };
}

function drawBeatStrip(el, slots, describe) {
	const r = el.getBoundingClientRect();
	el.width = r.width * dpr;
	el.height = r.height * dpr;
	const c = el.getContext("2d");
	c.scale(dpr, dpr);
	const w = r.width / slots.length;
	slots.forEach((slot, i) => {
		c.fillStyle = BEAT_COLOR[slot.state];
		c.fillRect(i * w + 0.5, 0, Math.max(1, w - 1.5), r.height);
	});
	el.onpointermove = (ev) => {
		const i = Math.min(slots.length - 1, Math.floor((ev.clientX - el.getBoundingClientRect().left) / w));
		el.title = describe(slots[i]);
	};
}

function drawHeartbeat(s) {
	const hour = beatSlots(s, 60, 60);
	const day = beatSlots(s, 48, 1800);
	const at = (t) => hhmm(new Date(t * 1000));
	drawBeatStrip(document.getElementById("beat-hour"), hour, (x) =>
		`${at(x.from)} · ${x.state === "before" ? "before its first report" : x.missed ? "no report" : x.n ? "report came in" : "on time, just over the minute"}`);
	drawBeatStrip(document.getElementById("beat-day"), day, (x) =>
		`${at(x.from)}–${at(x.to)} · ${x.state === "before" ? "before its first report" : `${x.n} of ${x.expected} reports` + (x.missed ? `, ${x.missed} missing` : "")}`);
	const { share, missing } = beatSummary(s);
	document.getElementById("beat-summary").textContent =
		`${share.toFixed(1)}% of the expected reports in the last 24 hours · ${missing} missing · ${s.restarts} restart${s.restarts === 1 ? "" : "s"}`;
}
