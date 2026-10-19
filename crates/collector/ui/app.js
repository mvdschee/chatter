"use strict";

// ---------------------------------------------------------------------------
// The floor plan. Drawn by hand here until the plan editor exists
// (docs/monitoring.md, plan step 3); it's an example, not anyone's house.
// ---------------------------------------------------------------------------

// Plan units; the view projects them into 3D.
const PLAN = { w: 1000, h: 600 };
const WALL_H = 56;

// Walls as polylines; gaps between them are doorways.
const WALLS = [
	[[40, 40], [960, 40], [960, 560], [40, 560], [40, 40]],
	[[400, 40], [400, 190]], [[400, 250], [400, 330]],
	[[40, 330], [140, 330]], [[200, 330], [520, 330]], [[580, 330], [700, 330]],
	[[700, 40], [700, 150]], [[700, 210], [700, 330]],
	[[700, 330], [960, 330]],
	[[520, 330], [520, 400]], [[520, 460], [520, 560]],
	[[300, 330], [300, 420]], [[300, 480], [300, 560]],
	[[800, 330], [800, 430]], [[800, 490], [800, 560]],
];

const ROOMS = [
	{ name: "Living room", at: [60, 70] },
	{ name: "Kitchen", at: [420, 70] },
	{ name: "Office", at: [720, 70] },
	{ name: "Bedroom", at: [60, 360] },
	{ name: "Hall", at: [320, 360] },
	{ name: "Bathroom", at: [540, 360] },
	{ name: "Utility", at: [820, 360] },
];

const ROUTER = { x: 410, y: 470, name: "Router" };
const SENSOR = { x: 455, y: 520, name: "Sensor" };

// ---------------------------------------------------------------------------
// The data, from the collector's /v1/overview
// ---------------------------------------------------------------------------

const MiB = 1024 * 1024;
const MINUTE_MS = 6000; // how long each replayed minute lasts on screen
const LOOP = 5; // minutes in the replay loop

// The day: 5-minute buckets of [tx, rx] bytes a minute, the last one holding
// the newest report.
const BUCKET_MIN = 5;
const BUCKETS = (24 * 60) / BUCKET_MIN;
const BUCKET_MS = BUCKET_MIN * 60_000;
let dayStart = Math.floor(Date.now() / BUCKET_MS) * BUCKET_MS - (BUCKETS - 1) * BUCKET_MS;
const bucketTime = (i) => new Date(dayStart + i * BUCKET_MS);

// The loop's first minute.
let firstMinute = new Date();

// Every device seen in the day. `minutes` are the loop's [tx, rx] bytes,
// `day` its buckets. `locationOnly` and `outlier` aren't filled in yet:
// devices the sensors can place but not count, and the outlier rules
// (docs/monitoring.md, plan step 7).
const DEVICES = [];

// The whole network's bytes a minute in bucket i, and its busiest device then.
const dayTotal = (i) => DEVICES.reduce((s, d) => s + d.day[i][0] + d.day[i][1], 0);
const busiestAt = (i) => DEVICES.reduce((a, d) => (!a || d.day[i][0] + d.day[i][1] > a.day[i][0] + a.day[i][1] ? d : a), null);

// What the HUD shows about the sensors.
const STATUS = { sensors: 0, channel: 0, dropped: 0 };

// The sensors, from the overview, each with `beatAt`: when the page saw a new
// report from it (for the heartbeat ring on the plan).
let SENSORS = [];

// The firmware reports once a minute. Live: a report within two and a half
// of those; late: within ten minutes; else silent.
const REPORT_PERIOD_S = 60;
function sensorState(s) {
	const age = Date.now() / 1000 - s.last_seen;
	return age <= 2.5 * REPORT_PERIOD_S ? "live" : age <= 600 ? "late" : "silent";
}
const STATE_COLOR = { live: "#7fe7f2", late: "#f2c14e", silent: "#ff3b3b" };

function fmtAgo(seconds) {
	const s = Math.max(0, Math.round(seconds));
	if (s < 90) return `${s} s ago`;
	if (s < 90 * 60) return `${Math.round(s / 60)} min ago`;
	if (s < 48 * 3600) return `${Math.round(s / 3600)} h ago`;
	return `${Math.round(s / 86400)} days ago`;
}

function fmtDuration(seconds) {
	const m = Math.floor(seconds / 60), h = Math.floor(m / 60), d = Math.floor(h / 24);
	if (d) return `${d} d ${h % 24} h`;
	if (h) return `${h} h ${m % 60} min`;
	return `${m} min`;
}

// The Timeline panel's events: `{ time, text, hot, device?, from?, to? }`.
// `hot`: an alert (a new device or port; later the outlier rules too);
// `device`: a click opens it; `from`/`to`: a time span. The port scans, and
// what they found changed.
const EVENTS = [];

// The day's port scans, as `{ from, to }` dates, marked on the day graph.
let PORT_SCANS = [];

// A small seeded random generator: the same scatter on every load.
let rngState = 7;
function rand() {
	rngState = (rngState * 1103515245 + 12345) & 0x7fffffff;
	return rngState / 0x7fffffff;
}

// Until devices can be placed by hand, each gets a spot on a grid over the
// plan, the same one on every load: spots shuffled once, handed out in MAC order.
const SPOTS = (() => {
	const spots = [];
	for (let x = 100; x <= 900; x += 114) {
		for (let y = 95; y <= 510; y += 83) {
			if (Math.hypot(x - ROUTER.x, y - ROUTER.y) > 70 && Math.hypot(x - SENSOR.x, y - SENSOR.y) > 50) spots.push([x, y]);
		}
	}
	let seed = 11;
	for (let i = spots.length - 1; i > 0; i--) {
		seed = (seed * 1103515245 + 12345) & 0x7fffffff;
		const j = seed % (i + 1);
		[spots[i], spots[j]] = [spots[j], spots[i]];
	}
	return spots;
})();

// A MAC with the "locally administered" bit set is made up (a phone's private
// address per network), so no vendor can be known.
const vendorless = (mac) => (parseInt(mac[1], 16) & 2 ? "private address (made-up MAC)" : "unknown");

function newDevice(mac) {
	return { id: mac, name: mac, short: mac.slice(9), room: "not placed yet", mac, vendor: "unknown", ports: [], dns: [],
		particles: [], spawn: [0, 0], shownAt: -1e9, screen: [0, 0] };
}

// A port scan's change as a Timeline event, in lan_scan's words.
function changeEvent(c) {
	const who = c.hostname?.split(".")[0] ?? c.vendor ?? c.mac;
	const label = `${who} (${c.ip ?? c.mac})`;
	const port = c.port == null ? "" : `${c.port}/${c.protocol}${c.service ? ` ${c.service}` : ""}${c.product ? `, ${c.product}` : ""}`;
	const text = {
		new_device: `New device: ${label}, ${c.ports} open port${c.ports === 1 ? "" : "s"}.`,
		new_port: `New port on ${label}: ${port}.`,
		new_rotating_port: `New rotating high port on ${label}: ${port}.`,
		closed_port: `Closed on ${label}: ${port}.`,
		changed_port: `Changed on ${label}: ${c.port}/${c.protocol} ${c.before} → ${c.product} (firmware update?).`,
	}[c.kind] ?? `${c.kind} on ${label}.`;
	return { time: new Date(c.at * 1000), text, hot: c.alert, device: c.mac };
}

