// Identify mode's page (docs/identify.md): a sensor carried around on a
// phone says which device is next to it. It walks through the steps: pick
// the sensor, move it to the phone, find devices ("What's here" lists what
// it hears, strongest first; "Find" follows one device's signal), and put
// it back. Each step shows what the sensor is doing, from /v1/identify and
// the overview. It opens from the top bar, or at #identify.

import { DEVICES, SENSORS, loadOverview, vendorless } from "./data.js";
import { editing } from "./edit.js";
import { esc, fmtAgo, hhmm } from "./format.js";
import { closePanel, panel, select } from "./panel.js";
import { sensorState } from "./sensors.js";
import { narrow, resize } from "./view.js";

// A session's length, and what "+10 min" adds; the collector takes at most
// an hour.
const SESSION_S = 600;
const MAX_S = 3600;
// The sensor posts every 3 s; the collector's readings go stale after 10.
const POLL_MS = 1500;
const STALE_S = 10;
// While a step waits on a minute report, the overview is fetched this often.
const OVERVIEW_MS = 10_000;
// The strongest device is marked as the one next to the sensor when it's
// this much above the next.
const STANDS_OUT_DB = 6;
// The signal bar: from noise to touching.
const BAR_FROM = -90, BAR_TO = -30;

const STEPS = ["sensor", "move", "find", "back"];

// The newest /v1/identify: `{ sensors, online, own_mac }`.
let VIEW = { sensors: [], online: [], own_mac: null };

export let identifyOpen = false;
let step = "sensor";
let sensor = null;
let endedAt = null; // when the session ended, for the last step
let tab = "here";
let picked = null; // the MAC Find follows
let menuOpen = false; // +10 min and Stop, under the time left
let timer = null;
let overviewAt = 0;
// whether opening the page added #identify, so Back goes back
let pushed = false;
const shown = new Map(); // what each part shows, so it's only redrawn on a change

const appEl = document.querySelector(".app");
const viewEl = document.getElementById("identify");
const stepsEl = document.getElementById("id-steps");
const mainEl = document.getElementById("id-main");
const titleEl = document.getElementById("id-title");
const ctlEl = document.getElementById("id-ctl");
const leftEl = document.getElementById("id-left");
const leftTextEl = document.getElementById("id-left-t");
const findPartEl = document.getElementById("id-find-part");
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
const postAge = (s) => (s?.received_at == null ? null : Date.now() / 1000 - s.received_at);
const posting = (s) => postAge(s) !== null && postAge(s) <= STALE_S;
const mmss = (s) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

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
		? `<p>Identify: ${mmss(s.left_s)} left. It doesn't count meanwhile.</p><button class="top-btn" id="mode-open">Open identify</button>`
		: `<p>Normal: counting, and reporting every minute.</p>`;
	document.getElementById("mode-open")?.addEventListener("click", openIdentify);
}

// --- opening and closing ------------------------------------------------

export function openIdentify() {
	pushed = true;
	location.hash = "identify";
}

function leave() {
	if (pushed) history.back();
	else {
		history.replaceState(null, "", location.pathname + location.search);
		hide();
	}
}

function show() {
	if (identifyOpen) return;
	closePanel();
	identifyOpen = true;
	viewEl.hidden = false;
	appEl.classList.add("identifying");
	step = "sensor";
	picked = null;
	menuOpen = false;
	shown.clear();
	render();
	refresh().then(() => {
		// a session already running: carry on with it
		const s = VIEW.sensors.find((x) => x.sensor === sensor) ?? VIEW.sensors[0];
		if (s) {
			sensor = s.sensor;
			step = posting(s) ? "find" : "move";
		}
		render();
	});
	clearInterval(timer);
	timer = setInterval(tick, POLL_MS);
}

function hide() {
	if (!identifyOpen) return;
	identifyOpen = false;
	pushed = false;
	viewEl.hidden = true;
	appEl.classList.remove("identifying");
	// the plan was hidden: a resize meanwhile measured it as nothing
	resize();
	clearInterval(timer);
}

async function tick() {
	await refresh();
	// waiting on a minute report: the sensor's last one, or its first back
	if ((step === "move" || step === "back") && Date.now() - overviewAt > OVERVIEW_MS) {
		overviewAt = Date.now();
		loadOverview();
	}
	render();
}

// --- the devices you carry ------------------------------------------------

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

// --- the steps ------------------------------------------------------------

