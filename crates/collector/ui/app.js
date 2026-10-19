"use strict";

// ---------------------------------------------------------------------------
// The floor plan: the one saved through /v1/plan, else this example. It's
// made up, not anyone's house. Rooms are polygons in metres, origin top left,
// y down; openings are doorways on an edge (docs/floorplan.md).
// ---------------------------------------------------------------------------

const EXAMPLE_PLAN = {
	rooms: [
		{ id: "living", name: "Living room", points: [[0, 0], [4.5, 0], [4.5, 3.6], [0, 3.6]] },
		{ id: "kitchen", name: "Kitchen", points: [[4.5, 0], [8.2, 0], [8.2, 3.6], [4.5, 3.6]] },
		{ id: "office", name: "Office", points: [[8.2, 0], [11.5, 0], [11.5, 3.6], [8.2, 3.6]] },
		{ id: "bedroom", name: "Bedroom", points: [[0, 3.6], [3.3, 3.6], [3.3, 6.5], [0, 6.5]] },
		{ id: "hall", name: "Hall", points: [[3.3, 3.6], [6, 3.6], [6, 6.5], [3.3, 6.5]] },
		{ id: "bathroom", name: "Bathroom", points: [[6, 3.6], [9.5, 3.6], [9.5, 6.5], [6, 6.5]] },
		{ id: "utility", name: "Utility", points: [[9.5, 3.6], [11.5, 3.6], [11.5, 6.5], [9.5, 6.5]] },
	],
	openings: [
		{ from: [4.5, 0], to: [4.5, 3.6], at: 0.6, width: 0.8 },
		{ from: [8.2, 0], to: [8.2, 3.6], at: 0.4, width: 0.8 },
		{ from: [0, 3.6], to: [3.3, 3.6], at: 0.5, width: 0.8 },
		{ from: [3.3, 3.6], to: [6, 3.6], at: 0.75, width: 0.8 },
		{ from: [3.3, 3.6], to: [3.3, 6.5], at: 0.5, width: 0.8 },
		{ from: [6, 3.6], to: [6, 6.5], at: 0.3, width: 0.8 },
		{ from: [9.5, 3.6], to: [9.5, 6.5], at: 0.5, width: 0.8 },
		{ from: [6, 6.5], to: [3.3, 6.5], at: 0.5, width: 1 },
	],
};

// Plan units; the view projects them into 3D. A plan in metres is scaled to
// fit 1000 by 600 inside the margin, so the look below holds for a flat or a
// house; PLAN then takes the plan's own shape (setPlan).
const PLAN = { w: 1000, h: 600 };
const MARGIN = 40;
const WALL_H = 56;

// Where the router and the sensor stand until they can be placed by hand,
// when that's inside a room (plan units).
const ROUTER_AT = [410, 470];
const SENSOR_AT = [455, 520];
const ROUTER = { x: ROUTER_AT[0], y: ROUTER_AT[1], name: "Router" };
const SENSOR = { x: SENSOR_AT[0], y: SENSOR_AT[1], name: "Sensor" };

// ---------------------------------------------------------------------------
// The data, from the collector's /v1/overview
// ---------------------------------------------------------------------------

const MiB = 1024 * 1024;
const MINUTE_MS = 6000; // how long each replayed minute lasts on screen
const LOOP = 5; // minutes in the replay loop

// The timeline: the stretch on show, from /v1/timeline (docs/timeline.md).
// `total` is per bucket [average, peak] bytes a minute; `series` each
// device's [tx, rx] per bucket. A range ends at the newest report; `zoomed`
// is a stretch picked on the graph, in unix seconds.
const RANGES = [["1h", "1 hour"], ["6h", "6 hours"], ["24h", "24 hours"], ["7d", "7 days"], ["30d", "30 days"]];
let range = "24h";
let zoomed = null;
// `hours`: each bucket's hour of the day, or null for buckets longer than an
// hour (scored against all hours).
const TL = { from: 0, to: 0, bucketMs: 300_000, total: [], series: new Map(), hours: [], scans: [], changes: [] };
const buckets = () => TL.total.length;
const bucketTime = (i) => new Date(TL.from * 1000 + i * TL.bucketMs);
const bucketOf = (date) => (date - TL.from * 1000) / TL.bucketMs;
const seriesAt = (d, i) => TL.series.get(d.mac)?.[i] ?? [0, 0];

