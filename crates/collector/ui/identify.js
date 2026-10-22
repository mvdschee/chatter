// Identify mode's view (docs/identify.md): a sensor carried around on a
// phone says which device is next to it. "What's here" lists what it hears,
// strongest first; "Find" follows one device's signal. It reads
// /v1/identify while it's open, and starts and stops a session from the
// sensor's panel.

import { DEVICES, vendorless } from "./data.js";
import { editing } from "./edit.js";
import { esc, fmtAgo } from "./format.js";
import { closePanel, panel, select } from "./panel.js";
import { resize } from "./view.js";

// A session's length, and what "+10 min" adds; the collector takes at most
// an hour.
const SESSION_S = 600;
const MAX_S = 3600;
// The sensor posts every 3 s; the collector's readings go stale after 10.
const POLL_MS = 1500;
const STALE_S = 10;
// The strongest device is marked as the one next to the sensor when it's
// this much above the next.
const STANDS_OUT_DB = 6;
// The signal bar: from noise to touching.
const BAR_FROM = -90, BAR_TO = -30;

// The newest /v1/identify: `{ sensors, online, own_mac }`.
let VIEW = { sensors: [], online: [], own_mac: null };

export let identifyOpen = false;
let sensor = null;
// whether the view saw the session running: once it's gone, it's over
let seen = false;
let tab = "here";
let picked = null; // the MAC Find follows
let timer = null;
let shown = "";

const appEl = document.querySelector(".app");
const viewEl = document.getElementById("identify");
const sensorEl = document.getElementById("id-sensor");
const statusEl = document.getElementById("id-status");
const ctlEl = document.getElementById("id-ctl");
const bodyEl = document.getElementById("id-body");
const findEl = document.getElementById("id-find");
const tabBtns = viewEl.querySelectorAll("[data-tab]");

async function refresh() {
	try {
		const res = await fetch("/v1/identify");
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		VIEW = await res.json();
	} catch (e) {
		console.error("identify:", e);
	}
}

// Starts a session for `secs`, extends it, or stops it with 0. Returns why
// it failed, or null.
async function startIdentify(name, secs) {
	try {
		const res = await fetch(`/v1/sensors/${encodeURIComponent(name)}/identify`, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ secs }),
		});
		if (!res.ok) return (await res.text()) || `HTTP ${res.status}`;
	} catch (e) {
		return e.message;
	}
	await refresh();
	return null;
}

const sessionOf = (name) => VIEW.sensors.find((s) => s.sensor === name);
const mmss = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

// --- the sensor panel's Mode section ----------------------------------

// Fills the panel's Mode section from the newest answer, then asks again.
export function showMode(name) {
	renderMode(name);
	refresh().then(() => renderMode(name));
}

function renderMode(name) {
	const el = document.getElementById("sensor-mode");
	if (!el) return;
	const s = sessionOf(name);
	el.innerHTML = s
		? `<p>Identify: ${mmss(s.left_s)} left. It doesn't count meanwhile.</p>
			<button class="top-btn" id="mode-open">Open identify</button>
			<button class="top-btn" id="mode-stop">Stop</button>`
		: `<p>Normal: counting, and reporting every minute.</p>
			<button class="top-btn" id="mode-start">Identify, ${SESSION_S / 60} min</button>
			<p class="note">Carry the sensor on a phone and hold it next to a device to tell which one it is. It stops counting until it's plugged back in its place, even when stopped early.</p>`;
	const note = (why) => el.insertAdjacentHTML("beforeend", `<p class="note">Not done: ${esc(why)}</p>`);
	document.getElementById("mode-open")?.addEventListener("click", () => openIdentify(name));
	document.getElementById("mode-stop")?.addEventListener("click", async () => {
		const why = await startIdentify(name, 0);
		if (why) note(why);
		else renderMode(name);
	});
	document.getElementById("mode-start")?.addEventListener("click", async () => {
		const why = await startIdentify(name, SESSION_S);
		if (why) note(why);
		else openIdentify(name);
	});
}

// --- the view ------------------------------------------------------------