// A checklist line: done, now (waiting on it) or todo.
const check = (state, title, detail) => `<li class="${state}"><b>${title}</b>${detail ? `<p>${detail}</p>` : ""}</li>`;

function sensorStep() {
	if (!SENSORS.length) return `<p class="id-say">No sensors have reported yet.</p>`;
	if (!sensor && SENSORS.length === 1) sensor = SENSORS[0].name;
	const rows = SENSORS.map((x) => {
		const s = sessionOf(x.name);
		const what = s ? `identify · ${mmss(s.left_s)} left` : `counting · last report ${fmtAgo(Date.now() / 1000 - x.last_seen)}`;
		return `<li><button class="id-row id-pick" data-sensor="${esc(x.name)}" aria-pressed="${x.name === sensor}">`
			+ `<span class="id-name"><span class="dot ${sensorState(x)}"></span>${esc(x.name)}</span><span class="id-sub">${what}</span></button></li>`;
	});
	const running = sensor && sessionOf(sensor);
	return `<p class="id-say">Pick the sensor to carry.</p>
		<ul class="id-list">${rows.join("")}</ul>
		<p class="note">Counting pauses until it's back in its place, even if you stop early.</p>
		<button class="id-go" data-act="start" ${sensor ? "" : "disabled"}>${running ? "Carry on" : "Start"}</button>`;
}

function moveStep(s) {
	const age = postAge(s);
	const x = SENSORS.find((y) => y.name === sensor);
	const next = x ? Math.round(x.last_seen + 60 - Date.now() / 1000) : null;
	const switched = age !== null
		? check("done", "Switched", "counting paused")
		: check("now", "Switching", `on boot, or at its next report if left in place${next > 0 ? ` (~${next} s)` : ""}`);
	const posts = age === null
		? check("todo", "Posting", "every 3 s, once switched")
		: age <= STALE_S
			? check("done", "Posting", `${plural(s.heard.length, "device")} · ${fmtAgo(age)}`)
			: check("now", "Booting?", `no post for ${fmtAgo(age).replace(" ago", "")} · boot takes ~30 s`);
	return `<p class="id-say">Unplug <b>${esc(sensor)}</b> and plug it into this phone.</p>
		<p class="note">Now is fine: on boot it goes straight to identify.</p>
		<ol class="id-checks">${check("done", "Identify on", "")}${switched}${posts}</ol>
		<button class="id-go" data-act="find" ${posting(s) ? "" : "disabled"}>It's on the phone</button>`;
}

function findStatus(s) {
	const age = postAge(s);
	return age !== null && age <= STALE_S
		? `<p class="id-live"><span class="dot live"></span>Hearing ${plural(s.heard.length, "device")} · ${fmtAgo(age)}</p>`
		: `<p class="id-live"><span class="dot late"></span>No post for ${fmtAgo(age ?? 0).replace(" ago", "")} · still plugged in?</p>`;
}

function backStep() {
	const x = SENSORS.find((y) => y.name === sensor);
	const back = x && x.last_seen > endedAt;
	const counting = back
		? check("done", "Counting again", `first report ${hhmm(new Date(x.last_seen * 1000))}`)
		: check("now", "Counting again", "first report ~1 min after boot");
	return `<p class="id-say">Plug <b>${esc(sensor)}</b> back in its place.</p>
		<ol class="id-checks">${check("done", "Identify ended", hhmm(new Date(endedAt * 1000)))}${counting}</ol>
		<button class="id-go" data-act="close">Done</button>
		<button class="ghost" data-act="again">Identify again</button>`;
}

// --- the Find step's lists ----------------------------------------------

// A MAC as the page knows it: its device from the overview, else what the
// sweep knows (a host the sensor never counted isn't in the overview), else
// just the MAC.
function who(mac) {
	const d = DEVICES.find((x) => x.mac === mac);
	const o = VIEW.online.find((x) => x.mac === mac);
	const host = o?.hostname?.split(".")[0];
	return { d, name: d?.name ?? host ?? mac, vendor: d?.vendor ?? o?.vendor ?? vendorless(mac), ip: o?.ip ?? d?.ip };
}

// Whether the sensor can hear it: now, or its own frames in the last 24
// hours. The rest answer the sweep over 5 GHz, a cable or another access
// point, so Find can't follow them.
const hearable = (mac, heard) => heard.has(mac) || DEVICES.find((x) => x.mac === mac)?.rssi != null;

