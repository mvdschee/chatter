// The layout check (docs/ui.md, plan step 2): opens the page in headless
// Chrome at a phone's width and a wide screen's, in each state worth seeing,
// and fails when a control leaves the screen, a control is covered by
// another layer, or two pieces of text overlap.
//
//   node crates/collector/ui-tests/layout.mjs [filter]
//
// `filter` keeps the scenes whose name contains it ("phone", "identify").
// SHOTS=dir saves a screenshot of each scene there. CHROME=path picks the
// browser. No collector or database: a small server here serves ui/ and
// answers /v1 with made-up data, made busy on purpose (four sensors, long
// names, close port scans), as tidy data would pass anything.
// No dependencies: Chrome is driven over the DevTools protocol, with node's
// own WebSocket.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

const UI = new URL("../ui/", import.meta.url).pathname;
const WIDTHS = [["phone", 390, 844], ["wide", 1440, 900]];

// --- made-up data, the same every run -----------------------------------

let seed = 7;
const rand = () => {
	seed = (seed + 0x6d2b79f5) | 0;
	let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
	t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
	return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const int = (n) => Math.floor(rand() * n);
const hex = (n) => Array.from({ length: n }, () => int(16).toString(16)).join("");
const pick = (xs) => xs[int(xs.length)];
// every other one locally administered: a phone's private address
const mac = (i) => [((int(64) << 2) | (i % 2 ? 2 : 0)).toString(16).padStart(2, "0"), ...Array.from({ length: 5 }, () => hex(2))].join(":");

const NOW = Math.floor(Date.now() / 1000);
const LATEST = NOW - 20;
const MiB = 1024 * 1024;
const since = () => Math.floor(Date.now() / 1000) - NOW;

const SENSORS = [
	["live", 20], ["late", 300], ["offline", 4000], ["live", 40],
].map(([, age], i) => ({
	name: i === 1 ? `sensor-${hex(4)}-${hex(4)}-${hex(4)}` : `sensor-${hex(4)}`,
	last_seen: NOW - age, seq: 1200 + i, uptime_s: 86_000 + i * 3000, channel: 1 + int(13), dropped: int(40), overflow: int(3),
	nets_known: [0], devices: 20 + int(20), reports: 1400, missing: int(10), restarts: int(3),
	beats: Array.from({ length: 1400 }, (_, k) => NOW - age - (1399 - k) * 60),
}));

const VENDORS = ["Example Devices Ltd.", "Acme Networks International Co., Ltd.", "Placeholder Systems", "Sample Electronics GmbH"];
const DEVICES = Array.from({ length: 40 }, (_, i) => {
	const m = mac(i);
	const named = i % 3 === 0;
	return {
		mac: m, first_seen: NOW - 86_000, last_seen: LATEST - int(600), rssi_max: i % 5 === 4 ? null : -30 - int(60),
		ip: i % 4 === 3 ? null : `10.${int(250)}.${int(250)}.${int(250)}`,
		hostname: named ? `host-${hex(6)}-${hex(6)}.lan` : null,
		vendor: i % 2 ? null : pick(VENDORS),
		name: i % 7 === 0 ? `Made-up device with a long name ${hex(4)}` : null,
		ports: i % 6 === 0 ? [{ port: 22, protocol: "tcp", service: "ssh", product: null }, { port: 8080, protocol: "tcp", service: "http-proxy", product: "Example httpd" }] : null,
		port_scanned_at: i % 6 === 0 ? NOW - 7200 : null,
		minutes: Array.from({ length: 5 }, () => [int(4 * MiB), int(MiB)]),
		usual: i % 2 ? null : { hours: Array.from({ length: 24 }, () => [8 + rand() * 4, 1]), all: [10, 1.2], learning: i % 4 === 0 },
	};
});

function overview() {
	const minutes = Array.from({ length: 5 }, (_, i) => LATEST - (4 - i) * 60);
	return {
		latest: LATEST, minutes, hours: minutes.map((t) => new Date(t * 1000).getHours()),
		score: { k: 0.6745, floor: 0.1, white: 1, red: 3.5 },
		// the sensors' ages stay put however long the run takes
		sensors: SENSORS.map((s) => ({ ...s, last_seen: s.last_seen + since() })), devices: DEVICES,
	};
}

// as the collector's Window: the smallest bucket that keeps it within 400
const BUCKETS = [60, 300, 900, 1800, 3600, 7200, 21_600, 86_400];
const SPANS = { "1h": 3600, "6h": 6 * 3600, "24h": 86_400, "7d": 7 * 86_400, "30d": 30 * 86_400 };

function timeline(q) {
	const [a, b] = q.has("from") ? [+q.get("from"), +q.get("to")] : [LATEST + 1 - SPANS[q.get("range") ?? "24h"], LATEST + 1];
	const bucket = BUCKETS.find((s) => (b - a) / s <= 400) ?? 86_400;
	const from = Math.floor(a / bucket) * bucket, to = Math.ceil(b / bucket) * bucket;
	const n = (to - from) / bucket, span = to - from;
	// a wandering level with spikes, some of them close together, up to GiB
	let level = 2 * MiB;
	const total = Array.from({ length: n }, (_, i) => {
		level = Math.max(50_000, level * (0.8 + rand() * 0.42));
		const spike = i % 37 === 5 || i % 37 === 7 ? 600 : 1 + rand() * 3;
		return [Math.round(level), Math.round(Math.min(1.5 * 1024 * MiB, level * spike))];
	});
	// port scans and identify sessions close together, wherever the range is
	const at = (f) => from + Math.round(span * f);
	return {
		from, to, bucket_s: bucket, total,
		hours: total.map((_, i) => (bucket <= 3600 ? new Date((from + i * bucket) * 1000).getHours() : null)),
		devices: DEVICES.slice(0, 12).map((d) => ({ mac: d.mac, buckets: total.map(([v]) => [Math.round(v * rand() * 0.1), Math.round(v * rand() * 0.05)]), usual: d.usual })),
		port_scans: [[0.3, 0.32, 0], [0.34, 0.36, 2], [0.8, 0.81, 1]].map(([s, f, alerts]) =>
			({ started_at: at(s), finished_at: at(f), hosts: 40, open_ports: 31, timed_out: 2, alerts })),
		changes: [{ at: at(0.36), mac: DEVICES[0].mac, ip: DEVICES[0].ip, hostname: DEVICES[0].hostname, vendor: null, kind: "new_port", alert: true, port: 8080, protocol: "tcp", service: "http-proxy", product: null, before: null, ports: 2 }],
		identify_sessions: [[0.31, 0.33], [0.6, 0.61], [0.62, 0.64]].map(([s, e], i) => ({ sensor: SENSORS[i % 2].name, started_at: at(s), ended_at: at(e) })),
	};
}

// What /v1/identify answers, set per scene: no session (the sensor step), one
// not posting yet (move) or one posting (find; back is its Stop).
let identify = "none";
function identifyView() {
	const s = SENSORS[1];
	const session = { sensor: s.name, left_s: 432, received_at: identify === "find" ? NOW + since() - 2 : null,
		heard: identify === "find" ? DEVICES.filter((d) => d.rssi_max != null).slice(0, 14).map((d, i) => ({ mac: d.mac, rssi_max: -28 - i * 4, frames: 40 - i })) : [] };
	// online: most devices, and a few the sensor never counted
	const extra = Array.from({ length: 8 }, (_, i) => ({ mac: mac(i + 100), ip: `10.0.${int(250)}.${int(250)}`, hostname: i % 2 ? `host-${hex(8)}.lan` : null, vendor: pick(VENDORS) }));
	return {
		sensors: identify === "none" ? [] : [session],
		online: [...DEVICES.filter((d) => d.ip).map((d) => ({ mac: d.mac, ip: d.ip, hostname: d.hostname, vendor: d.vendor })), ...extra],
		own_mac: DEVICES[2].mac,
	};
}

// --- the server -----------------------------------------------------------

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml" };

function serve(req, res) {
	const url = new URL(req.url, "http://x");
	const json = (body) => {
		res.writeHead(200, { "content-type": "application/json" });
		res.end(JSON.stringify(body));
	};
	if (url.pathname === "/v1/overview") return json(overview());
	if (url.pathname === "/v1/timeline") return json(timeline(url.searchParams));
	if (url.pathname === "/v1/plan") return json({ rooms: [] });
	if (url.pathname === "/v1/identify") return json(identifyView());
	if (/^\/v1\/sensors\/[^/]+\/identify$/.test(url.pathname) && req.method === "POST") {
		let body = "";
		req.on("data", (c) => (body += c));
		req.on("end", () => {
			if (JSON.parse(body).secs === 0) identify = "none";
			res.writeHead(204);
			res.end();
		});
		return;
	}
	const file = join(UI, url.pathname === "/" ? "index.html" : url.pathname);
	if (!file.startsWith(UI) || !existsSync(file)) {
		res.writeHead(404);
		return res.end();
	}
	res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
	res.end(readFileSync(file));
}

// --- Chrome over the DevTools protocol ----------------------------------

const CHROME = process.env.CHROME ?? [
	"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
	"/Applications/Chromium.app/Contents/MacOS/Chromium",
	"/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser",
].find(existsSync);

async function launch() {
	if (!CHROME) throw new Error("no Chrome found: set CHROME to its path");
	const profile = mkdtempSync(join(tmpdir(), "chatter-layout-"));
	const proc = spawn(CHROME, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
		"--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
	const wsUrl = await new Promise((ok, no) => {
		let err = "";
		proc.stderr.on("data", (c) => {
			err += c;
			const m = err.match(/DevTools listening on (ws:\/\/\S+)/);
			if (m) ok(m[1]);
		});
		proc.on("exit", () => no(new Error(`Chrome quit:\n${err}`)));
	});
	const ws = new WebSocket(wsUrl);
	await new Promise((ok, no) => ((ws.onopen = ok), (ws.onerror = no)));
	let id = 0;
	const pending = new Map();
	ws.onmessage = (m) => {
		const msg = JSON.parse(m.data);
		const p = pending.get(msg.id);
		if (!p) return;
		pending.delete(msg.id);
		if (msg.error) p.no(new Error(`${p.method}: ${msg.error.message}`));
		else p.ok(msg.result);
	};
	const send = (method, params = {}, sessionId) => new Promise((ok, no) => {
		pending.set(++id, { ok, no, method });
		ws.send(JSON.stringify({ id, method, params, sessionId }));
	});
	const close = async () => {
		ws.close();
		const gone = new Promise((ok) => proc.on("exit", ok));
		proc.kill();
		await gone;
		rmSync(profile, { recursive: true, force: true, maxRetries: 3 });
	};
	return { send, close };
}

// A tab at a size, with reduced motion so nothing plays or slides.
async function openTab(browser, width, height) {
	const { targetId } = await browser.send("Target.createTarget", { url: "about:blank" });
	const { sessionId } = await browser.send("Target.attachToTarget", { targetId, flatten: true });
	const send = (method, params) => browser.send(method, params, sessionId);
	await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: width < 600 ? 3 : 1, mobile: width < 600 });
	await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] });
	const evaluate = async (expression) => {
		const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
		if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
		return r.result.value;
	};
	const waitFor = async (expression, what) => {
		for (let t = 0; t < 100; t++) {
			// a page still loading answers with an error
			if (await evaluate(`!!(${expression})`).catch(() => false)) return;
			await new Promise((ok) => setTimeout(ok, 50));
		}
		throw new Error(`timed out waiting for ${what ?? expression}`);
	};
	return { send, evaluate, waitFor };
}