// Merges an overview into DEVICES, keeping each device's particles.
function applyOverview(o) {
	dayStart = o.day_start * 1000;
	if (o.minutes.length) firstMinute = new Date(o.minutes[0] * 1000);
	const pad = (m) => [...Array(Math.max(0, LOOP - m.length)).fill([0, 0]), ...m.slice(-LOOP)];
	const seen = o.devices.filter((x) => x.last_seen * 1000 >= dayStart).sort((a, b) => a.mac.localeCompare(b.mac));
	const old = new Map(DEVICES.map((d) => [d.id, d]));
	DEVICES.length = 0;
	seen.forEach((x, i) => {
		const d = old.get(x.mac) ?? newDevice(x.mac);
		const [sx, sy] = SPOTS[i % SPOTS.length];
		// the name the host sweep found (without the local domain), else the MAC
		const host = x.hostname?.split(".")[0];
		Object.assign(d, {
			x: sx, y: sy, minutes: pad(x.minutes), day: x.day, rssi: x.rssi_max, firstSeen: x.first_seen,
			name: host ?? x.mac, short: host ?? x.mac.slice(9), vendor: x.vendor ?? vendorless(x.mac),
			ip: x.ip, hostname: x.hostname, ports: x.ports, portScannedAt: x.port_scanned_at,
		});
		DEVICES.push(d);
	});
	PORT_SCANS = o.port_scans.map((p) => ({ from: new Date(p.started_at * 1000), to: new Date(p.finished_at * 1000), ...p }));
	EVENTS.length = 0;
	for (const p of PORT_SCANS) {
		EVENTS.push({
			time: p.to, from: p.from, to: p.to, hot: false,
			text: `Port scan, ${hhmm(p.from)}–${hhmm(p.to)}: ${p.hosts} hosts, ${p.open_ports} open ports`
				+ (p.timed_out ? `, ${p.timed_out} timed out (their ports aren't known)` : "")
				+ (p.alerts ? `, ${p.alerts} alert${p.alerts > 1 ? "s" : ""}` : "") + ".",
		});
	}
	for (const c of o.changes) EVENTS.push(changeEvent(c));
	const before = new Map(SENSORS.map((x) => [x.name, x]));
	SENSORS = o.sensors.map((x) => {
		const old = before.get(x.name);
		// a new report since the last fetch: a heartbeat on the plan
		const beatAt = !old ? -1e9 : x.last_seen > old.last_seen ? performance.now() : old.beatAt;
		return { ...x, beatAt };
	});
	renderSensorPills();
	const newest = o.sensors.reduce((a, b) => (b.last_seen > a.last_seen ? b : a), o.sensors[0]);
	Object.assign(STATUS, {
		sensors: o.sensors.length,
		channel: newest?.channel ?? 0,
		dropped: o.sensors.reduce((s, x) => s + x.dropped, 0),
	});
	if (selected && !DEVICES.includes(selected)) closePanel();
	onData();
}

async function loadOverview() {
	try {
		const res = await fetch("/v1/overview");
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		applyOverview(await res.json());
	} catch (e) {
		console.error("overview:", e);
	}
}

// ---------------------------------------------------------------------------
// Traffic to colour: white is low, red is high, on a log scale.
// ---------------------------------------------------------------------------

const LOW = 1024;
const HIGH = 5 * MiB;

function heat(bytes) {
	if (bytes <= LOW) return 0;
	return Math.min(1, Math.log(bytes / LOW) / Math.log(HIGH / LOW));
}

const WHITE = [232, 244, 246];
const PALE = [255, 180, 160];
const RED = [255, 59, 59];

function heatColor(t, alpha = 1) {
	const [a, b, u] = t < 0.5 ? [WHITE, PALE, t / 0.5] : [PALE, RED, (t - 0.5) / 0.5];
	const c = a.map((v, i) => Math.round(v + (b[i] - v) * u));
	return `rgba(${c[0]},${c[1]},${c[2]},${alpha})`;
}

function fmtBytes(b) {
	if (b < 1024) return `${Math.round(b)} B`;
	if (b < MiB) return `${(b / 1024).toFixed(1)} KiB`;
	if (b < 1024 * MiB) return `${(b / MiB).toFixed(1)} MiB`;
	return `${(b / 1024 / MiB).toFixed(1)} GiB`;
}

