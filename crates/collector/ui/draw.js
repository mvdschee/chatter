// Drawing the plan, each frame: the floor, walls and rooms, the traffic as
// glows and packets on arcs to the router, the devices, the sensor, the
// selected device's box and the HUD.

import { heat, redness, score, scoreColor } from "./colour.js";
import { DEVICES, SENSORS, STATUS } from "./data.js";
import { fmtBytes } from "./format.js";
import { panel, selected } from "./panel.js";
import { ROUTER, SENSOR, WALL_H, floorDots, rooms, wallDots } from "./plan.js";
import { current, outlierNow, play, scoreNow } from "./playback.js";
import { rand } from "./random.js";
import { STATE_COLOR, sensorState } from "./sensors.js";
import { H, TILT, W, cam, canvas, ctx, dpr, fit, project, reduced, yaw } from "./view.js";

export function draw(now) {
	const dt = Math.min(0.05, (now - play.lastFrame) / 1000);
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