const barPct = (rssi) => Math.round(100 * Math.min(1, Math.max(0, (rssi - BAR_FROM) / (BAR_TO - BAR_FROM))));
const OUT_OF_REACH = "The sensor hasn't heard it in 24 hours. It listens on 2.4 GHz, on the router's channel, and this one is on 5 GHz, on a cable or on another access point. It answers the network sweep, so it shows as online.";
const OUT_OF_REACH_ALL = "The sensor hasn't heard these in 24 hours. It listens on 2.4 GHz, on the router's channel, and these are on 5 GHz, on a cable or on another access point. They answer the network sweep, so they show as online.";
// whether the out-of-reach list is open, kept while the list is redrawn
let outOpen = false;

const bar = (rssi) => `<span class="id-bar"><span style="width:${barPct(rssi)}%"></span></span>`;

// "I carry this" is only on a device's own view; a list shows the mark.
function carryBtn(mac) {
	if (mac === VIEW.own_mac) return `<span class="id-tag">this phone</span>`;
	const on = carried.has(mac);
	return `<button class="ghost" data-carry="${mac}" aria-pressed="${on}">${on ? "Carried" : "I carry this"}</button>`;
}
const carryTag = (mac) => (mac === VIEW.own_mac ? `<span class="id-tag">this phone</span>` : carried.has(mac) ? `<span class="id-tag">carried</span>` : "");

function hereHtml(s) {
	if (!s?.heard.length) return `<p class="note">Nothing heard yet.</p>`;
	const heard = s.heard.filter((h) => !isCarried(h.mac));
	const left = s.heard.length - heard.length;
	const out = (heard[1]?.rssi_max ?? BAR_FROM) + STANDS_OUT_DB;
	const rows = heard.map((h, i) => {
		const { d, name, vendor } = who(h.mac);
		const mark = i === 0 && h.rssi_max >= out ? `<span class="id-tag on">next to it</span>` : "";
		return `<li><button class="id-row" data-see="${h.mac}"><span class="id-name">${esc(name)}${mark}</span>`
			+ `<span class="id-sub">${esc(vendor)}${d ? "" : " · not seen in 24 h"}</span>`
			+ `<span class="id-dbm">${h.rssi_max} dBm</span>${bar(h.rssi_max)}</button></li>`;
	});
	const note = left ? `<p class="note">${left} carried, left out · unmark one from its view in Find</p>` : "";
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
			: `<div class="id-big off">not heard</div>${bar(BAR_FROM)}<p class="note">${!hearable(picked, heard) ? OUT_OF_REACH : online.has(picked) ? "Online, but sending nothing the sensor hears." : "Offline: it didn't answer the newest sweep."}</p>`;
		return `<button class="ghost id-icon" data-pick=""><svg class="icon" aria-hidden="true"><use href="icons.svg#arrow-left"/></svg>All devices</button>
			<h2 class="name">${esc(name)}</h2><div class="where">${esc(vendor)}${ip ? ` · ${esc(ip)}` : ""}</div>
			${signal}
			<div class="id-acts">${d ? `<button class="ghost" data-open="${picked}">${narrow.matches ? "Name it" : "Name or place it"}</button>` : ""}${carryBtn(picked)}</div>`;
	}
	const q = findEl.value.trim().toLowerCase();
	const macs = [...new Set([...online, ...DEVICES.map((d) => d.mac)])];
	const rows = macs.map((mac) => ({ mac, ...who(mac), on: online.has(mac) || heard.has(mac) }))
		.filter((x) => !q || [x.name, x.vendor, x.ip ?? "", x.mac].some((t) => t.toLowerCase().includes(q)))
		.sort((a, b) => b.on - a.on || a.name.localeCompare(b.name));
	const row = (x) => {
		const h = heard.get(x.mac);
		return `<li class="${x.on ? "" : "off"}"><button class="id-row" data-pick="${x.mac}">`
			+ `<span class="id-name"><span class="dot ${x.on ? "live" : ""}"></span>${esc(x.name)}</span>`
			+ `<span class="id-sub">${esc(x.vendor)}${x.ip ? ` · ${esc(x.ip)}` : ""}</span>`
			+ `<span class="id-dbm">${h ? `${h.rssi_max} dBm` : x.on ? "online" : "offline"}</span></button>${carryTag(x.mac)}</li>`;
	};
	const findable = rows.filter((x) => hearable(x.mac, heard));
	const out = rows.filter((x) => !hearable(x.mac, heard));
	const swept = VIEW.online.some((x) => x.ip) ? "" : `<p class="note">No sweep yet, so only what the sensor hears is online.</p>`;
	const outPart = out.length
		? `<details class="id-out" ${outOpen || q ? "open" : ""}><summary>Out of the sensor's reach (${out.length})</summary>`
			+ `<p class="note">${OUT_OF_REACH_ALL}</p><ul class="id-list">${out.map(row).join("")}</ul></details>`
		: "";
	return `${swept}<ul class="id-list">${findable.map(row).join("") || `<li class="note">Nothing matches.</li>`}</ul>${outPart}`;
}