// Every time on the page: day before month, 24 hours (17/10/2026, 13:29:47),
// whatever the browser's language.
const fmt = (options) => new Intl.DateTimeFormat("en-GB", { hourCycle: "h23", ...options });
const TIME = fmt({ hour: "2-digit", minute: "2-digit" });
const TIME_S = fmt({ hour: "2-digit", minute: "2-digit", second: "2-digit" });
const DATE_TIME = fmt({ day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
const hhmm = (date) => TIME.format(date);

// ---------------------------------------------------------------------------
// The 3D view: plan units in, screen pixels out
// ---------------------------------------------------------------------------

const canvas = document.getElementById("plan");
const ctx = canvas.getContext("2d");
const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const TILT = 0.88; // radians from looking straight down (about 50°)
const DIST = 1700; // camera distance: smaller is more perspective
let yaw = 0;
// The user's camera on top of the fit: zoom and pan (screen pixels) and a
// yaw target. Each eases towards its target; a drag sets both at once.
// Until the user touches the view, the yaw drifts slowly by itself.
const cam = { zoom: 1, panX: 0, panY: 0 };
const camT = { zoom: 1, panX: 0, panY: 0, yaw: 0 };
let touched = false;
let dpr = 1, W = 0, H = 0, fit = 1, cx = 0, cy = 0;

function view(x, y, z) {
	const X = x - PLAN.w / 2, Y = y - PLAN.h / 2;
	const c = Math.cos(yaw), s = Math.sin(yaw);
	const x1 = X * c - Y * s, y1 = X * s + Y * c;
	const depth = DIST - (y1 * Math.sin(TILT) + z * Math.cos(TILT));
	const k = DIST / depth;
	return [x1 * k, (y1 * Math.cos(TILT) - z * Math.sin(TILT)) * k, k];
}

function project(x, y, z = 0) {
	const [vx, vy, k] = view(x, y, z);
	const f = fit * cam.zoom;
	return [cx + cam.panX + vx * f, cy + cam.panY + vy * f, k];
}

function resize() {
	dpr = window.devicePixelRatio || 1;
	const r = canvas.getBoundingClientRect();
	W = r.width;
	H = r.height;
	canvas.width = Math.round(W * dpr);
	canvas.height = Math.round(H * dpr);
	// fit the plan, walls included, with room for the camera drift
	const saved = yaw;
	let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
	for (const a of [-0.14, 0, 0.14]) {
		yaw = a;
		for (const [x, y] of [[0, 0], [PLAN.w, 0], [PLAN.w, PLAN.h], [0, PLAN.h]]) {
			for (const z of [0, WALL_H]) {
				const [vx, vy] = view(x, y, z);
				minX = Math.min(minX, vx); maxX = Math.max(maxX, vx);
				minY = Math.min(minY, vy); maxY = Math.max(maxY, vy);
			}
		}
	}
	yaw = saved;
	// room at the sides for the HUD's readouts
	const mx = W > 900 ? 175 : W * 0.05;
	fit = Math.min((W - 2 * mx) / (maxX - minX), (H * 0.86) / (maxY - minY));
	cx = W / 2 - ((minX + maxX) / 2) * fit;
	cy = H / 2 - ((minY + maxY) / 2) * fit + H * 0.02;
	heatDirty = true;
}
window.addEventListener("resize", resize);

// Walls as point-cloud curtains: dots along each wall, stacked to wall height.
const wallDots = [];
for (const line of WALLS) {
	for (let i = 0; i < line.length - 1; i++) {
		const [x1, y1] = line[i], [x2, y2] = line[i + 1];
		const n = Math.max(1, Math.round(Math.hypot(x2 - x1, y2 - y1) / 6));
		for (let k = 0; k <= n; k++) {
			const x = x1 + ((x2 - x1) * k) / n, y = y1 + ((y2 - y1) * k) / n;
			for (let z = 0; z <= WALL_H; z += 7) {
				wallDots.push({ x, y, z, top: z + 7 > WALL_H, phase: rand() * Math.PI * 2 });
			}
		}
	}
}

// The floor: a dot grid with a little jitter, so it reads as scanned, not drawn.
const floorDots = [];
for (let x = 50; x < PLAN.w - 40; x += 16) {
	for (let y = 50; y < PLAN.h - 40; y += 16) {
		floorDots.push({ x: x + (rand() - 0.5) * 4, y: y + (rand() - 0.5) * 4, a: 0.25 + rand() * 0.5 });
	}
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

let loopStart = 0, ringStart = 0, lastFrame = 0, glitchUntil = 0;
let selected = null;
let lastPlayed = -1;

// "live" plays the loop of the last minutes; "history" shows the day's bucket
// at `cursor`, which is fractional while playing.
let mode = "live";
let cursor = BUCKETS - 1;
let playing = false;
const PLAY_RATE = 4; // buckets a second: the day in 72 s

// Traffic at this moment. Live: eases from the previous minute into the
// current one over the first second. History: between two buckets while
// playing. Only real minutes and buckets are used.
function current(d, now) {
	if (mode === "history") {
		const i = Math.floor(cursor), j = Math.min(BUCKETS - 1, i + 1), u = cursor - i;
		return [0, 1].map((k) => d.day[i][k] + (d.day[j][k] - d.day[i][k]) * u);
	}
	const pos = (now - loopStart) / MINUTE_MS;
	const i = Math.min(LOOP - 1, Math.floor(pos));
	const prev = d.minutes[(i + LOOP - 1) % LOOP];
	const cur = d.minutes[i];
	const u = Math.min(1, (pos - i) * (MINUTE_MS / 1000));
	return [0, 1].map((k) => prev[k] + (cur[k] - prev[k]) * u);
}

// The device's outlier minute is playing in the live loop.
function outlierNow(d, now) {
	if (mode !== "live" || !d.outlier) return false;
	return heat(d.minutes[Math.min(LOOP - 1, Math.floor((now - loopStart) / MINUTE_MS))][0]) > 0.9;
}

// The loop came round: fetch the data again, so a new minute joins if the
// sensor sent one.
function newMinute(now) {
	ringStart = now;
	loadOverview();
}

function step(now) {
	if (now - loopStart >= MINUTE_MS * LOOP) {
		loopStart += MINUTE_MS * LOOP;
		newMinute(now);
	}
	const played = Math.floor((now - loopStart) / MINUTE_MS);
	if (played !== lastPlayed) {
		lastPlayed = played;
		// the outlier's minute starts: the whole view tears for a moment
		if (DEVICES.some((d) => outlierNow(d, now))) glitchUntil = now + 700;
	}
	if (playing) {
		cursor += ((now - lastFrame) / 1000) * PLAY_RATE;
		if (cursor >= BUCKETS - 1) goLive();
	}
	moveCamera(now);
	draw(now);
	drawTimeline(now);
	lastFrame = now;
	requestAnimationFrame(step);
}

function moveCamera(now) {
	if (!touched) camT.yaw = reduced ? 0 : Math.sin(now / 21000) * 0.12;
	const a = reduced ? 1 : 1 - Math.exp(-((now - lastFrame) / 1000) * 12);
	yaw += (camT.yaw - yaw) * a;
	for (const k of ["zoom", "panX", "panY"]) cam[k] += (camT[k] - cam[k]) * a;
}

function takeControl() {
	if (!touched) camT.yaw = yaw;
	touched = true;
}

// Zoom by `f` around a screen point, so what's under it stays put.
function zoomAt(mx, my, f) {
	takeControl();
	const z = Math.max(0.6, Math.min(5, camT.zoom * f));
	const u = z / camT.zoom;
	camT.panX = mx - cx - (mx - cx - camT.panX) * u;
	camT.panY = my - cy - (my - cy - camT.panY) * u;
	camT.zoom = z;
}

function rotate(by) {
	takeControl();
	camT.yaw += by;
}

function resetView() {
	touched = false;
	Object.assign(camT, { zoom: 1, panX: 0, panY: 0 });
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

function draw(now) {
	const dt = Math.min(0.05, (now - lastFrame) / 1000);
	ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	ctx.clearRect(0, 0, W, H);

	drawFloor();
	drawFloorHeat(now);
	drawRing(now);
	drawRooms();
	drawWalls(now);

	ctx.globalCompositeOperation = "lighter";
	for (const d of DEVICES) drawFlow(d, now, dt);
	ctx.globalCompositeOperation = "source-over";

	drawStation(ROUTER, "#7fe7f2", 9);
	drawSensor(now);
	for (const d of DEVICES) drawNode(d, now);
	drawCallouts(now);
	drawHud(now);
	if (!reduced && now < glitchUntil) glitch(now);
}

function drawFloor() {
	for (const p of floorDots) {
		const [x, y, k] = project(p.x, p.y);
		ctx.fillStyle = `rgba(60,110,124,${(p.a * 0.5 * k).toFixed(3)})`;
		const s = 1.2 * k * Math.sqrt(cam.zoom);
		ctx.fillRect(x, y, s, s);
	}
}

function drawRooms() {
	ctx.font = `600 ${11 * fit * Math.min(cam.zoom, 1.6)}px "Avenir Next Condensed", "Arial Narrow", sans-serif`;
	ctx.fillStyle = "rgba(92,122,134,0.85)";
	for (const r of ROOMS) {
		const [x, y] = project(r.at[0], r.at[1]);
		ctx.fillText(r.name.toUpperCase().split("").join(" "), x, y);
	}
}

function drawWalls(now) {
	for (const d of wallDots) {
		const [x, y, k] = project(d.x, d.y, d.z);
		const tw = reduced ? 0.8 : 0.55 + 0.45 * Math.sin(now / 800 + d.phase);
		const a = d.top ? 0.95 : (0.18 + 0.5 * (1 - d.z / WALL_H)) * tw;
		ctx.fillStyle = d.top ? `rgba(200,250,255,${a.toFixed(3)})` : `rgba(127,231,242,${a.toFixed(3)})`;
		const s = (d.top ? 1.9 : 1.4) * k * Math.sqrt(cam.zoom);
		ctx.fillRect(x - s / 2, y - s / 2, s, s);
	}
}

// Clip to the house's outline, so glows and the ring stay on the floor.
function clipToFloor() {
	ctx.beginPath();
	WALLS[0].forEach(([x, y], i) => {
		const [px, py] = project(x, y);
		i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
	});
	ctx.clip();
}

// A glow on the floor under each device: where the traffic is, at a glance.
function drawFloorHeat(now) {
	ctx.save();
	clipToFloor();
	ctx.globalCompositeOperation = "lighter";
	for (const d of DEVICES) {
		if (d.locationOnly) continue;
		const [tx, rx] = current(d, now);
		const t = heat(tx + rx);
		if (t < 0.05) continue;
		const [x, y, k] = project(d.x, d.y);
		const r = (30 + 80 * t) * fit * cam.zoom * k;
		const g = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
		g.addColorStop(0, heatColor(t, 0.22 * t));
		g.addColorStop(1, heatColor(t, 0));
		ctx.save();
		ctx.translate(x, y);
		ctx.scale(1, Math.cos(TILT));
		ctx.fillStyle = g;
		ctx.beginPath();
		ctx.arc(0, 0, r, 0, Math.PI * 2);
		ctx.fill();
		ctx.restore();
	}
	ctx.restore();
}

// A ring from the router when a new minute arrives.
function drawRing(now) {
	if (reduced || mode !== "live") return;
	const t = (now - ringStart) / 1800;
	ctx.save();
	clipToFloor();
	if (t < 1) {
		ctx.strokeStyle = `rgba(127,231,242,${(0.4 * (1 - t)).toFixed(3)})`;
		ctx.lineWidth = 1;
		ctx.beginPath();
		for (let i = 0; i <= 64; i++) {
			const ang = (i / 64) * Math.PI * 2;
			const [x, y] = project(ROUTER.x + Math.cos(ang) * t * 900, ROUTER.y + Math.sin(ang) * t * 900);
			i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
		}
		ctx.stroke();
	}
	ctx.restore();
}

// A point on the arc from a to b, raised in the middle.
function arcPoint(a, b, lift, u) {
	const x = a[0] + (b[0] - a[0]) * u;
	const y = a[1] + (b[1] - a[1]) * u;
	const z = 4 * lift * u * (1 - u);
	return project(x, y, z);
}

function path(pts) {
	ctx.beginPath();
	pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
}

// Packets ride the arcs as short streaks, all at one steady pace, in bursts:
// a train of a few, then a pause. Busier directions send trains more often,
// with more and longer packets, and glow hotter. One arc per device: sent
// packets run up it to the router, received packets run down it.
const FLOW_PACE = 38; // plan units a second: a long arc takes about 15 s

function drawFlow(d, now, dt) {
	if (d.locationOnly) return;
	const [tx, rx] = current(d, now);
	const len = Math.hypot(ROUTER.x - d.x, ROUTER.y - d.y);
	const at = (u) => arcPoint([d.x, d.y], [ROUTER.x, ROUTER.y], 25 + len * 0.19, u);
	const arc = Array.from({ length: 33 }, (_, i) => at(i / 32));
	const all = heat(tx + rx);

	// the channel, faint, with a slow drift each way
	ctx.lineCap = "butt";
	ctx.strokeStyle = heatColor(all, reduced ? 0.3 + 0.5 * all : 0.05 + 0.1 * all);
	ctx.lineWidth = reduced ? 1 + 3 * all : 1;
	path(arc);
	ctx.stroke();
	if (reduced) return;
	[tx, rx].forEach((bytes, k) => {
		const t = heat(bytes);
		ctx.setLineDash(k ? [1, 13] : [1, 9]);
		ctx.lineDashOffset = (k ? 1 : -1) * (now / 1000) * 8;
		ctx.strokeStyle = heatColor(t, 0.08 + 0.2 * t);
		path(arc);
		ctx.stroke();
	});
	ctx.setLineDash([]);

	[tx, rx].forEach((bytes, k) => {
		const t = heat(bytes);
		d.spawn[k] += dt * (0.06 + 0.6 * t * t);
		while (d.spawn[k] >= 1) {
			d.spawn[k] -= 1;
			const n = 1 + Math.floor(rand() * (1 + 5 * t));
			let s = 0;
			for (let j = 0; j < n; j++) {
				const size = 5 + rand() * (6 + 16 * t);
				d.particles.push({ lane: k, s: -s, size, t });
				s += size + 5;
			}
		}
	});
	ctx.lineCap = "round";
	for (const p of d.particles) {
		p.s += FLOW_PACE * dt;
		const head = Math.min(1, p.s / len), tail = Math.max(0, (p.s - p.size) / len);
		if (head <= tail) continue;
		// distance runs from the device for sent packets, from the router for received
		const u = (v) => (p.lane ? 1 - v : v);
		const seg = Array.from({ length: 5 }, (_, i) => at(u(tail + ((head - tail) * i) / 4)));
		const [hx, hy, kk] = seg[4];
		const g = ctx.createLinearGradient(seg[0][0], seg[0][1], hx, hy);
		g.addColorStop(0, heatColor(p.t, 0));
		g.addColorStop(1, heatColor(p.t, 0.95));
		ctx.strokeStyle = g;
		// a soft glow, then the core
		ctx.globalAlpha = 0.25;
		ctx.lineWidth = 3.4 * kk;
		path(seg);
		ctx.stroke();
		ctx.globalAlpha = 1;
		ctx.lineWidth = 1.3 * kk;
		path(seg);
		ctx.stroke();
	}
	ctx.lineCap = "butt";
	d.particles = d.particles.filter((p) => (p.s - p.size) / len < 1);
}

// The sensor on the plan, in its state's colour; a ring goes out when a new
// report has come in.
function drawSensor(now) {
	const s = SENSORS[0];
	if (!s) return drawStation(SENSOR, "#5c7a86", 6);
	const color = STATE_COLOR[sensorState(s)];
	const [x, yTop] = drawStation({ ...SENSOR, name: s.name }, color, 6);
	SENSOR.screen = [x, yTop];
	const t = (now - s.beatAt) / 1400;
	if (t >= 0 && t < 1 && !reduced) {
		ctx.strokeStyle = `rgba(127,231,242,${(0.8 * (1 - t)).toFixed(3)})`;
		ctx.lineWidth = 1.2;
		ctx.beginPath();
		ctx.arc(x, yTop, 8 + 26 * t, 0, Math.PI * 2);
		ctx.stroke();
	}
}

function drawStation(s, color, size) {
	const [x, y] = project(s.x, s.y);
	const [, yTop] = project(s.x, s.y, 26);
	ctx.strokeStyle = color;
	ctx.lineWidth = 1;
	ctx.beginPath();
	ctx.moveTo(x, y);
	ctx.lineTo(x, yTop);
	ctx.stroke();
	ctx.fillStyle = "rgba(127,231,242,0.15)";
	ctx.fillRect(x - size, yTop - size, size * 2, size * 2);
	ctx.strokeRect(x - size, yTop - size, size * 2, size * 2);
	label(s.name.toUpperCase(), x + size + 6, yTop + 4, color, "display");
	return [x, yTop];
}

function drawNode(d, now) {
	const lift = 14;
	let [x, y, k] = project(d.x, d.y, lift);
	const [fx, fy] = project(d.x, d.y, 0);
	d.screen = [x, y];
	const [tx, rx] = current(d, now);
	const t = heat(tx + rx);
	const glitching = outlierNow(d, now);

	// a stem down to the floor, and its footprint
	ctx.strokeStyle = "rgba(127,231,242,0.25)";
	ctx.lineWidth = 1;
	ctx.beginPath();
	ctx.moveTo(fx, fy);
	ctx.lineTo(x, y);
	ctx.stroke();
	ctx.strokeStyle = "rgba(127,231,242,0.18)";
	ctx.beginPath();
	ctx.ellipse(fx, fy, 9 * k, 9 * k * Math.cos(TILT), 0, 0, Math.PI * 2);
	ctx.stroke();

	if (glitching && !reduced && Math.random() < 0.4) {
		x += (Math.random() - 0.5) * 8;
		ctx.fillStyle = "rgba(127,231,242,0.6)";
		ctx.fillRect(x - 16 + Math.random() * 6, y - 2, 30, 2);
		ctx.fillStyle = "rgba(255,59,59,0.7)";
		ctx.fillRect(x - 10 + Math.random() * 6, y + 3, 22, 1);
	}

	const color = glitching ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : heatColor(t);
	const pulse = reduced || d.locationOnly ? 0 : (Math.sin(now / (500 - 300 * t)) + 1) * (2 + 6 * t);

	if (!d.locationOnly) {
		ctx.fillStyle = glitching ? "rgba(255,59,59,0.2)" : heatColor(t, 0.12);
		ctx.beginPath();
		ctx.arc(x, y, (10 + pulse) * k, 0, Math.PI * 2);
		ctx.fill();
	}

	ctx.lineWidth = 1.4;
	ctx.strokeStyle = color;
	ctx.fillStyle = color;
	ctx.beginPath();
	ctx.arc(x, y, 5.5 * k, 0, Math.PI * 2);
	if (!d.locationOnly) ctx.fill();
	ctx.stroke();

	if (selected === d) brackets(x, y, 17 * k, "#7fe7f2");

	const sub = d.locationOnly ? "location only" : `${fmtBytes(tx + rx)}/min`;
	label(d.short ?? d.name, x + 12, y - 3, glitching ? "#ff3b3b" : "rgba(232,244,246,0.8)");
	label(sub, x + 12, y + 10, "#5c7a86", "mono");
}

function brackets(x, y, s, color) {
	ctx.strokeStyle = color;
	ctx.lineWidth = 1;
	const a = s * 0.4;
	for (const [sx, sy] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
		ctx.beginPath();
		ctx.moveTo(x + sx * s, y + sy * (s - a));
		ctx.lineTo(x + sx * s, y + sy * s);
		ctx.lineTo(x + sx * (s - a), y + sy * s);
		ctx.stroke();
	}
}

// A floating "profiler" box next to the selected device, typed in.
function drawCallouts(now) {
	const show = selected ? [selected] : [];
	for (const d of DEVICES) if (!show.includes(d)) d.shownAt = -1e9;

	const bw = 248, bh = 72, gap = 8;
	// the HUD's readouts, the buttons, the view controls and the open panel are taken
	const taken = [{ x: 0, y: 0, w: 290, h: 235 }];
	taken.push(panel ? { x: W - 370, y: 0, w: 370, h: H } : { x: W - 320, y: 0, w: 320, h: 64 });
	taken.push({ x: W - 520, y: H - 64, w: 520, h: 64 });
	const overlaps = (a) => taken.some((b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h);

	show.forEach((d) => {
		if (d.shownAt < 0) d.shownAt = now;
		const [tx, rx] = current(d, now);
		const lines = [
			d.name.toUpperCase(),
			`MAC   ${d.mac}`,
			`TX    ${fmtBytes(tx).padStart(10)}  RX ${fmtBytes(rx).padStart(10)}`,
			`RSSI  ${d.rssi ?? "-"} dBm   PORTS ${d.ports ? d.ports.length : "-"}`,
		];
		const typed = reduced ? Infinity : Math.floor((now - d.shownAt) / 14);
		const [nx, ny] = d.screen;
		// try right of the node, then left; slide down until the box is free
		let box = null, right = true;
		for (const side of [true, false]) {
			const cand = { x: side ? nx + 46 : nx - 46 - bw, y: Math.max(14, ny - 92), w: bw, h: bh };
			for (let tries = 0; tries < 12 && overlaps(cand); tries++) cand.y += bh / 3 + gap;
			if (!overlaps(cand) && cand.x > 8 && cand.x + bw < W - 8 && cand.y + bh < H - 8) {
				[box, right] = [cand, side];
				break;
			}
		}
		if (!box) return;
		taken.push({ x: box.x - gap, y: box.y - gap, w: bw + 2 * gap, h: bh + 2 * gap });
		const bx = box.x, by = box.y;
		const hot = outlierNow(d, now);
		const color = hot ? "255,59,59" : "127,231,242";

		ctx.strokeStyle = `rgba(${color},0.5)`;
		ctx.lineWidth = 1;
		ctx.beginPath();
		ctx.moveTo(nx, ny);
		ctx.lineTo(right ? bx : bx + bw, by + bh / 2);
		ctx.stroke();
		ctx.fillStyle = "rgba(4,10,14,0.82)";
		ctx.fillRect(bx, by, bw, bh);
		ctx.strokeStyle = `rgba(${color},0.35)`;
		ctx.strokeRect(bx + 0.5, by + 0.5, bw, bh);
		ctx.fillStyle = `rgba(${color},0.9)`;
		ctx.fillRect(bx, by, 3, bh);

		let left = typed;
		lines.forEach((text, j) => {
			const shown = text.slice(0, Math.max(0, left));
			left -= text.length;
			ctx.font = j === 0 ? `600 12px "Avenir Next Condensed", "Arial Narrow", sans-serif` : "10.5px ui-monospace, Menlo, monospace";
			ctx.fillStyle = j === 0 ? `rgb(${color})` : "rgba(232,244,246,0.85)";
			ctx.fillText(shown, bx + 12, by + 18 + j * 15);
		});
	});
}

// The HUD around the plan: corner brackets, readouts, channel occupancy.
function drawHud(now) {
	const m = 16;
	ctx.strokeStyle = "rgba(127,231,242,0.5)";
	ctx.lineWidth = 1;
	for (const [x, y, sx, sy] of [[m, m, 1, 1], [W - m, m, -1, 1], [W - m, H - m, -1, -1], [m, H - m, 1, -1]]) {
		ctx.beginPath();
		ctx.moveTo(x, y + sy * 22);
		ctx.lineTo(x, y);
		ctx.lineTo(x + sx * 22, y);
		ctx.stroke();
	}

	const total = DEVICES.reduce((s, d) => { const [a, b] = current(d, now); return s + a + b; }, 0);
	const fps = Math.round(total / 60 / 900) + 140;
	const rows = [
		["SNIFFER", `CH ${STATUS.channel} · 2.4 GHZ · 20 MHZ`],
		["FRAMES/S", String(fps)],
		["THROUGHPUT", `${fmtBytes(total / 60)}/s`],
		["SENSORS", String(STATUS.sensors)],
		["DEVICES", String(DEVICES.length)],
		["DROPPED", String(STATUS.dropped)],
	];
	ctx.font = "10px ui-monospace, Menlo, monospace";
	rows.forEach(([k, v], i) => {
		ctx.fillStyle = "rgba(92,122,134,0.9)";
		ctx.fillText(k, m + 12, m + 26 + i * 15);
		ctx.fillStyle = "rgba(232,244,246,0.9)";
		ctx.fillText(v, m + 92, m + 26 + i * 15);
	});

	// channel occupancy, 1 to 13: ours lit
	const bx = m + 12, by = m + 26 + rows.length * 15 + 14;
	ctx.fillStyle = "rgba(92,122,134,0.9)";
	ctx.fillText("2.4 GHZ OCCUPANCY", bx, by - 6);
	for (let ch = 1; ch <= 13; ch++) {
		const base = ch === STATUS.channel ? 0.55 : ch === 6 || ch === 1 ? 0.3 : 0.08;
		const v = Math.min(1, base + (reduced ? 0 : 0.08 * Math.sin(now / 300 + ch * 1.7)));
		const h = 4 + v * 26;
		ctx.fillStyle = ch === STATUS.channel ? "rgba(127,231,242,0.9)" : "rgba(127,231,242,0.25)";
		ctx.fillRect(bx + (ch - 1) * 9, by + 30 - h, 6, h);
	}

	ctx.fillStyle = "rgba(92,122,134,0.8)";
	ctx.font = "10px ui-monospace, Menlo, monospace";
	ctx.fillText(`VIEW  YAW ${(yaw * 57.3).toFixed(1).padStart(6)}°  TILT ${(TILT * 57.3).toFixed(0)}°  ZOOM ${cam.zoom.toFixed(1)}×`, m + 12, H - m - 8);
}

// The view tears: horizontal strips shift, with a red/cyan split.
function glitch(now) {
	const strips = 7;
	for (let i = 0; i < strips; i++) {
		const y = Math.random() * H, h = 4 + Math.random() * 26;
		const dx = (Math.random() - 0.5) * 40;
		ctx.drawImage(canvas, 0, y * dpr, W * dpr, h * dpr, dx, y, W, h);
	}
	ctx.globalCompositeOperation = "lighter";
	ctx.fillStyle = "rgba(255,59,59,0.08)";
	ctx.fillRect(0, 0, W, H);
	ctx.globalCompositeOperation = "source-over";
	for (let i = 0; i < 3; i++) {
		ctx.fillStyle = i % 2 ? "rgba(127,231,242,0.25)" : "rgba(255,59,59,0.3)";
		ctx.fillRect(0, Math.random() * H, W, 1 + Math.random() * 2);
	}
}

function label(text, x, y, color, face = "body") {
	ctx.font = face === "mono"
		? "10px ui-monospace, Menlo, monospace"
		: face === "display"
			? `600 11px "Avenir Next Condensed", "Arial Narrow", sans-serif`
			: "500 12px system-ui, sans-serif";
	ctx.fillStyle = color;
	ctx.fillText(text, x, y);
}

// ---------------------------------------------------------------------------
// Heatmap, side panel
// ---------------------------------------------------------------------------

function onData() {
	heatDirty = true;
	renderEventsBtn();
	if (panel === "list") renderList();
	if (panel === "events") renderEvents();
	if (panel === "sensor") openSensor(openSensorName);
}

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
let heatDirty = true, hover = null, dragging = false, peak = 0;
let graphMax = 1; // the y axis's top, bytes a minute

function heatGeom() {
	const r = heatEl.getBoundingClientRect();
	return { w: r.width, h: r.height, cw: (r.width - GUTTER) / BUCKETS, ph: r.height - TOP - AXIS };
}

const colX = (g, i) => GUTTER + i * g.cw;
const valY = (g, v) => TOP + g.ph * (1 - v / graphMax);

function dayLabel(date) {
	const today = new Date().toDateString() === date.toDateString();
	return `${today ? "today" : "yesterday"} ${hhmm(date)}`;
}

// A round amount at or above `v` bytes: 1, 2 or 5 times a power of ten, in
// the unit fmtBytes shows it in, so the axis reads 20 MiB, not 19.1.
function niceCeil(v) {
	const unit = v < 1024 ? 1 : v < MiB ? 1024 : v < 1024 * MiB ? MiB : 1024 * MiB;
	const x = Math.max(1, v / unit);
	const p = 10 ** Math.floor(Math.log10(x));
	return [1, 2, 5, 10].map((m) => m * p).find((n) => n >= x) * unit;
}

// The day's highest buckets, at least `apart` buckets from each other.
function hotspots(totals, n, apart) {
	const picked = [];
	for (const i of totals.map((_, i) => i).sort((a, b) => totals[b] - totals[a])) {
		if (totals[i] <= 0 || picked.length === n) break;
		if (picked.every((j) => Math.abs(i - j) >= apart)) picked.push(i);
	}
	return picked;
}

// The day as one line: the whole network's traffic per bucket, the area under
// it coloured by how close it comes to the day's peak. Drawn once per change
// into `heatCache`; each frame adds the playhead and the hover readout.
function renderHeat() {
	const g = heatGeom();
	heatCache.width = Math.round(g.w * dpr);
	heatCache.height = Math.round(g.h * dpr);
	const c = heatCache.getContext("2d");
	c.setTransform(dpr, 0, 0, dpr, 0, 0);
	c.clearRect(0, 0, g.w, g.h);

	const totals = Array.from({ length: BUCKETS }, (_, i) => dayTotal(i));
	const best = Math.max(0, ...totals);
	peak = totals.indexOf(best);
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

	// the port scans: a band over their span, labelled
	c.font = "10px ui-monospace, Menlo, monospace";
	for (const p of PORT_SCANS) {
		const from = Math.max(0, (p.from - dayStart) / BUCKET_MS), to = Math.min(BUCKETS, (p.to - dayStart) / BUCKET_MS);
		if (to <= 0 || from >= BUCKETS) continue;
		const x1 = colX(g, from), x2 = Math.max(x1 + 2, colX(g, to));
		c.fillStyle = "rgba(127,231,242,0.07)";
		c.fillRect(x1, TOP, x2 - x1, g.ph);
		const color = p.alerts ? "rgba(255,59,59,0.9)" : "rgba(127,231,242,0.55)";
		c.fillStyle = color;
		c.fillRect(x1, TOP - 2, x2 - x1, 1);
		const text = p.alerts ? `port scan · ${p.alerts} alert${p.alerts > 1 ? "s" : ""}` : "port scan";
		c.fillText(text, Math.min(x1, g.w - c.measureText(text).width), TOP - 5);
	}

	// the area, a column per bucket, and the line on top
	if (best > 0) {
		for (let i = 0; i < BUCKETS; i++) {
			const y = valY(g, totals[i]);
			c.fillStyle = heatColor(totals[i] / best, 0.18 + 0.5 * (totals[i] / best));
			c.fillRect(colX(g, i), y, Math.max(1, g.cw - 0.4), TOP + g.ph - y);
		}
		c.strokeStyle = "rgba(232,244,246,0.85)";
		c.lineWidth = 1.2;
		c.beginPath();
		totals.forEach((v, i) => (i ? c.lineTo : c.moveTo).call(c, colX(g, i + 0.5), valY(g, v)));
		c.stroke();
	}

	// hours along the bottom
	for (let i = 0; i < BUCKETS; i++) {
		const t = bucketTime(i);
		if (t.getMinutes() !== 0) continue;
		const x = colX(g, i);
		c.fillStyle = "rgba(127,231,242,0.18)";
		c.fillRect(x, g.h - AXIS, 1, t.getHours() % 3 ? 3 : 6);
		if (t.getHours() % 3 === 0 && x > GUTTER + 12 && x < g.w - 30) {
			c.fillStyle = "rgba(92,122,134,0.9)";
			c.fillText(hhmm(t), x - 15, g.h - 2);
		}
	}

	// the hotspots: a dot on the line, with when and how much
	for (const i of hotspots(totals, 3, 24)) {
		const x = colX(g, i + 0.5), y = valY(g, totals[i]);
		c.fillStyle = heatColor(totals[i] / best);
		c.beginPath();
		c.arc(x, y, 2.5, 0, Math.PI * 2);
		c.fill();
		const text = `${hhmm(bucketTime(i))} · ${fmtBytes(totals[i])}/min`;
		const w = c.measureText(text).width;
		c.fillStyle = "rgba(232,244,246,0.9)";
		c.fillText(text, Math.max(GUTTER, Math.min(x - w / 2, g.w - w)), Math.max(10, y - 7));
	}

	// a mark above the line: the outlier
	if (DEVICES.some((d) => d.outlier && d.minutes.some((m) => heat(m[0]) > 0.9))) {
		c.fillStyle = "rgba(255,59,59,0.9)";
		c.fillRect(colX(g, BUCKETS - 1), 4, 2, 6);
	}

	const top = busiestAt(peak);
	peakBtn.textContent = best > 0 ? `peak ${dayLabel(bucketTime(peak))} · ${fmtBytes(best)}/min · most: ${top.short}` : "";
	heatDirty = false;
}

function drawTimeline(now) {
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

	// hover: a line, and the time and amount under the pointer
	if (hover !== null && !dragging) {
		const x = colX(g, hover + 0.5);
		hctx.fillStyle = "rgba(232,244,246,0.25)";
		hctx.fillRect(x, TOP, 1, g.ph);
		const text = `${hhmm(bucketTime(hover))} · ${fmtBytes(dayTotal(hover))}/min`;
		const w = hctx.measureText(text).width;
		hctx.fillStyle = "rgba(4,10,14,0.85)";
		hctx.fillRect(Math.min(x + 6, g.w - w - 8) - 3, TOP + 2, w + 6, 14);
		hctx.fillStyle = "#e8f4f6";
		hctx.fillText(text, Math.min(x + 6, g.w - w - 8), TOP + 12);
	}

	// the playhead: a line with a marker at the top
	const x = colX(g, cursor + 0.5);
	const live = mode === "live";
	const glow = live && !reduced ? 0.6 + 0.4 * Math.sin(now / 300) : 1;
	hctx.fillStyle = live ? `rgba(127,231,242,${glow.toFixed(2)})` : "#e8f4f6";
	hctx.fillRect(x - 0.5, TOP - 4, 1.5, g.ph + 4);
	hctx.beginPath();
	hctx.moveTo(x - 4, TOP - 6);
	hctx.lineTo(x + 4.5, TOP - 6);
	hctx.lineTo(x, TOP - 1);
	hctx.fill();

	if (live) {
		const left = Math.ceil((MINUTE_MS * LOOP - (now - loopStart)) / 1000);
		atEl.innerHTML = `<b>live</b> · replaying the last ${LOOP} minutes · next minute in ${left} s (played ${60_000 / MINUTE_MS}× faster)`;
	} else {
		const i = Math.floor(cursor);
		const d = busiestAt(i);
		atEl.innerHTML = `<b>${dayLabel(bucketTime(i))}</b> · ${fmtBytes(dayTotal(i))}/min`
			+ (d ? ` · most: ${d.short}, ${fmtBytes(d.day[i][0] + d.day[i][1])}/min` : "");
	}
}

function renderControls() {
	playBtn.textContent = playing ? "Pause" : "Play";
	liveBtn.setAttribute("aria-pressed", String(mode === "live"));
}

function seek(i) {
	mode = "history";
	cursor = Math.max(0, Math.min(BUCKETS - 1, i));
	renderControls();
}

function goLive() {
	mode = "live";
	playing = false;
	cursor = BUCKETS - 1;
	renderControls();
}

function togglePlay() {
	if (!playing && (mode === "live" || cursor >= BUCKETS - 2)) seek(0);
	playing = !playing;
	renderControls();
}

function bucketAt(ev) {
	const r = heatEl.getBoundingClientRect();
	return Math.max(0, Math.min(BUCKETS - 1, Math.floor((ev.clientX - r.left - GUTTER) / ((r.width - GUTTER) / BUCKETS))));
}

heatEl.addEventListener("pointerdown", (ev) => {
	dragging = true;
	playing = false;
	heatEl.setPointerCapture(ev.pointerId);
	seek(bucketAt(ev));
});
heatEl.addEventListener("pointermove", (ev) => {
	hover = bucketAt(ev);
	if (dragging) seek(hover);
});
heatEl.addEventListener("pointerup", () => (dragging = false));
heatEl.addEventListener("pointerleave", () => (hover = null));
heatEl.addEventListener("keydown", (ev) => {
	const by = ev.shiftKey ? 12 : 1;
	if (ev.key === "ArrowLeft") seek(Math.round(cursor) - by);
	else if (ev.key === "ArrowRight") seek(Math.round(cursor) + by);
	else if (ev.key === "End") goLive();
	else if (ev.key === " ") togglePlay();
	else return;
	ev.preventDefault();
});
playBtn.addEventListener("click", togglePlay);
liveBtn.addEventListener("click", goLive);
peakBtn.addEventListener("click", () => {
	playing = false;
	seek(peak);
});

const side = document.getElementById("side");
const devicesBtn = document.getElementById("devices-btn");
const eventsBtn = document.getElementById("events-btn");
let panel = null; // null, "list", "device", "events" or "sensor"

function showPanel(mode) {
	panel = mode;
	side.classList.toggle("open", mode !== null);
	devicesBtn.setAttribute("aria-expanded", String(mode === "list"));
	eventsBtn.setAttribute("aria-expanded", String(mode === "events"));
}

function closePanel() {
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

const CLOSE = `<button class="close" id="close" aria-label="Close">×</button>`;

function wireClose() {
	document.getElementById("close").addEventListener("click", closePanel);
}

function renderList() {
	const rows = DEVICES.map((d) => {
		const [tx, rx] = d.minutes[LOOP - 1];
		return { d, total: tx + rx };
	}).sort((a, b) => b.total - a.total);
	side.innerHTML = `${CLOSE}<div class="eyebrow">Devices, by the last minute</div><ul class="list">${rows
		.map(({ d, total }) => {
			const color = d.outlier ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : heatColor(heat(total));
			const rate = d.locationOnly ? "location only" : fmtBytes(total);
			return `<li><button data-id="${d.id}"><span class="swatch" style="background:${color}"></span><span>${d.name}</span><span class="rate">${rate}</span></button></li>`;
		})
		.join("")}</ul><p class="note">Or click a device on the plan.</p>`;
	wireClose();
	side.querySelectorAll("button[data-id]").forEach((b) => b.addEventListener("click", () => select(DEVICES.find((d) => d.id === b.dataset.id))));
}

function select(d) {
	if (!d) return closePanel();
	selected = d;
	const [tx, rx] = d.minutes[LOOP - 1];
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

		<div class="eyebrow">Last minute</div>
		<dl class="kv">
			<dt>sent</dt><dd>${fmtBytes(tx)}</dd>
			<dt>received</dt><dd>${fmtBytes(rx)}</dd>
			<dt>signal</dt><dd>${d.rssi ?? "-"} dBm</dd>
		</dl>

		<div class="eyebrow">Traffic, last 24 hours</div>
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
			<dt>first seen</dt><dd>${DATE_TIME.format(new Date(d.firstSeen * 1000))}</dd>
		</dl>`;
	document.getElementById("back").addEventListener("click", openList);
	wireClose();
	showPanel("device");
	drawSpark(d);
}

function drawSpark(d) {
	const el = document.getElementById("spark");
	const r = el.getBoundingClientRect();
	el.width = r.width * dpr;
	el.height = r.height * dpr;
	const c = el.getContext("2d");
	c.scale(dpr, dpr);
	// half hours: the average of six buckets
	const halves = Array.from({ length: BUCKETS / 6 }, (_, i) => d.day.slice(i * 6, i * 6 + 6).reduce((s, [a, b]) => s + a + b, 0) / 6);
	const max = Math.max(1, ...halves);
	const bw = r.width / halves.length;
	halves.forEach((v, i) => {
		const hgt = (v / max) * (r.height - 4);
		c.fillStyle = heatColor(heat(v));
		c.globalAlpha = 0.85;
		c.fillRect(i * bw + 1, r.height - hgt, Math.max(1, bw - 2), hgt);
	});
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
		playing = false;
		seek(Math.round(((e.from ?? e.time) - dayStart) / BUCKET_MS));
	}));
}

// --- the sensors: a pill each in the top bar, and their panel ---------------

const pillsEl = document.getElementById("sensor-pills");

function renderSensorPills() {
	pillsEl.innerHTML = SENSORS.map((s) =>
		`<button class="top-btn sensor-pill" data-sensor="${s.name}" aria-controls="side">`
		+ `<span class="dot ${sensorState(s)}"></span>${s.name}<span class="ago"></span></button>`).join("");
	pillsEl.querySelectorAll("button").forEach((b) => b.addEventListener("click", () =>
		(panel === "sensor" && openSensorName === b.dataset.sensor ? closePanel() : openSensor(b.dataset.sensor))));
	updateSensorAges();
}

// The "… ago" texts, the dots and the open panel's line, every tick.
function updateSensorAges() {
	for (const b of pillsEl.querySelectorAll("button")) {
		const s = SENSORS.find((x) => x.name === b.dataset.sensor);
		if (!s) continue;
		b.querySelector(".ago").textContent = fmtAgo(Date.now() / 1000 - s.last_seen);
		b.querySelector(".dot").className = `dot ${sensorState(s)}`;
	}
	const line = document.getElementById("sensor-ago");
	const s = SENSORS.find((x) => x.name === openSensorName);
	if (line && s) line.innerHTML = `<span class="dot ${sensorState(s)}"></span> ${sensorState(s)} · last report ${fmtAgo(Date.now() / 1000 - s.last_seen)}`;
}

let openSensorName = null;

function openSensor(name) {
	const s = SENSORS.find((x) => x.name === name);
	if (!s) return;
	selected = null;
	openSensorName = name;
	const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
	side.innerHTML = `
		${CLOSE}
		<div class="eyebrow">Sensor</div>
		<h2 class="name">${s.name}</h2>
		<div class="where" id="sensor-ago"></div>

		<div class="eyebrow">Heartbeat, last 24 hours</div>
		<canvas class="spark" id="beat"></canvas>
		<p class="note">${plural(s.reports, "report")}, ${s.missing} missing, ${plural(s.restarts, "restart")}</p>

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
		<p>Normal: counting, and reporting every minute.</p>
		<p class="note">Identify mode (hold the sensor next to a device to tell which one it is) comes after the floor plan.</p>`;
	wireClose();
	showPanel("sensor");
	updateSensorAges();
	drawBeat(s);
}

// The day as a strip: a bar per 5 minutes, as tall as the reports in it (five
// is a full one); a red tick where it was expected and nothing came.
function drawBeat(s) {
	const el = document.getElementById("beat");
	const r = el.getBoundingClientRect();
	el.width = r.width * dpr;
	el.height = r.height * dpr;
	const c = el.getContext("2d");
	c.scale(dpr, dpr);
	const bw = r.width / s.heartbeat.length;
	const first = s.heartbeat.findIndex((n) => n > 0);
	s.heartbeat.forEach((n, i) => {
		const x = i * bw;
		if (n > 0) {
			const h = (Math.min(n, BUCKET_MIN) / BUCKET_MIN) * (r.height - 4);
			c.fillStyle = n >= BUCKET_MIN - 1 ? "rgba(127,231,242,0.75)" : "rgba(242,193,78,0.85)";
			c.fillRect(x, r.height - h, Math.max(1, bw - 0.3), h);
		} else if (first >= 0 && i > first) {
			c.fillStyle = "rgba(255,59,59,0.8)";
			c.fillRect(x, r.height - 3, Math.max(1, bw - 0.3), 3);
		}
	});
}

// The button counts the day's alerts.
function renderEventsBtn() {
	const hot = EVENTS.filter((e) => e.hot).length;
	eventsBtn.innerHTML = `Timeline${hot ? `<span class="badge">${hot}</span>` : ""}`;
}

// Drag to pan; shift-drag or right-drag to rotate. A drag isn't a click.
let drag = null, dragged = false;
canvas.addEventListener("pointerdown", (ev) => {
	drag = { x: ev.clientX, y: ev.clientY, rotate: ev.shiftKey || ev.button === 2, moved: false };
	canvas.setPointerCapture(ev.pointerId);
});
canvas.addEventListener("pointermove", (ev) => {
	if (!drag) return;
	const dx = ev.clientX - drag.x, dy = ev.clientY - drag.y;
	if (!drag.moved && Math.hypot(dx, dy) < 4) return;
	if (!drag.moved) canvas.classList.add("dragging");
	drag.moved = true;
	takeControl();
	if (drag.rotate) {
		yaw = camT.yaw = yaw + dx * 0.006;
	} else {
		cam.panX = camT.panX = cam.panX + dx;
		cam.panY = camT.panY = cam.panY + dy;
	}
	[drag.x, drag.y] = [ev.clientX, ev.clientY];
});
canvas.addEventListener("pointerup", () => {
	dragged = drag?.moved ?? false;
	drag = null;
	canvas.classList.remove("dragging");
});
canvas.addEventListener("contextmenu", (ev) => ev.preventDefault());
canvas.addEventListener("wheel", (ev) => {
	ev.preventDefault();
	const r = canvas.getBoundingClientRect();
	zoomAt(ev.clientX - r.left, ev.clientY - r.top, Math.exp(-ev.deltaY * 0.0015));
}, { passive: false });
canvas.addEventListener("keydown", (ev) => {
	const step = Math.PI / 12; // 15°
	if (ev.key === "ArrowLeft") rotate(-step);
	else if (ev.key === "ArrowRight") rotate(step);
	else if (ev.key === "+" || ev.key === "=") zoomAt(W / 2, H / 2, 1.25);
	else if (ev.key === "-") zoomAt(W / 2, H / 2, 0.8);
	else if (ev.key === "0") resetView();
	else return;
	ev.preventDefault();
});
document.getElementById("rot-left").addEventListener("click", () => rotate(-Math.PI / 12));
document.getElementById("rot-right").addEventListener("click", () => rotate(Math.PI / 12));
document.getElementById("zoom-in").addEventListener("click", () => zoomAt(W / 2, H / 2, 1.25));
document.getElementById("zoom-out").addEventListener("click", () => zoomAt(W / 2, H / 2, 0.8));
document.getElementById("view-reset").addEventListener("click", resetView);

// pick a device by clicking near its node, or the sensor
canvas.addEventListener("click", (ev) => {
	if (dragged) {
		dragged = false;
		return;
	}
	const r = canvas.getBoundingClientRect();
	const x = ev.clientX - r.left, y = ev.clientY - r.top;
	if (SENSORS[0] && SENSOR.screen && Math.hypot(SENSOR.screen[0] - x, SENSOR.screen[1] - y) < 16) {
		return openSensor(SENSORS[0].name);
	}
	let best = null, bestDist = 24;
	for (const d of DEVICES) {
		const dist = Math.hypot(d.screen[0] - x, d.screen[1] - y);
		if (dist < bestDist) [best, bestDist] = [d, dist];
	}
	// a click on an empty spot closes the panel
	select(best);
});

devicesBtn.addEventListener("click", () => (panel === "list" ? closePanel() : openList()));
eventsBtn.addEventListener("click", () => (panel === "events" ? closePanel() : openEvents()));

document.addEventListener("keydown", (ev) => {
	if (ev.key === "Escape" && panel) closePanel();
});

setInterval(() => {
	document.getElementById("clock").textContent = TIME_S.format(new Date());
	updateSensorAges();
}, 250);

resize();
onData();
loadOverview();
requestAnimationFrame((now) => {
	lastFrame = loopStart = ringStart = now;
	requestAnimationFrame(step);
});
