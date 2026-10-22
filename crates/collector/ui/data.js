// The data, from the collector: /v1/overview (the devices, their last minutes
// and the sensors) and /v1/timeline (the stretch on the graph).

import { hhmm, whenLabel } from "./format.js";
import { closePanel, onData, selected } from "./panel.js";
import { arrange } from "./plan.js";
import { play } from "./playback.js";
import { rand } from "./random.js";
import { renderSensorStatus } from "./sensors.js";
import { range, renderControls, zoomed } from "./timeline.js";

export const LOOP = 5; // minutes in the replay loop

// The timeline: the stretch on show, from /v1/timeline (docs/timeline.md).
// `total` is per bucket [average, peak] bytes a minute; `series` each
// device's [tx, rx] per bucket. A range ends at the newest report, or it's
// the stretch zoomed in to (`zoomed` in timeline.js).
// `hours`: each bucket's hour of the day, or null for buckets longer than an
// hour (scored against all hours).
export const TL = { from: 0, to: 0, bucketMs: 300_000, total: [], series: new Map(), hours: [], scans: [], changes: [] };
export const buckets = () => TL.total.length;
export const bucketTime = (i) => new Date(TL.from * 1000 + i * TL.bucketMs);
export const bucketOf = (date) => (date - TL.from * 1000) / TL.bucketMs;
export const seriesAt = (d, i) => TL.series.get(d.mac)?.[i] ?? [0, 0];

// The loop's minutes, as dates, and the hour of the day of each.
export let loopTimes = [];
export let loopHours = [];

// How traffic becomes a score, from the overview: `{ k, floor, white, red }`.
export let SCALE = null;

// Every device seen in the last 24 hours. `minutes` are the loop's [tx, rx]
// bytes; its history is in `TL.series`. `usual`: its baselines, `{ hours,
// all, learning }`, or null before the first are worked out. `locationOnly`
// and `outlier` aren't filled in yet: devices the sensors can place but not
// count, and the outlier rules (docs/monitoring.md, plan step 7).
export const DEVICES = [];

// The network's average and peak bytes a minute in bucket i, and its busiest
// device then.
export const totalAt = (i) => TL.total[i]?.[0] ?? 0;
export const peakAt = (i) => TL.total[i]?.[1] ?? 0;
export const sum = ([tx, rx]) => tx + rx;
export const busiestAt = (i) => DEVICES.reduce((a, d) => (!a || sum(seriesAt(d, i)) > sum(seriesAt(a, i)) ? d : a), null);

// What the HUD shows about the sensors.
export const STATUS = { sensors: 0, channel: 0, dropped: 0 };

// The sensors, from the overview, each with `beatAt`: when the page saw a new
// report from it (for the heartbeat ring on the plan).
export let SENSORS = [];

// The Timeline panel's events: `{ time, text, hot, device?, from?, to? }`.
// `hot`: an alert (a new device or port; later the outlier rules too);
// `device`: a click opens it; `from`/`to`: a time span. The port scans, and
// what they found changed.
export const EVENTS = [];

// The day's port scans, as `{ from, to }` dates, marked on the day graph.
export let PORT_SCANS = [];

// The day's identify sessions, as `{ from, to, sensor }`, marked on the day
// graph: the sensor didn't count during them.
export let SESSIONS = [];

// A MAC with the "locally administered" bit set is made up (a phone's private
// address per network), so no vendor can be known.
export const vendorless = (mac) => (parseInt(mac[1], 16) & 2 ? "private address (made-up MAC)" : "unknown");

// Each flow starts partway to its next train of packets, or every arc stays
// empty for the first seconds and then they all fire at once.
function newDevice(mac) {
	return { id: mac, name: mac, short: mac.slice(9), room: "waiting to be placed", x: 0, y: 0, mac, vendor: "unknown", ports: [], dns: [],
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
	seen.forEach((x) => {
		const d = old.get(x.mac) ?? newDevice(x.mac);
		// the name given, else the one the host sweep found (without the local
		// domain), else the MAC
		const host = x.hostname?.split(".")[0];
		Object.assign(d, {
			minutes: pad(x.minutes), rssi: x.rssi_max, firstSeen: x.first_seen, given: x.name,
			name: x.name ?? host ?? x.mac, short: x.name ?? host ?? x.mac.slice(9), vendor: x.vendor ?? vendorless(x.mac),
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
	arrange();
	renderSensorStatus();
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
	const at = bucketTime(play.cursor);
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
	SESSIONS = t.identify_sessions.map((s) => ({ from: new Date(s.started_at * 1000), to: new Date(s.ended_at * 1000), sensor: s.sensor }));
	for (const s of SESSIONS) {
		const end = s.to > Date.now() ? "until now" : `–${hhmm(s.to)}`;
		EVENTS.push({
			time: s.to, from: s.from, to: s.to, hot: false,
			text: `Identify on ${s.sensor}, ${whenLabel(s.from)} ${end}: it didn't count meanwhile.`,
		});
	}
	if (play.mode === "live") play.cursor = buckets() - 1;
	else play.cursor = Math.max(0, Math.min(bucketOf(at), buckets() - 1));
	renderControls();
	onData();
}

export async function loadTimeline() {
	const query = zoomed ? `from=${zoomed.from}&to=${zoomed.to}` : `range=${range}`;
	try {
		const res = await fetch(`/v1/timeline?${query}`);
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		applyTimeline(await res.json());
	} catch (e) {
		console.error("timeline:", e);
	}
}

export async function loadOverview() {
	try {
		const res = await fetch("/v1/overview");
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		applyOverview(await res.json());
	} catch (e) {
		console.error("overview:", e);
	}
}

// Gives a device a name, or takes it away with an empty one, then fetches the
// overview so it shows everywhere. Returns why it failed, or null.
export async function renameDevice(mac, name) {
	try {
		const res = await fetch(`/v1/devices/${encodeURIComponent(mac)}/name`, {
			method: "PUT",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ name }),
		});
		if (!res.ok) return (await res.text()) || `HTTP ${res.status}`;
	} catch (e) {
		return e.message;
	}
	await loadOverview();
	return null;
}