// --- what's checked, in the page --------------------------------------------

// Runs in the page. Ink is what's drawn: each line of text and each icon.
// Text drawn on a canvas isn't in the page: the timeline's labels come from
// timeline.js, the HUD's are checked apart (hudFaults).
// Fails: a control or ink past the left or right edge, the page scrolling
// sideways, a control whose middle is under something else, and two inks of
// the same layer overlapping by more than a sliver (a line's box is taller
// than its line height, so wrapped lines touch). Ink under another layer (the plan under the side
// panel) is covered, not overlapping; that layer covering a control is the fault.
// `vw`, `vh`: the screen's size. Not innerWidth: a phone widens the page to
// fit what runs off it, so nothing would ever be off the screen. `drawn`: the
// timeline's labels, boxes on its canvas (timeline.js `drawnLabels`).
function probe(vw, vh, drawn) {
	const LAYERS = "#side, #identify, #editor";
	const PLACES = [[".top", "top bar"], [".view-ctl", "camera"], [".tl-head", "timeline head"], ["#side", "side panel"], ["#identify", "identify"]];
	const place = (el) => PLACES.find(([s]) => el.closest(s))?.[1] ?? el.tagName.toLowerCase();
	const layer = (el) => el.closest(LAYERS) ?? document.body;
	// not in a closed <details>, hidden or see-through (the camera's hint)
	const shown = (el) => el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
	// A box cut to what its scrolling or clipping ancestors let through.
	// `cut`: the ancestor that hides part of it without a way to scroll to
	// it or an ellipsis to say so (the plan cutting off the top bar).
	const clip = (r, el) => {
		let { left, top, right, bottom } = r;
		let cut = null;
		for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
			const s = getComputedStyle(e);
			if (s.overflowX === "visible" && s.overflowY === "visible") continue;
			const c = e.getBoundingClientRect();
			const before = [left, top, right, bottom].join();
			if (s.overflowX !== "visible") [left, right] = [Math.max(left, c.left), Math.min(right, c.right)];
			if (s.overflowY !== "visible") [top, bottom] = [Math.max(top, c.top), Math.min(bottom, c.bottom)];
			const hides = [s.overflowX, s.overflowY].some((o) => o === "hidden" || o === "clip") && s.textOverflow !== "ellipsis";
			if (hides && !cut && [left, top, right, bottom].join() !== before) cut = e;
		}
		return right - left > 0.5 && bottom - top > 0.5 ? { left, top, right, bottom, cut } : null;
	};
	// What's over its middle, or another layer over a corner (2 px in): a
	// panel over a button's edge. Not a neighbour over a corner: a heading's
	// letters reach a few px past its box.
	const covered = (el, box) => {
		const xs = [box.left + 2, (box.left + box.right) / 2, box.right - 2];
		const ys = [box.top + 2, (box.top + box.bottom) / 2, box.bottom - 2];
		const points = [[xs[1], ys[1]], [xs[0], ys[0]], [xs[2], ys[0]], [xs[0], ys[2]], [xs[2], ys[2]]];
		for (const [i, [x, y]] of points.entries()) {
			if (x < 0 || x >= vw || y < 0 || y >= vh) continue;
			const hit = document.elementFromPoint(x, y);
			if (hit && !el.contains(hit) && !hit.contains(el) && (i === 0 || layer(hit) !== layer(el))) return hit;
		}
		return null;
	};
	const name = (el) => `"${(el.textContent.trim() || el.getAttribute("aria-label") || el.id || el.tagName).replace(/\s+/g, " ").slice(0, 32)}" (${place(el)})`;

	const ink = [];
	const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
	for (let n = walk.nextNode(); n; n = walk.nextNode()) {
		const el = n.parentElement;
		if (!n.data.trim() || el.closest("script, style, datalist, option") || !shown(el)) continue;
		const range = document.createRange();
		range.selectNodeContents(n);
		for (const r of range.getClientRects()) {
			const box = clip(r, el);
			if (box) ink.push({ el, node: n, box, what: `"${n.data.trim().replace(/\s+/g, " ").slice(0, 32)}" (${place(el)})` });
		}
	}
	for (const el of document.querySelectorAll("svg.icon")) {
		const box = shown(el) && clip(el.getBoundingClientRect(), el);
		if (box) ink.push({ el, node: el, box, what: `icon ${el.querySelector("use")?.getAttribute("href") ?? ""} (${place(el)})` });
	}

	const heat = document.getElementById("heat");
	if (shown(heat)) {
		const r = heat.getBoundingClientRect();
		for (const l of drawn) {
			const box = { left: r.left + l.left, top: r.top + l.top, right: r.left + l.right, bottom: r.top + l.bottom };
			ink.push({ el: heat, node: l, box, what: `"${l.text}" (timeline graph)` });
		}
	}

	const faults = [];
	if (document.documentElement.scrollWidth > vw + 1) faults.push(`the page scrolls sideways: ${document.documentElement.scrollWidth} px wide`);
	const off = (b) => b.left < -1 || b.right > vw + 1;
	const controls = [...document.querySelectorAll("button, input, select, textarea, summary, [role=separator]")]
		.map((el) => ({ el, box: shown(el) && clip(el.getBoundingClientRect(), el) }))
		.filter((c) => c.box);
	const cutBy = (box) => box.cut && `cut off by ${box.cut.className ? `.${box.cut.className.split(" ")[0]}` : box.cut.tagName.toLowerCase()}`;
	for (const c of controls) {
		if (cutBy(c.box)) faults.push(`${name(c.el)} is ${cutBy(c.box)}`);
		if (off(c.box)) faults.push(`${name(c.el)} runs off the screen: ${Math.round(c.box.left)} to ${Math.round(c.box.right)} px`);
		const hit = covered(c.el, c.box);
		if (hit) faults.push(`${name(c.el)} is under ${name(hit.closest("button, [id]") ?? hit)}`);
	}
	for (const i of ink) {
		if (i.el.closest("button")) continue;
		if (cutBy(i.box)) faults.push(`${i.what} is ${cutBy(i.box)}`);
		if (off(i.box)) faults.push(`${i.what} runs off the screen: ${Math.round(i.box.left)} to ${Math.round(i.box.right)} px`);
	}
	// ink under another layer is covered, and only its own layer can clash
	const top = ink.filter((i) => {
		const hit = covered(i.el, i.box);
		return !hit || layer(hit) === layer(i.el);
	});
	// a text's own pieces (a wrapped line, an ellipsis) aren't a clash
	for (let a = 0; a < top.length; a++) {
		for (let b = a + 1; b < top.length; b++) {
			if (top[a].node === top[b].node) continue;
			const [p, q] = [top[a].box, top[b].box];
			const w = Math.min(p.right, q.right) - Math.max(p.left, q.left);
			const h = Math.min(p.bottom, q.bottom) - Math.max(p.top, q.top);
			const sliver = Math.min(p.bottom - p.top, q.bottom - q.top) / 3;
			if (w > 1 && h > sliver && layer(top[a].el) === layer(top[b].el)) faults.push(`${top[a].what} overlaps ${top[b].what}`);
		}
	}
	return faults;
}