export function openIdentify(name) {
	closePanel();
	sensor = name;
	seen = false;
	picked = null;
	identifyOpen = true;
	viewEl.hidden = false;
	appEl.classList.add("identifying");
	render();
	clearInterval(timer);
	timer = setInterval(() => refresh().then(render), POLL_MS);
}

function closeIdentify() {
	identifyOpen = false;
	viewEl.hidden = true;
	appEl.classList.remove("identifying");
	// the plan was hidden: a resize meanwhile measured it as nothing
	resize();
	clearInterval(timer);
	shown = "";
}

// The devices you carry, kept in this browser: they'd always be next to the
// sensor. The phone the page is open on is one too, found by the collector.
const CARRIED_KEY = "chatter.carried";
function storedCarried() {
	try {
		return new Set(JSON.parse(localStorage.getItem(CARRIED_KEY) ?? "[]"));
	} catch {
		return new Set();
	}
}
let carried = storedCarried();
const isCarried = (mac) => carried.has(mac) || mac === VIEW.own_mac;
function toggleCarried(mac) {
	if (carried.has(mac)) carried.delete(mac);
	else carried.add(mac);
	try {
		localStorage.setItem(CARRIED_KEY, JSON.stringify([...carried]));
	} catch {
		// not kept: it lasts until the page is reloaded
	}
}

// A MAC as the page knows it: its device from the overview, or just the MAC.
function who(mac) {
	const d = DEVICES.find((x) => x.mac === mac);
	const ip = VIEW.online.find((x) => x.mac === mac)?.ip ?? d?.ip;
	return { d, name: d?.name ?? mac, vendor: d?.vendor ?? vendorless(mac), ip };
}

const barPct = (rssi) => Math.round(100 * Math.min(1, Math.max(0, (rssi - BAR_FROM) / (BAR_TO - BAR_FROM))));
const bar = (rssi) => `<span class="id-bar"><span style="width:${barPct(rssi)}%"></span></span>`;

function carryBtn(mac) {
	if (mac === VIEW.own_mac) return `<span class="id-tag">this phone</span>`;
	const on = carried.has(mac);
	return `<button class="tl-btn id-carry" data-carry="${mac}" aria-pressed="${on}">${on ? "Carried" : "I carry this"}</button>`;
}

function statusText(s) {
	if (!s) return seen
		? `Over. Plug ${esc(sensor)} back in its place: it counts again once it has booted.`
		: `${esc(sensor)} isn't identifying. Start it from the sensor's panel.`;
	if (s.received_at == null) return `Unplug ${esc(sensor)} and plug it into this phone: it starts once it has booted and joined the Wi-Fi, about half a minute. Left in its place, it starts with its next minute report.`;
	const age = Date.now() / 1000 - s.received_at;
	if (age > STALE_S) return `Nothing from ${esc(sensor)} since ${fmtAgo(age)}. Being moved? It carries on once it has booted.`;
	return `Hearing ${s.heard.length} device${s.heard.length === 1 ? "" : "s"} · ${mmss(s.left_s)} left`;
}

function hereHtml(s) {
	if (!s?.heard.length) return "";
	const heard = s.heard.filter((h) => !isCarried(h.mac));
	const left = s.heard.length - heard.length;
	const out = (heard[1]?.rssi_max ?? BAR_FROM) + STANDS_OUT_DB;
	const rows = heard.map((h, i) => {
		const { d, name, vendor } = who(h.mac);
		const mark = i === 0 && h.rssi_max >= out ? `<span class="id-tag on">next to it</span>` : "";
		const open = d ? `data-open="${h.mac}"` : "disabled";
		return `<li><button class="id-row" ${open}><span class="id-name">${esc(name)}${mark}</span>`
			+ `<span class="id-sub">${esc(vendor)}${d ? "" : " · not seen in 24 h"}</span>`
			+ `<span class="id-dbm">${h.rssi_max} dBm</span>${bar(h.rssi_max)}</button>${carryBtn(h.mac)}</li>`;
	});
	const note = left ? `<p class="note">Left out: ${left} you carry. Unmark them in Find.</p>` : "";
	return `<ul class="id-list">${rows.join("")}</ul>${note}`;
}

