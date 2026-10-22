// Drawing the plan, each frame: the floor, walls and rooms, the areas in
// front of the house, the traffic as glows and packets on arcs to the router,
// the devices, the sensors, the selected device's box and the HUD. Things
// whose names would overlap stack, as in the editor (chunk 8 of
// docs/floorplan.md).

import { heat, redness, score, scoreColor } from "./colour.js";
import { DEVICES, SENSORS, STATUS } from "./data.js";
import { fmtBytes } from "./format.js";
import { fanColumn, stacks } from "./geometry.js";
import { selected } from "./panel.js";
import { ROUTER, WALL_H, areas, floorDots, rooms, sensorAt, wallDots } from "./plan.js";
import { current, outlierNow, play, scoreNow } from "./playback.js";
import { rand } from "./random.js";
import { STATE_COLOR, sensorState } from "./sensors.js";
import { H, TILT, W, cam, cameraKey, canvas, controls, ctx, dpr, fit, keepClear, narrow, project, reduced, yaw } from "./view.js";

export function draw(now) {
	const dt = Math.min(0.05, (now - play.lastFrame) / 1000);
	ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	ctx.clearRect(0, 0, W, H);

	drawFloor();
	drawFloorHeat(now);
	drawRing(now);
	drawRooms();
	drawAreas();
	drawWalls(now);

	ctx.globalCompositeOperation = "lighter";
	for (const d of DEVICES) drawFlow(d, now, dt);
	ctx.globalCompositeOperation = "source-over";

	layOut();
	if (folded.has(ROUTER)) floorMark(ROUTER);
	else drawStation(ROUTER, "#7fe7f2", 9, !stacked.has(ROUTER));
	for (const s of SENSORS) drawSensor(s, now);
	for (const d of DEVICES) drawNode(d, now);
	drawStacks();
	drawCallouts(now);
	drawHud(now);
	if (!reduced && now < play.glitchUntil) glitch(now);
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

// The areas in front of the house, marked out on the ground: the waiting
// one dashed, a person's drawn through; each named like a room, just behind
// it.
function drawAreas() {
	ctx.lineWidth = 1;
	for (const a of areas) {
		ctx.strokeStyle = a.compact ? "rgba(92,122,134,0.6)" : "rgba(127,231,242,0.3)";
		ctx.setLineDash(a.compact ? [4, 5] : []);
		path([[a.x, a.y], [a.x + a.w, a.y], [a.x + a.w, a.y + a.h], [a.x, a.y + a.h], [a.x, a.y]].map(([x, y]) => project(x, y)));
		ctx.stroke();
		const [x, y] = project(a.x, a.y - 10);
		ctx.font = `600 ${11 * fit * Math.min(cam.zoom, 1.6)}px "Avenir Next Condensed", "Arial Narrow", sans-serif`;
		ctx.fillStyle = "rgba(92,122,134,0.85)";
		ctx.fillText(a.name.toUpperCase().split("").join(" "), x, y);
	}
	ctx.setLineDash([]);
}

function drawWalls(now) {
	for (const d of wallDots) {
		const [x, y, k] = project(d.x, d.y, d.z);
		const tw = reduced ? 0.8 : 0.55 + 0.45 * Math.sin(now / 800 + d.phase);
		const a = d.top ? 0.95 : (0.18 + 0.5 * (d.lintel ? 1 : 1 - d.z / WALL_H)) * tw;
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
	if (reduced || play.mode !== "live") return;
	const t = (now - play.ringStart) / 1800;
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
	const extra = play.mode === "history" ? redness(s) : 0;
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

// A sensor where it stands, in its state's colour; a ring goes out when a
// new report has come in.
function drawSensor(s, now) {
	const at = sensorAt.get(s.name);
	if (!at) return;
	if (folded.has(s)) return floorMark(at);
	const [x, yTop] = drawStation({ ...at, name: s.name }, STATE_COLOR[sensorState(s)], 6, !stacked.has(s));
	s.screen = [x, yTop];
	const t = (now - s.beatAt) / 1400;
	if (t >= 0 && t < 1 && !reduced) {
		ctx.strokeStyle = `rgba(127,231,242,${(0.8 * (1 - t)).toFixed(3)})`;
		ctx.lineWidth = 1.2;
		ctx.beginPath();
		ctx.arc(x, yTop, 8 + 26 * t, 0, Math.PI * 2);
		ctx.stroke();
	}
}

function drawStation(s, color, size, named) {
	const [x, y] = project(s.x, s.y);
	const [, yTop] = project(s.x, s.y, STATION_LIFT);
	ctx.strokeStyle = color;
	ctx.lineWidth = 1;
	ctx.beginPath();
	ctx.moveTo(x, y);
	ctx.lineTo(x, yTop);
	ctx.stroke();
	ctx.fillStyle = "rgba(127,231,242,0.15)";
	ctx.fillRect(x - size, yTop - size, size * 2, size * 2);
	ctx.strokeRect(x - size, yTop - size, size * 2, size * 2);
	if (named) label(s.name.toUpperCase(), x + size + 6, yTop + 4, color, "display");
	return [x, yTop];
}

function drawNode(d, now) {
	let [x, y, k] = project(d.x, d.y, NODE_LIFT);
	const [fx, fy] = project(d.x, d.y, 0);
	d.screen = [x, y];
	const [tx, rx] = current(d, now);
	const t = heat(tx + rx);
	const glitching = outlierNow(d, now);
	const s = scoreNow(d, now);
	const color = glitching ? "#ff3b3b" : d.locationOnly ? "#5c7a86" : scoreColor(s);
	// kept for its stack, if it's in one
	Object.assign(d, { tint: color, hot: glitching, s, sub: d.locationOnly ? "location only" : `${fmtBytes(tx + rx)}/min` });
	if (folded.has(d)) return floorMark(d);

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

	// a device waiting to be placed is a dot in a crowd: named only when picked
	if ((d.waiting && selected !== d) || stacked.has(d)) return;
	label(d.short ?? d.name, x + 12, y - 3, glitching ? "#ff3b3b" : "rgba(232,244,246,0.8)");
	label(d.sub, x + 12, y + 10, "#5c7a86", "mono");
}

// A thing in a folded stack is only a faint mark on the floor where it
// stands; its stack's disc stands for it.
function floorMark(p) {
	const [x, y] = project(p.x, p.y, 0);
	ctx.fillStyle = "rgba(127,231,242,0.5)";
	ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
}

const NODE_LIFT = 14, STATION_LIFT = 26;
const NAME_FONT = "500 12px system-ui, sans-serif", SUB_FONT = "10px ui-monospace, Menlo, monospace";
const STATION_FONT = `600 11px "Avenir Next Condensed", "Arial Narrow", sans-serif`;

// The router, the sensors and the devices that are named on the plan, each
// { key, name, at (on screen), foot (below it, on the floor), and router,
// sensor or device }. Devices
// waiting in front of the house are dots without names: they don't stack.
function namedThings() {
	const spot = (p, lift) => ({ at: project(p.x, p.y, lift).slice(0, 2), foot: project(p.x, p.y, 0).slice(0, 2) });
	return [
		{ key: "router", name: ROUTER.name.toUpperCase(), ...spot(ROUTER, STATION_LIFT), router: ROUTER },
		...SENSORS.filter((s) => sensorAt.has(s.name)).map((s) =>
			({ key: `sensor ${s.name}`, name: s.name.toUpperCase(), ...spot(sensorAt.get(s.name), STATION_LIFT), sensor: s })),
		...DEVICES.filter((d) => !d.waiting).map((d) =>
			({ key: d.mac, name: d.short ?? d.name, ...spot(d, NODE_LIFT), device: d })),
	];
}

// A thing's name box on screen, its dot or square included. A device's has
// room for any rate under its name, so a changing rate doesn't restack.
function nameBox({ at: [x, y], name, device, router }) {
	if (!device) {
		const size = router ? 9 : 6;
		ctx.font = STATION_FONT;
		return [x - size, y - size, x + size + 6 + ctx.measureText(name).width, y + size];
	}
	ctx.font = SUB_FONT;
	const rate = ctx.measureText("999.9 KiB/min").width;
	ctx.font = NAME_FONT;
	return [x - 6, y - 12, x + 12 + Math.max(ctx.measureText(name).width, rate), y + 10];
}

const stackName = (ts) => `${ts[0].name} +${ts.length - 1}`;

// Which things stack, as lists of keys, from when the camera last settled.
let grouping = { key: null, groups: [] };
// This frame's stacks of two or more, each { things, and rows and left when
// fanned }, and the router, sensors and devices in them.
export let layout = [];
const stacked = new Set(), folded = new Set();
// A stack opened by a click on it stays open while the selection is what
// it was then; otherwise the selected device's stack is the one fanned.
const opened = { key: null, with: null };

export function openStack(key) {
	Object.assign(opened, { key, with: selected });
}

export function fold() {
	opened.key = null;
}

function layOut() {
	const things = namedThings();
	const cam = cameraKey();
	const key = cam && `${cam}|${things.map((t) => `${t.key}=${t.name}`).join()}|${things.map((t) => (t.device ?? t.router ?? sensorAt.get(t.sensor.name))).map((p) => `${p.x},${p.y}`).join()}`;
	if (key && key !== grouping.key) {
		const boxes = things.map(nameBox);
		const mark = (g) => {
			const [x, y] = things[g[0]].at;
			ctx.font = NAME_FONT;
			return [x - 10, y - 10, x + 14 + ctx.measureText(stackName(g.map((i) => things[i]))).width, y + 10];
		};
		grouping = { key, groups: stacks(boxes, mark).filter((g) => g.length > 1).map((g) => g.map((i) => things[i].key)) };
	}
	const byKey = new Map(things.map((t) => [t.key, t]));
	const fan = opened.key && opened.with === selected ? opened.key : selected?.mac;
	layout = grouping.groups.map((g) => ({ things: g.map((k) => byKey.get(k)).filter(Boolean) })).filter((s) => s.things.length > 1);
	stacked.clear();
	folded.clear();
	for (const s of layout) {
		for (const t of s.things) stacked.add(t.device ?? t.sensor ?? t.router);
		if (s.things.some((t) => t.key === fan)) {
			const { top, camera, side } = controls();
			Object.assign(s, fanColumn(s.things.map((t) => t.at), { row: FAN_ROW, nameW: rowWidth(s) + 9, w: side ? side.x : W, top: top.y + top.h, h: camera.y }));
		} else for (const t of s.things) folded.add(t.device ?? t.sensor ?? t.router);
	}
}

const FAN_ROW = 20;

// How wide a fanned row's text is: the name, and a device's rate after it.
function rowWidth(s) {
	return Math.max(...s.things.map((t) => {
		ctx.font = NAME_FONT;
		const name = ctx.measureText(t.name).width;
		ctx.font = SUB_FONT;
		return name + (t.device ? 8 + ctx.measureText("999.9 KiB/min").width : 0);
	}));
}

// A folded stack is a disc with the count on a stem above its first thing's
// spot, the things in it only marks on the floor (floorMark), named
// after it with "+2", ringed in the colour of its most unusual device (red
// while one is sending far more than usual), so an outlier can't hide in
// a cupboard. A fanned one draws its things in a column, each joined to
// its own spot.
function drawStacks() {
	for (const s of layout) {
		if (s.rows) drawFan(s);
		else drawFolded(s);
	}
}

function drawFolded({ things: ts }) {
	const devices = ts.map((t) => t.device).filter(Boolean);
	// devices still learning their usual have no score
	const scored = devices.filter((d) => d.s != null && !d.locationOnly);
	const worst = devices.find((d) => d.hot) ?? scored.reduce((a, d) => (d.s > a.s ? d : a), scored[0]);
	const color = worst?.hot ? "#ff3b3b" : worst ? worst.tint : "#7fe7f2";
	const [x, y] = ts[0].at, [fx, fy] = ts[0].foot;
	ctx.strokeStyle = "rgba(127,231,242,0.35)";
	ctx.lineWidth = 1;
	ctx.beginPath();
	ctx.moveTo(fx, fy);
	ctx.lineTo(x, y + 9);
	ctx.stroke();
	ctx.fillStyle = "rgba(4,10,14,0.9)";
	ctx.beginPath();
	ctx.arc(x, y, 9, 0, Math.PI * 2);
	ctx.fill();
	ctx.strokeStyle = color;
	ctx.lineWidth = 1.4;
	ctx.stroke();
	ctx.font = SUB_FONT;
	ctx.fillStyle = "#e8f4f6";
	ctx.textAlign = "center";
	ctx.fillText(String(ts.length), x, y + 3.5);
	ctx.textAlign = "left";
	label(stackName(ts), x + 14, y + 4, worst?.hot ? "#ff3b3b" : "rgba(232,244,246,0.8)");
}

function drawFan(s) {
	const { things: ts, rows, left } = s;
	const box = fanBox(s);
	ctx.fillStyle = "rgba(4,10,14,0.82)";
	ctx.fillRect(box.x, box.y, box.w, box.h);
	ctx.lineWidth = 1;
	ts.forEach((t, i) => {
		const [x, y] = t.at, [rx, ry] = rows[i];
		ctx.strokeStyle = "rgba(127,231,242,0.35)";
		ctx.beginPath();
		ctx.moveTo(x, y);
		ctx.lineTo(rx + (left ? 6 : -6), ry);
		ctx.stroke();
		const d = t.device;
		const color = d ? (d.hot ? "#ff3b3b" : d.tint) : t.router ? "#7fe7f2" : STATE_COLOR[sensorState(t.sensor)];
		ctx.fillStyle = ctx.strokeStyle = color;
		ctx.beginPath();
		if (d) {
			ctx.arc(rx, ry, 4, 0, Math.PI * 2);
			if (!d.locationOnly) ctx.fill();
			ctx.stroke();
		} else ctx.strokeRect(rx - 4, ry - 4, 8, 8);
		// the name, then a device's rate; left of the stack, ending at the dot
		ctx.font = NAME_FONT;
		const nameW = ctx.measureText(t.name).width;
		ctx.font = SUB_FONT;
		const subW = d ? 8 + ctx.measureText(d.sub).width : 0;
		const nx = left ? rx - 10 - subW - nameW : rx + 10;
		if (d) label(d.sub, nx + nameW + 8, ry + 4, "#5c7a86", "mono");
		label(t.name, nx, ry + 4, d === selected && d ? "#7fe7f2" : d?.hot ? "#ff3b3b" : "rgba(232,244,246,0.8)");
	});
}

// The box a fanned column's rows take, dots and names.
function fanBox({ rows, left, ...s }) {
	const w = rowWidth(s) + 18, ys = rows.map((r) => r[1]);
	return { x: left ? rows[0][0] - w + 8 : rows[0][0] - 8, y: Math.min(...ys) - FAN_ROW / 2, w, h: Math.max(...ys) - Math.min(...ys) + FAN_ROW };
}

// What a click at (x, y) lands on: { device }, { sensor, key } or
// { router } for a thing alone or a fanned row (its dot or its name);
// { stack: key } for a folded stack, its disc or any of its things; or null.
export function thingAt(x, y) {
	for (const s of layout) {
		if (!s.rows) continue;
		const w = rowWidth(s) + 10;
		const i = s.rows.findIndex(([rx, ry]) => Math.abs(y - ry) < FAN_ROW / 2 && (s.left ? x >= rx - w && x <= rx + 8 : x >= rx - 8 && x <= rx + w));
		if (i >= 0) return s.things[i];
	}
	const near = (p, r) => p && Math.hypot(p[0] - x, p[1] - y) < r;
	const sensor = SENSORS.find((s) => !stacked.has(s) && near(s.screen, 16));
	if (sensor) return { sensor, key: `sensor ${sensor.name}` };
	const folded = layout.find((s) => !s.rows && s.things.some((t) => near(t.at, 14)));
	if (folded) return { stack: folded.things[0].key };
	let best = null, bestDist = 24;
	for (const d of DEVICES) {
		if (stacked.has(d)) continue;
		const dist = Math.hypot(d.screen[0] - x, d.screen[1] - y);
		if (dist < bestDist) [best, bestDist] = [d, dist];
	}
	return best && { device: best };
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

// The callouts drawn this frame, boxes on the plan's canvas, for the layout
// check (crates/collector/ui-tests/layout.mjs).
export let drawnCallouts = [];

// A floating "profiler" box next to the selected device, typed in.
function drawCallouts(now) {
	const show = selected ? [selected] : [];
	for (const d of DEVICES) if (!show.includes(d)) d.shownAt = -1e9;
	drawnCallouts = [];

	const bw = 248, bh = 72, gap = 8;
	// the HUD, the controls over the plan and a fanned stack's column are taken
	const taken = keepClear(now);
	for (const s of layout.filter((s) => s.rows)) taken.push(fanBox(s));
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
		drawnCallouts.push({ text: d.name, left: box.x, top: box.y, right: box.x + bw, bottom: box.y + bh });
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

const HUD_M = 16, HUD_FONT = "10px ui-monospace, Menlo, monospace";
const DIM = "rgba(92,122,134,0.9)", LIT = "rgba(232,244,246,0.9)";

// The HUD's text and the occupancy's place on the canvas, for drawHud and
// what keeps clear of it. Texts are [text, x, baseline, colour]. On a phone
// the readouts fold into one line and the occupancy goes.
function hudLayout(now) {
	const m = HUD_M, x = m + 12;
	const total = DEVICES.reduce((s, d) => { const [a, b] = current(d, now); return s + a + b; }, 0);
	const fps = Math.round(total / 60 / 900) + 140;
	const rate = `${fmtBytes(total / 60)}/s`;
	if (narrow.matches) {
		return {
			text: [
				[`CH ${STATUS.channel} · ${fps} F/S · ${rate} · ${DEVICES.length} DEV · ${STATUS.dropped} DROP`, x, m + 14, LIT],
				[`YAW ${(yaw * 57.3).toFixed(1)}° · ZOOM ${cam.zoom.toFixed(1)}×`, x, H - m - 8, DIM],
			],
			occupancy: null,
		};
	}
	const rows = [
		["SNIFFER", `CH ${STATUS.channel} · 2.4 GHZ · 20 MHZ`],
		["FRAMES/S", String(fps)],
		["THROUGHPUT", rate],
		["SENSORS", String(STATUS.sensors)],
		["DEVICES", String(DEVICES.length)],
		["DROPPED", String(STATUS.dropped)],
	];
	const by = m + 26 + rows.length * 15 + 14;
	return {
		text: [
			...rows.flatMap(([k, v], i) => [[k, x, m + 26 + i * 15, DIM], [v, m + 92, m + 26 + i * 15, LIT]]),
			["2.4 GHZ OCCUPANCY", x, by - 6, DIM],
			[`VIEW  YAW ${(yaw * 57.3).toFixed(1).padStart(6)}°  TILT ${(TILT * 57.3).toFixed(0)}°  ZOOM ${cam.zoom.toFixed(1)}×`, x, H - m - 8, DIM],
		],
		occupancy: { x, y: by, w: 13 * 9, h: 30 },
	};
}

// What the HUD takes on the canvas, as drawHud draws it: each text (with
// its `text`, for the layout check), the occupancy and the corner brackets.
export function hudBoxes(now) {
	const m = HUD_M, { text, occupancy } = hudLayout(now);
	ctx.font = HUD_FONT;
	return [
		...text.map(([t, x, y]) => ({ text: t, x, y: y - 9, w: ctx.measureText(t).width, h: 12 })),
		...(occupancy ? [occupancy] : []),
		{ x: m, y: m, w: 22, h: 22 },
		{ x: W - m - 22, y: m, w: 22, h: 22 },
		{ x: m, y: H - m - 22, w: 22, h: 22 },
		{ x: W - m - 22, y: H - m - 22, w: 22, h: 22 },
	];
}

// The HUD around the plan: corner brackets, readouts, channel occupancy.
function drawHud(now) {
	const m = HUD_M;
	ctx.strokeStyle = "rgba(127,231,242,0.5)";
	ctx.lineWidth = 1;
	for (const [x, y, sx, sy] of [[m, m, 1, 1], [W - m, m, -1, 1], [W - m, H - m, -1, -1], [m, H - m, 1, -1]]) {
		ctx.beginPath();
		ctx.moveTo(x, y + sy * 22);
		ctx.lineTo(x, y);
		ctx.lineTo(x + sx * 22, y);
		ctx.stroke();
	}

	const { text, occupancy } = hudLayout(now);
	ctx.font = HUD_FONT;
	for (const [t, x, y, colour] of text) {
		ctx.fillStyle = colour;
		ctx.fillText(t, x, y);
	}

	// channel occupancy, 1 to 13: ours lit
	if (!occupancy) return;
	for (let ch = 1; ch <= 13; ch++) {
		const base = ch === STATUS.channel ? 0.55 : ch === 6 || ch === 1 ? 0.3 : 0.08;
		const v = Math.min(1, base + (reduced ? 0 : 0.08 * Math.sin(now / 300 + ch * 1.7)));
		const h = 4 + v * 26;
		ctx.fillStyle = ch === STATUS.channel ? "rgba(127,231,242,0.9)" : "rgba(127,231,242,0.25)";
		ctx.fillRect(occupancy.x + (ch - 1) * 9, occupancy.y + occupancy.h - h, 6, h);
	}
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