// Runs in the page. The selected device's callout, drawn on the plan's
// canvas (draw.js `drawnCallouts`), under a control, the clock or the open
// side panel. elementFromPoint can't see into a canvas, so these are boxes.
function calloutFaults(callouts) {
	const plan = document.getElementById("plan").getBoundingClientRect();
	const shown = (el) => el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
	// the panel as a whole, not each thing in it
	const over = [...document.querySelectorAll("button, input, select, textarea, summary, [role=separator], .clock, #side")]
		.filter((el) => shown(el) && (el.id === "side" || !el.closest("#side")));
	const name = (el) => `"${(el.getAttribute("aria-label") || el.textContent.trim() || el.id).replace(/\s+/g, " ").slice(0, 32)}"`;
	const faults = [];
	for (const c of callouts) {
		const a = { left: plan.left + c.left, top: plan.top + c.top, right: plan.left + c.right, bottom: plan.top + c.bottom };
		for (const el of over) {
			const r = el.getBoundingClientRect();
			if (a.left < r.right && r.left < a.right && a.top < r.bottom && r.top < a.bottom) faults.push(`the callout "${c.text}" is under ${el.id === "side" ? "the side panel" : name(el)}`);
		}
	}
	return faults;
}

// Runs in the page. The HUD's text, drawn on the plan's canvas (draw.js
// `hudBoxes`), under a control or the clock. Not under the side panel or
// the phone's tasks menu: a layer over the plan covers it, as the probe
// treats ink.
function hudFaults(boxes) {
	const plan = document.getElementById("plan").getBoundingClientRect();
	const shown = (el) => el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
	const over = [...document.querySelectorAll("button, input, select, textarea, summary, [role=separator], .clock")]
		.filter((el) => shown(el) && !el.closest("#side, #identify, #editor"))
		.filter((el) => !el.closest(".tasks") || getComputedStyle(el.closest(".tasks")).display === "contents");
	const name = (el) => `"${(el.getAttribute("aria-label") || el.textContent.trim() || el.id).replace(/\s+/g, " ").slice(0, 32)}"`;
	const faults = [];
	for (const b of boxes.filter((b) => b.text)) {
		const a = { left: plan.left + b.x, top: plan.top + b.y, right: plan.left + b.x + b.w, bottom: plan.top + b.y + b.h };
		if (a.left < plan.left - 1 || a.right > plan.right + 1) faults.push(`the HUD's "${b.text.slice(0, 32)}" runs off the plan`);
		for (const el of over) {
			const r = el.getBoundingClientRect();
			if (a.left < r.right && r.left < a.right && a.top < r.bottom && r.top < a.bottom) faults.push(`the HUD's "${b.text.slice(0, 32)}" is under ${name(el)}`);
		}
	}
	return faults;
}