// Find: every device, online or not, picked by name, vendor, IP or MAC.
function findHtml(s) {
	const heard = new Map((s?.heard ?? []).map((h) => [h.mac, h]));
	const online = new Set(VIEW.online.map((x) => x.mac));
	if (picked) {
		const { d, name, vendor, ip } = who(picked);
		const h = heard.get(picked);
		const signal = h
			? `<div class="id-big">${h.rssi_max}<small> dBm</small></div>${bar(h.rssi_max)}<p class="note">${h.frames} frames in the last post</p>`
			: `<div class="id-big off">not heard</div>${bar(BAR_FROM)}<p class="note">${online.has(picked) ? "Online, but sending nothing the sensor hears." : "Offline: it didn't answer the newest sweep."}</p>`;
		return `<button class="back" data-pick="">← All devices</button>
			<h2 class="name">${esc(name)}</h2><div class="where">${esc(vendor)}${ip ? ` · ${esc(ip)}` : ""}</div>
			${signal}
			<div class="id-acts">${d ? `<button class="top-btn" data-open="${picked}">Name or place it</button>` : ""}${carryBtn(picked)}</div>`;
	}
	const q = findEl.value.trim().toLowerCase();
	const macs = [...new Set([...online, ...DEVICES.map((d) => d.mac)])];
	const rows = macs.map((mac) => ({ mac, ...who(mac), on: online.has(mac) || heard.has(mac) }))
		.filter((x) => !q || [x.name, x.vendor, x.ip ?? "", x.mac].some((t) => t.toLowerCase().includes(q)))
		.sort((a, b) => b.on - a.on || a.name.localeCompare(b.name))
		.map((x) => {
			const h = heard.get(x.mac);
			return `<li class="${x.on ? "" : "off"}"><button class="id-row" data-pick="${x.mac}">`
				+ `<span class="id-name"><span class="dot ${x.on ? "live" : ""}"></span>${esc(x.name)}</span>`
				+ `<span class="id-sub">${esc(x.vendor)}${x.ip ? ` · ${esc(x.ip)}` : ""}</span>`
				+ `<span class="id-dbm">${h ? `${h.rssi_max} dBm` : x.on ? "online" : "offline"}</span></button>${carryBtn(x.mac)}</li>`;
		});
	const swept = VIEW.online.some((x) => x.ip) ? "" : `<p class="note">No sweep yet, so only what the sensor hears is online.</p>`;
	return `${swept}<ul class="id-list">${rows.join("") || `<li class="note">Nothing matches.</li>`}</ul>`;
}

function render() {
	if (!identifyOpen) return;
	const s = sessionOf(sensor);
	if (s) seen = true;
	sensorEl.textContent = sensor;
	statusEl.innerHTML = statusText(s);
	ctlEl.hidden = !s;
	findEl.hidden = tab !== "find" || picked !== null;
	tabBtns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.tab === tab)));
	const html = tab === "here" ? hereHtml(s) : findHtml(s);
	// only when it changed, so a tap isn't lost to a list drawn anew
	if (html !== shown) bodyEl.innerHTML = shown = html;
}

bodyEl.addEventListener("click", (ev) => {
	const b = ev.target.closest("button");
	if (!b) return;
	if (b.dataset.carry) toggleCarried(b.dataset.carry);
	else if (b.dataset.pick !== undefined) picked = b.dataset.pick || null;
	else if (b.dataset.open) select(DEVICES.find((d) => d.mac === b.dataset.open));
	render();
});
tabBtns.forEach((b) => b.addEventListener("click", () => {
	tab = b.dataset.tab;
	picked = null;
	render();
}));
findEl.addEventListener("input", render);
document.getElementById("id-close").addEventListener("click", closeIdentify);
document.getElementById("id-more").addEventListener("click", async () => {
	const s = sessionOf(sensor);
	if (s) await startIdentify(sensor, Math.min(s.left_s + SESSION_S, MAX_S));
	render();
});
document.getElementById("id-stop").addEventListener("click", async () => {
	await startIdentify(sensor, 0);
	render();
});
// Esc closes the device panel over the view first (panel.js), then the view;
// in capture, so it sees the panel before it's closed
document.addEventListener("keydown", (ev) => {
	if (ev.key === "Escape" && identifyOpen && !panel && !editing) closeIdentify();
}, true);