// The loop's minutes, as dates, and the hour of the day of each.
let loopTimes = [];
let loopHours = [];

// How traffic becomes a score, from the overview: `{ k, floor, white, red }`.
let SCALE = null;

// Every device seen in the last 24 hours. `minutes` are the loop's [tx, rx]
// bytes; its history is in `TL.series`. `usual`: its baselines, `{ hours,
// all, learning }`, or null before the first are worked out. `locationOnly`
// and `outlier` aren't filled in yet: devices the sensors can place but not
// count, and the outlier rules (docs/monitoring.md, plan step 7).
const DEVICES = [];

// The network's average and peak bytes a minute in bucket i, and its busiest
// device then.
const totalAt = (i) => TL.total[i]?.[0] ?? 0;
const peakAt = (i) => TL.total[i]?.[1] ?? 0;
const sum = ([tx, rx]) => tx + rx;
const busiestAt = (i) => DEVICES.reduce((a, d) => (!a || sum(seriesAt(d, i)) > sum(seriesAt(a, i)) ? d : a), null);

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
// plan's rooms, the same one on every load: spots shuffled once, handed out in
// MAC order. Set by setPlan.
let SPOTS = [];

// A MAC with the "locally administered" bit set is made up (a phone's private
// address per network), so no vendor can be known.
const vendorless = (mac) => (parseInt(mac[1], 16) & 2 ? "private address (made-up MAC)" : "unknown");