// --- drawing ----------------------------------------------------------------

// Sets an element's HTML only when it changed, so a tap isn't lost to a
// part drawn anew.
function put(el, html) {
	if (shown.get(el) !== html) {
		el.innerHTML = html;
		shown.set(el, html);
	}
}

function render() {
	if (!identifyOpen) return;
	const s = sessionOf(sensor);
	// the session went (stopped, ran out, or the collector restarted)
	if (!s && (step === "move" || step === "find")) {
		step = "back";
		endedAt = Date.now() / 1000;
	}
	const at = STEPS.indexOf(step);
	stepsEl.querySelectorAll("li").forEach((li, i) => (li.className = i < at ? "done" : i === at ? "now" : ""));
	put(mainEl, { sensor: sensorStep, move: moveStep, find: findStatus, back: backStep }[step](s));
	titleEl.textContent = step === "sensor" ? "Identify" : sensor;
	leftEl.hidden = !s || step === "sensor";
	if (leftEl.hidden) menuOpen = false;
	ctlEl.hidden = !menuOpen;
	leftEl.setAttribute("aria-expanded", String(menuOpen));
	if (s) leftTextEl.textContent = `${mmss(s.left_s)} left`;
	findPartEl.hidden = step !== "find";
	if (step !== "find") return;
	findEl.hidden = tab !== "find" || picked !== null;
	tabBtns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.tab === tab)));
	put(bodyEl, tab === "here" ? hereHtml(s) : findHtml(s));
}

viewEl.addEventListener("click", async (ev) => {
	const b = ev.target.closest("button");
	// any tap closes the time's menu, the time itself toggles it
	const toggle = b?.dataset.act === "time";
	menuOpen = toggle && !menuOpen;
	if (!b || b.disabled || toggle) return render();
	const act = b.dataset.act;
	if (b.dataset.sensor) sensor = b.dataset.sensor;
	else if (b.dataset.see) {
		tab = "find";
		picked = b.dataset.see;
	}
	else if (b.dataset.carry) toggleCarried(b.dataset.carry);
	else if (b.dataset.pick !== undefined) picked = b.dataset.pick || null;
	else if (b.dataset.open) select(DEVICES.find((d) => d.mac === b.dataset.open));
	else if (b.dataset.tab) {
		tab = b.dataset.tab;
		picked = null;
	} else if (act === "start") {
		const s = sessionOf(sensor);
		const why = s ? null : await startIdentify(sensor, SESSION_S);
		if (why) mainEl.insertAdjacentHTML("beforeend", `<p class="note">Not started: ${esc(why)}</p>`);
		else step = posting(sessionOf(sensor)) ? "find" : "move";
		overviewAt = 0;
	} else if (act === "find") step = "find";
	else if (act === "more") {
		const s = sessionOf(sensor);
		if (s) await startIdentify(sensor, Math.min(s.left_s + SESSION_S, MAX_S));
	} else if (act === "stop") {
		await startIdentify(sensor, 0);
		step = "back";
		endedAt = Date.now() / 1000;
	} else if (act === "again") step = "sensor";
	else if (act === "close") return leave();
	render();
});
findEl.addEventListener("input", render);
// toggle doesn't bubble
bodyEl.addEventListener("toggle", (ev) => {
	if (ev.target.classList.contains("id-out")) outOpen = ev.target.open;
}, true);
document.getElementById("id-close").addEventListener("click", leave);
document.getElementById("identify-btn").addEventListener("click", openIdentify);
window.addEventListener("hashchange", () => (location.hash === "#identify" ? show() : hide()));
// after every module has run: show() closes the panel
if (location.hash === "#identify") setTimeout(show);
// Esc closes the device panel over the page first (panel.js), then the
// time's menu, then the page; in capture, so it sees the panel before it's
// closed
document.addEventListener("keydown", (ev) => {
	if (ev.key !== "Escape" || !identifyOpen || panel || editing) return;
	if (menuOpen) {
		menuOpen = false;
		render();
	} else leave();
}, true);