// Runs in the page: whether the plan's canvas has nothing on it, a frame
// later. The HUD is drawn every frame, so blank means the loop stopped (a
// throw in it) or the canvas was cleared after the last frame.
const BLANK = `new Promise((ok) => requestAnimationFrame(() => {
	const c = document.getElementById("plan");
	ok(!c.getContext("2d").getImageData(0, 0, c.width, c.height).data.some((v, i) => i % 4 === 3 && v));
}))`;

// Runs in the page: selects each device in turn and checks its callout.
const EVERY_CALLOUT = `Promise.all([import("/data.js"), import("/panel.js"), import("/draw.js")]).then(async ([data, panel, draw]) => {
	const faults = new Set();
	for (const d of data.DEVICES) {
		panel.select(d);
		await new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)));
		for (const f of (${calloutFaults})(draw.drawnCallouts)) faults.add(f);
	}
	return [...faults];
})`;

// --- the scenes ---------------------------------------------------------------

// `act`: what to click, in order; `zoom`: then shift-drag across the middle
// of the graph; `ready`: what to wait for after; `every`: then select each
// device in turn and check its callout. The page opens live here
// (reduced motion doesn't play), so the other states of the timeline's head
// are scenes of their own.
const panel = (name, act, ready = "document.querySelector('#side.open')") => ({ name, act, ready });
const range = (r) => ({
	name: `range ${r}`, act: [`#ranges [data-range="${r}"]`],
	ready: `import("/data.js").then((m) => m.TL.to - m.TL.from >= ${SPANS[r]} && m.TL.to - m.TL.from < ${SPANS[r]} * 1.1)`,
});
const id = (name, state, act, ready) => ({ name: `identify: ${name}`, hash: "#identify", identify: state, act, ready: `document.querySelector('${ready}')` });
const SCENES = [
	{ name: "home" },
	{ name: "tasks menu", act: ["#tasks-btn"], ready: "document.querySelector('#tasks-btn[aria-expanded=true]')" },
	panel("devices", ["#devices-btn"]),
	panel("a device", ["#devices-btn", "#side .list button"], "document.querySelector('#side .name')"),
	{ ...panel("every device's callout", ["#devices-btn"]), every: true },
	panel("events", ["#events-btn"]),
	panel("sensors", ["#sensors-btn"]),
	panel("a sensor", ["#sensors-btn", "#side [data-sensor]"], "document.querySelector('#sensor-mode p')"),
	...["1h", "6h", "7d", "30d"].map(range),
	{ name: "a moment", act: ["#peak"], ready: "document.querySelector('#live[aria-pressed=false]')" },
	{ name: "playing", act: ["#play"], ready: "document.querySelector('#play').textContent === 'Pause'" },
	{ name: "zoomed in", zoom: true, ready: "document.querySelector('#zoom-out-tl:not([hidden])')" },
	id("sensor", "none", [], ".id-pick"),
	id("move", "move", [], "[data-act=find]"),
	id("what's here", "find", [], "#id-body .id-list"),
	id("find", "find", ["[data-tab=find]"], "#id-find:not([hidden])"),
	id("a device", "find", ["[data-tab=find]", "#id-body [data-pick]"], ".id-big"),
	id("put back", "find", ["[data-act=stop]"], "[data-act=close]"),
];
// loaded: the sensors' status, the ranges and the graph's peak filled in
const LOADED = "document.querySelector('#sensors-sub').textContent && document.querySelector('#ranges button') && document.querySelector('#peak').textContent";
const frames = "new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)))";