// Each flow starts partway to its next train of packets, or every arc stays
// empty for the first seconds and then they all fire at once.
function newDevice(mac) {
	return { id: mac, name: mac, short: mac.slice(9), room: "not placed yet", mac, vendor: "unknown", ports: [], dns: [],
		particles: [], spawn: [rand(), rand()], shownAt: -1e9, screen: [0, 0] };
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
	const pad = (m, fill = [0, 0]) => [...Array(Math.max(0, LOOP - m.length)).fill(fill), ...m.slice(-LOOP)];
	loopTimes = pad(o.minutes.map((t) => new Date(t * 1000)), null);
	loopHours = pad(o.hours, null);
	SCALE = o.score;
	const seen = [...o.devices].sort((a, b) => a.mac.localeCompare(b.mac));
	const old = new Map(DEVICES.map((d) => [d.id, d]));
	DEVICES.length = 0;
	seen.forEach((x, i) => {
		const d = old.get(x.mac) ?? newDevice(x.mac);
		const [sx, sy] = SPOTS[i % SPOTS.length] ?? [ROUTER.x, ROUTER.y];
		// the name the host sweep found (without the local domain), else the MAC
		const host = x.hostname?.split(".")[0];
		Object.assign(d, {
			x: sx, y: sy, minutes: pad(x.minutes), rssi: x.rssi_max, firstSeen: x.first_seen,
			name: host ?? x.mac, short: host ?? x.mac.slice(9), vendor: x.vendor ?? vendorless(x.mac),
			ip: x.ip, hostname: x.hostname, ports: x.ports, portScannedAt: x.port_scanned_at, usual: x.usual,
		});
		DEVICES.push(d);
	});
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

// Takes in a timeline: the graph, the plan's history, and the stretch's port
// scans and changes as Timeline events.
function applyTimeline(t) {
	// the playhead stays on its moment while the range moves on with new reports
	const at = bucketTime(cursor);
	Object.assign(TL,{ from: t.from, to: t.to, bucketMs: t.bucket_s * 1000, total: t.total, hours: t.hours });
	TL.series = new Map(t.devices.map((x) => [x.mac, x.buckets]));
	PORT_SCANS = t.port_scans.map((p) => ({ from: new Date(p.started_at * 1000), to: new Date(p.finished_at * 1000), ...p }));
	EVENTS.length = 0;
	for (const p of PORT_SCANS) {
		EVENTS.push({
			time: p.to, from: p.from, to: p.to, hot: false,
			text: `Port scan, ${whenLabel(p.from)}–${hhmm(p.to)}: ${p.hosts} hosts, ${p.open_ports} open ports`
				+ (p.timed_out ? `, ${p.timed_out} timed out (their ports aren't known)` : "")
				+ (p.alerts ? `, ${p.alerts} alert${p.alerts > 1 ? "s" : ""}` : "") + ".",
		});
	}
	for (const c of t.changes) EVENTS.push(changeEvent(c));
	if (mode === "live") cursor = buckets() - 1;
	else cursor = Math.max(0, Math.min(bucketOf(at), buckets() - 1));
	renderControls();
	onData();
}

async function loadTimeline() {
	const query = zoomed ? `from=${zoomed.from}&to=${zoomed.to}` : `range=${range}`;
	try {
		const res = await fetch(`/v1/timeline?${query}`);
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		applyTimeline(await res.json());
	} catch (e) {
		console.error("timeline:", e);
	}
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

// ---------------------------------------------------------------------------
// Colour by a device's own normal (docs/outliers.md): how unusual its bytes a
// minute are for it at that hour of the day. Size and particles stay on
// traffic, through `heat`.
// ---------------------------------------------------------------------------

// The score: 0 at its usual or below, `SCALE.red` at red. `hour` null: against
// all hours. Null while the device is still learning.
function score(d, bytes, hour) {
	const u = d.usual;
	if (!u || u.learning || !SCALE) return null;
	const [median, mad] = hour == null ? u.all : u.hours[hour];
	return Math.max(0, (SCALE.k * (Math.log1p(bytes) - median)) / Math.max(mad, SCALE.floor));
}

// White up to `SCALE.white`, then ramping to red at `SCALE.red`; learning is
// a neutral grey.
const LEARNING = [143, 166, 175];
// How far a score is from white (0) to red (1).
const redness = (s) => (s == null ? 0 : Math.min(1, Math.max(0, (s - SCALE.white) / (SCALE.red - SCALE.white))));
function scoreColor(s, alpha = 1) {
	if (s == null) return `rgba(${LEARNING.join(",")},${alpha})`;
	return heatColor(redness(s), alpha);
}

// The hour of the day the plan is showing.
function hourNow(now) {
	if (mode === "history") return TL.hours[Math.floor(cursor)] ?? null;
	return loopHours[Math.min(LOOP - 1, Math.floor((now - loopStart) / MINUTE_MS))] ?? null;
}

const scoreNow = (d, now) => score(d, sum(current(d, now)), hourNow(now));

// The minute or bucket the plan shows: when it is, and a device's [tx, rx]
// bytes a minute then.
function shownMoment(now) {
	if (mode === "history") {
		const i = Math.floor(cursor);
		const when = TL.bucketMs > 60_000 ? `${whenLabel(bucketTime(i))}–${hhmm(bucketTime(i + 1))}, average` : whenLabel(bucketTime(i));
		return { when, at: (d) => seriesAt(d, i) };
	}
	const i = Math.min(LOOP - 1, Math.floor((now - loopStart) / MINUTE_MS));
	return { when: loopTimes[i] ? `live · ${hhmm(loopTimes[i])}` : "live", at: (d) => d.minutes[i] };
}

// The open device panel's traffic, usual range and bars follow the plan's
// moment.
function updateMoment(now) {
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

// ---------------------------------------------------------------------------
// The plan in plan units: walls, floor and labels
// ---------------------------------------------------------------------------

// Set by setPlan: the rooms (points in plan units, where the name goes), the
// walls as point-cloud curtains (dots along each wall, stacked to wall
// height), and the floor (a dot grid with a little jitter, so it reads as
// scanned, not drawn).
let rooms = [];
let wallDots = [];
let floorDots = [];

const EPS = 1e-6;
const corner = ([x, y]) => `${x},${y}`;

// The plan from metres into plan units: its bounds fitted into 1000 by 600,
// the same scale both ways, with the margin round them; w and h are its frame.
function toUnits(plan) {
	const xs = plan.rooms.flatMap((r) => r.points.map((p) => p[0]));
	const ys = plan.rooms.flatMap((r) => r.points.map((p) => p[1]));
	const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
	const s = Math.min((1000 - 2 * MARGIN) / (x1 - x0), (600 - 2 * MARGIN) / (y1 - y0));
	const u = ([x, y]) => [MARGIN + (x - x0) * s, MARGIN + (y - y0) * s];
	return {
		w: (x1 - x0) * s + 2 * MARGIN,
		h: (y1 - y0) * s + 2 * MARGIN,
		rooms: plan.rooms.map((r) => ({ name: r.name, points: r.points.map(u) })),
		openings: (plan.openings ?? []).map((o) => ({ from: u(o.from), to: u(o.to), at: o.at, width: o.width * s })),
	};
}

// Where c is along a→b: its distance from a along the line, and from the line.
function along(c, a, b) {
	const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
	const [dx, dy] = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
	const [px, py] = [c[0] - a[0], c[1] - a[1]];
	return { t: px * dx + py * dy, off: Math.abs(px * dy - py * dx), len };
}

// The walls: every room edge, split at each corner that lies on it, so rooms
// that share a wall, whole or in part, give the same pieces; each piece once.
function wallPieces(rooms) {
	const corners = rooms.flatMap((r) => r.points);
	const pieces = new Map();
	for (const { points } of rooms) {
		points.forEach((a, i) => {
			const b = points[(i + 1) % points.length];
			const stops = corners
				.map((c) => ({ c, ...along(c, a, b) }))
				.filter(({ t, off, len }) => off < EPS && t > EPS && t < len - EPS)
				.sort((p, q) => p.t - q.t)
				.map(({ c }) => c);
			[a, ...stops, b].forEach((p, k, all) => {
				const q = all[k + 1];
				if (q && corner(p) !== corner(q)) pieces.set([corner(p), corner(q)].sort().join(" "), [p, q]);
			});
		});
	}
	return [...pieces.values()];
}

// Whether p is in a doorway: on its edge's line, within its width.
function inOpening(p, openings) {
	return openings.some(({ from, to, at, width }) => {
		const { t, off, len } = along(p, from, to);
		return off < EPS && Math.abs(t - at * len) < width / 2;
	});
}

// Whether p is inside the polygon (ray casting).
function inside([x, y], points) {
	let odd = false;
	for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
		const [xi, yi] = points[i], [xj, yj] = points[j];
		if (yi > y !== yj > y && x < xi + ((y - yi) * (xj - xi)) / (yj - yi)) odd = !odd;
	}
	return odd;
}

// The widest stretch across the polygon at height y, [left, right].
function widestAt(y, points) {
	const xs = [];
	for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
		const [xi, yi] = points[i], [xj, yj] = points[j];
		if (yi > y !== yj > y) xs.push(xi + ((y - yi) * (xj - xi)) / (yj - yi));
	}
	xs.sort((a, b) => a - b);
	let best = [xs[0], xs[1]];
	for (let k = 2; k + 1 < xs.length; k += 2) if (xs[k + 1] - xs[k] > best[1] - best[0]) best = [xs[k], xs[k + 1]];
	return best;
}

// A room's name goes inset from its top left, on the widest stretch across it
// just below its top.
function labelAt(points) {
	const ys = points.map((p) => p[1]);
	const y = Math.min(Math.min(...ys) + 30, (Math.min(...ys) + Math.max(...ys)) / 2);
	const [l, r] = widestAt(y, points);
	return [l + Math.min(20, (r - l) / 2), y];
}

// A point in the middle of a room.
function middleOf(points) {
	const ys = points.map((p) => p[1]);
	const y = (Math.min(...ys) + Math.max(...ys)) / 2;
	const [l, r] = widestAt(y, points);
	return [(l + r) / 2, y];
}

// Draws this plan from now on: a saved one or the example.
function setPlan(plan) {
	const p = toUnits(plan);
	Object.assign(PLAN, { w: p.w, h: p.h });
	const inPlan = (q) => p.rooms.some((r) => inside(q, r.points));
	rooms = p.rooms.map((r) => ({ ...r, label: labelAt(r.points) }));

	wallDots = [];
	const pieces = wallPieces(p.rooms);
	const corners = new Map(pieces.flat().map((c) => [corner(c), c]));
	const between = pieces.flatMap(([[x1, y1], [x2, y2]]) => {
		const n = Math.max(1, Math.round(Math.hypot(x2 - x1, y2 - y1) / 6));
		return Array.from({ length: n - 1 }, (_, k) => [x1 + ((x2 - x1) * (k + 1)) / n, y1 + ((y2 - y1) * (k + 1)) / n]);
	});
	for (const [x, y] of [...corners.values(), ...between]) {
		if (inOpening([x, y], p.openings)) continue;
		for (let z = 0; z <= WALL_H; z += 7) {
			wallDots.push({ x, y, z, top: z + 7 > WALL_H, phase: rand() * Math.PI * 2 });
		}
	}

	floorDots = [];
	for (let x = 50; x < PLAN.w - 40; x += 16) {
		for (let y = 50; y < PLAN.h - 40; y += 16) {
			const q = [x + (rand() - 0.5) * 4, y + (rand() - 0.5) * 4];
			if (inPlan(q)) floorDots.push({ x: q[0], y: q[1], a: 0.25 + rand() * 0.5 });
		}
	}

	// the router and the sensor where they stood, if that's in a room
	const [rx, ry] = inPlan(ROUTER_AT) ? ROUTER_AT : middleOf(p.rooms[0].points);
	const near = [rx + 45, ry + 50];
	const [sx, sy] = inPlan(SENSOR_AT) ? SENSOR_AT : inPlan(near) ? near : [rx, ry];
	Object.assign(ROUTER, { x: rx, y: ry });
	Object.assign(SENSOR, { x: sx, y: sy });

	SPOTS = [];
	for (let x = 100; x <= PLAN.w - 100; x += 114) {
		for (let y = 95; y <= PLAN.h - 90; y += 83) {
			if (inPlan([x, y]) && Math.hypot(x - rx, y - ry) > 70 && Math.hypot(x - sx, y - sy) > 50) SPOTS.push([x, y]);
		}
	}
	let seed = 11;
	for (let i = SPOTS.length - 1; i > 0; i--) {
		seed = (seed * 1103515245 + 12345) & 0x7fffffff;
		const j = seed % (i + 1);
		[SPOTS[i], SPOTS[j]] = [SPOTS[j], SPOTS[i]];
	}
	DEVICES.forEach((d, i) => ([d.x, d.y] = SPOTS[i % SPOTS.length] ?? [rx, ry]));
	resize();
}

// The saved plan, or the example until one with rooms is saved.
async function loadPlan() {
	let plan = null;
	try {
		const res = await fetch("/v1/plan");
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		plan = await res.json();
	} catch (e) {
		console.error("plan:", e);
	}
	setPlan(plan?.rooms.length ? plan : EXAMPLE_PLAN);
}

// ---------------------------------------------------------------------------
// The loop
// ---------------------------------------------------------------------------

let loopStart = 0, ringStart = 0, lastFrame = 0, glitchUntil = 0;
let selected = null;
let lastPlayed = -1;

// "live" plays the loop of the last minutes; "history" shows the timeline's
// bucket at `cursor`, which is fractional while playing. Play loops over the
// stretch, a whole pass in about a minute at any range.
let mode = "live";
let cursor = 0;
let playing = false;
const PASS_S = 60;

// Traffic at this moment, worked out once a frame. History plays several
// buckets a second, so it eases toward them (on the log scale the colours
// use), or the plan would flash from bucket to bucket.
const EASE_MS = 300;
function current(d, now) {
	if (d.curAt === now) return d.cur;
	const target = traffic(d, now);
	const a = mode === "history" && d.cur ? 1 - Math.exp(-(now - d.curAt) / EASE_MS) : 1;
	d.cur = a < 1 ? d.cur.map((v, k) => Math.expm1(Math.log1p(v) + (Math.log1p(target[k]) - Math.log1p(v)) * a)) : target;
	d.curAt = now;
	return d.cur;
}

// Live: eases from the previous minute into the current one over the first
// second. History: between two buckets while playing. Only real minutes and
// buckets are used.
function traffic(d, now) {
	if (mode === "history") {
		const i = Math.floor(cursor), j = Math.min(buckets() - 1, i + 1), u = cursor - i;
		const a = seriesAt(d, i), b = seriesAt(d, j);
		return [0, 1].map((k) => a[k] + (b[k] - a[k]) * u);
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
// sensor sent one. A zoomed-in stretch doesn't move.
function newMinute(now) {
	ringStart = now;
	loadOverview();
	if (!zoomed) loadTimeline();
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
	// held while the playhead is dragged; playing carries on from where it's dropped
	if (playing && !dragging) {
		cursor += ((now - lastFrame) / 1000) * (buckets() / PASS_S);
		if (cursor >= buckets() - 1) cursor = 0;
	}
	moveCamera(now);
	draw(now);
	drawTimeline(now);
	updateMoment(now);
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
	for (const r of rooms) {
		const [x, y] = project(r.label[0], r.label[1]);
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

// Clip to the rooms, so glows and the ring stay on the floor.
function clipToFloor() {
	ctx.beginPath();
	for (const r of rooms) {
		r.points.forEach(([x, y], i) => {
			const [px, py] = project(x, y);
			i ? ctx.lineTo(px, py) : ctx.moveTo(px, py);
		});
		ctx.closePath();
	}
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
		const s = scoreNow(d, now);
		const [x, y, k] = project(d.x, d.y);
		const r = (30 + 80 * t) * fit * cam.zoom * k;
		const g = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
		g.addColorStop(0, scoreColor(s, 0.22 * t));
		g.addColorStop(1, scoreColor(s, 0));
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
	const s = scoreNow(d, now);

	// the channel, faint, with a slow drift each way
	ctx.lineCap = "butt";
	ctx.strokeStyle = scoreColor(s, reduced ? 0.3 + 0.5 * all : 0.05 + 0.1 * all);
	ctx.lineWidth = reduced ? 1 + 3 * all : 1;
	path(arc);
	ctx.stroke();
	if (reduced) return;
	[tx, rx].forEach((bytes, k) => {
		const t = heat(bytes);
		ctx.setLineDash(k ? [1, 13] : [1, 9]);
		ctx.lineDashOffset = (k ? 1 : -1) * (now / 1000) * 8;
		ctx.strokeStyle = scoreColor(s, 0.08 + 0.2 * t);
		path(arc);
		ctx.stroke();
	});
	ctx.setLineDash([]);

	// History plays half an hour or more a second, too short for the trains
	// traffic alone sends, so there a red moment adds up to one train a second
	// each way: a second of red sends at least one.
	const extra = mode === "history" ? redness(s) : 0;
	[tx, rx].forEach((bytes, k) => {
		const t = heat(bytes);
		d.spawn[k] += dt * (0.06 + 0.6 * t * t + extra);
		while (d.spawn[k] >= 1) {
			d.spawn[k] -= 1;
			const n = 1 + Math.floor(rand() * (1 + 5 * t));
			let gap = 0;
			for (let j = 0; j < n; j++) {
				const size = 5 + rand() * (6 + 16 * t);
				d.particles.push({ lane: k, s: -gap, size, score: s });
				gap += size + 5;
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
		g.addColorStop(0, scoreColor(p.score, 0));
		g.addColorStop(1, scoreColor(p.score, 0.95));
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

	const s = scoreNow(d, now);
	const color = glitching ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : scoreColor(s);
	// the pulse's phase adds up, so a change in traffic changes its speed
	// without making it jump
	d.pulsePhase = (d.pulsePhase ?? 0) + (now - (d.pulseAt ?? now)) / (500 - 300 * t);
	d.pulseAt = now;
	const pulse = reduced || d.locationOnly ? 0 : (Math.sin(d.pulsePhase) + 1) * (2 + 6 * t);

	if (!d.locationOnly) {
		ctx.fillStyle = glitching ? "rgba(255,59,59,0.2)" : scoreColor(s, 0.12);
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
let picking = null; // shift-drag: the stretch being picked, [from, to] buckets

function heatGeom() {
	const r = heatEl.getBoundingClientRect();
	return { w: r.width, h: r.height, cw: (r.width - GUTTER) / Math.max(1, buckets()), ph: r.height - TOP - AXIS };
}

const colX = (g, i) => GUTTER + i * g.cw;
const valY = (g, v) => TOP + g.ph * (1 - v / graphMax);

// "today 18:50", "yesterday 18:50", else "17/10 18:50".
const DAY_MONTH = fmt({ day: "2-digit", month: "2-digit" });
function whenLabel(date) {
	const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(date).setHours(0, 0, 0, 0)) / 86_400_000);
	const day = days === 0 ? "today" : days === 1 ? "yesterday" : DAY_MONTH.format(date);
	return `${day} ${hhmm(date)}`;
}

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
		atEl.innerHTML = `<b>${whenLabel(bucketTime(i))}</b> · ${fmtBytes(totalAt(i))}/min`
			+ (d ? ` · most: ${d.short}, ${fmtBytes(sum(seriesAt(d, i)))}/min` : "");
	}
}

const rangesEl = document.getElementById("ranges");
const zoomOutBtn = document.getElementById("zoom-out-tl");

function renderControls() {
	playBtn.textContent = playing ? "Pause" : "Play";
	liveBtn.setAttribute("aria-pressed", String(mode === "live"));
	rangesEl.innerHTML = RANGES.map(([r, label]) =>
		`<button class="tl-btn range" data-range="${r}" aria-pressed="${!zoomed && r === range}" title="the last ${label}">${r}</button>`).join("");
	rangesEl.querySelectorAll("button").forEach((b) => b.addEventListener("click", () => setRange(b.dataset.range)));
	zoomOutBtn.hidden = !zoomed;
}

function setRange(r) {
	range = r;
	zoomed = null;
	playing = false;
	loadTimeline();
	renderControls();
}

// Zoom in to buckets a to b (shift-drag), or back out to the range.
function zoomTo(a, b) {
	if (b - a < 2) return;
	zoomed = { from: Math.floor(bucketTime(a) / 1000), to: Math.ceil(bucketTime(b + 1) / 1000) };
	playing = false;
	mode = "history";
	cursor = 0;
	loadTimeline();
	renderControls();
}

function zoomOut() {
	if (!zoomed) return;
	zoomed = null;
	loadTimeline();
	renderControls();
}

function seek(i) {
	mode = "history";
	cursor = Math.max(0, Math.min(buckets() - 1, i));
	renderControls();
}

function goLive() {
	mode = "live";
	playing = false;
	if (zoomed) zoomOut();
	cursor = buckets() - 1;
	renderControls();
}

// Play from the playhead; from the start if it's live or at the end.
function togglePlay() {
	if (!playing && (mode === "live" || cursor >= buckets() - 2)) seek(0);
	playing = !playing;
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
	if (ev.key === "ArrowLeft") seek(Math.round(cursor) - by);
	else if (ev.key === "ArrowRight") seek(Math.round(cursor) + by);
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
			const color = d.outlier ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : scoreColor(score(d, total, loopHours[LOOP - 1]));
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
	return `${TL.from}:${TL.to}:${mode === "live" ? "live" : Math.floor(Math.floor(cursor) / per)}`;
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
	const at = mode === "live" ? bars.length - 1 : Math.min(bars.length - 1, Math.floor(Math.floor(cursor) / per));
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
		c.fillStyle = mode === "live" ? "#7fe7f2" : "#e8f4f6";
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
		playing = false;
		seek(Math.round(bucketOf(e.from ?? e.time)));
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
		<p>Normal: counting, and reporting every minute.</p>
		<p class="note">Identify mode (hold the sensor next to a device to tell which one it is) comes after the floor plan.</p>`;
	wireClose();
	showPanel("sensor");
	updateSensorAges();
	drawHeartbeat(s);
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
renderControls();
onData();
loadPlan();
loadOverview();
// the page opens playing the range (not with reduced motion)
loadTimeline().then(() => {
	if (reduced || !buckets()) return;
	seek(0);
	playing = true;
	renderControls();
});
requestAnimationFrame((now) => {
	lastFrame = loopStart = ringStart = now;
	requestAnimationFrame(step);
});