async function main() {
	const only = process.argv[2] ?? "";
	const shots = process.env.SHOTS;
	const server = createServer(serve);
	await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
	const base = `http://127.0.0.1:${server.address().port}/`;
	const browser = await launch();
	let failed = 0, ran = 0;
	try {
		for (const [size, width, height] of WIDTHS) {
			const tab = await openTab(browser, width, height);
			for (const scene of SCENES) {
				const title = `${size} ${width}px, ${scene.name}`;
				if (!title.includes(only)) continue;
				ran++;
				identify = scene.identify ?? "none";
				// a URL of its own, or one that differs only in its hash isn't loaded
				// again; and the old page is marked, to wait until it's gone
				await tab.evaluate("window.stale = true");
				await tab.send("Page.navigate", { url: `${base}?scene=${ran}${scene.hash ?? ""}` });
				await tab.waitFor(`!window.stale && ${scene.hash ? "document.querySelector('#id-main').textContent" : LOADED}`, "the page to load");
				for (const sel of scene.act ?? []) {
					await tab.waitFor(`document.querySelector('${sel}')`);
					await tab.evaluate(`document.querySelector('${sel}').click()`);
					await tab.evaluate(frames);
				}
				if (scene.zoom) {
					const r = await tab.evaluate("(({ left, top, width, height }) => ({ left, top, width, height }))(document.getElementById('heat').getBoundingClientRect())");
					const y = r.top + r.height / 2;
					const mouse = (type, x) => tab.send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1, modifiers: 8 });
					await mouse("mousePressed", r.left + r.width * 0.4);
					await mouse("mouseMoved", r.left + r.width * 0.6);
					await mouse("mouseReleased", r.left + r.width * 0.6);
				}
				if (scene.ready) await tab.waitFor(scene.ready);
				// let fetches that follow a click land, and the canvas draw
				await new Promise((ok) => setTimeout(ok, 300));
				await tab.evaluate(frames);
				const drawn = await tab.evaluate(`import("/timeline.js").then((m) => m.drawnLabels)`);
				const faults = await tab.evaluate(`(${probe})(${width}, ${height}, ${JSON.stringify(drawn)})`);
				faults.push(...await tab.evaluate(`import("/draw.js").then((m) => (${calloutFaults})(m.drawnCallouts))`));
				if (!scene.hash) faults.push(...await tab.evaluate(`import("/draw.js").then((m) => (${hudFaults})(m.hudBoxes(performance.now())))`));
				if (!scene.hash && await tab.evaluate(BLANK)) faults.push("the plan is blank: its frames stopped");
				if (scene.every) faults.push(...await tab.evaluate(EVERY_CALLOUT));
				if (shots) {
					const { data } = await tab.send("Page.captureScreenshot", { format: "png" });
					writeFileSync(join(shots, `${title.replace(/[^\w]+/g, "-")}.png`), Buffer.from(data, "base64"));
				}
				if (!faults.length) {
					console.log(`ok    ${title}`);
					continue;
				}
				failed++;
				console.log(`FAIL  ${title}`);
				for (const f of faults.slice(0, 12)) console.log(`        ${f}`);
				if (faults.length > 12) console.log(`        and ${faults.length - 12} more`);
			}
		}
	} finally {
		await browser.close();
		server.close();
	}
	console.log(`\n${ran - failed} of ${ran} scenes clean`);
	process.exit(failed ? 1 : 0);
}

await main();
