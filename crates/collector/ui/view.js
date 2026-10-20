// The 3D view: plan units in, screen pixels out; the camera, and moving it
// with the mouse, the keys and the buttons.

import { DEVICES, SENSORS } from "./data.js";
import { select } from "./panel.js";
import { PLAN, WALL_H } from "./plan.js";
import { play } from "./playback.js";
import { openSensor } from "./sensors.js";
import { redrawGraph } from "./timeline.js";

export const canvas = document.getElementById("plan");
export const ctx = canvas.getContext("2d");
export const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export const TILT = 0.88; // radians from looking straight down (about 50°)
const DIST = 1700; // camera distance: smaller is more perspective
export let yaw = 0;
// The user's camera on top of the fit: zoom and pan (screen pixels) and a
// yaw target. Each eases towards its target; a drag sets both at once.
// Until the user touches the view, the yaw drifts slowly by itself.
export const cam = { zoom: 1, panX: 0, panY: 0 };
const camT = { zoom: 1, panX: 0, panY: 0, yaw: 0 };
let touched = false;
export let dpr = 1, W = 0, H = 0, fit = 1, cx = 0, cy = 0;

function view(x, y, z) {
	const X = x - PLAN.w / 2, Y = y - PLAN.h / 2;
	const c = Math.cos(yaw), s = Math.sin(yaw);
	const x1 = X * c - Y * s, y1 = X * s + Y * c;
	const depth = DIST - (y1 * Math.sin(TILT) + z * Math.cos(TILT));
	const k = DIST / depth;
	return [x1 * k, (y1 * Math.cos(TILT) - z * Math.sin(TILT)) * k, k];
}

export function project(x, y, z = 0) {
	const [vx, vy, k] = view(x, y, z);
	const f = fit * cam.zoom;
	return [cx + cam.panX + vx * f, cy + cam.panY + vy * f, k];
}

export function resize() {
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
	redrawGraph();
}
window.addEventListener("resize", resize);

export function moveCamera(now) {
	if (!touched) camT.yaw = reduced ? 0 : Math.sin(now / 21000) * 0.12;
	const a = reduced ? 1 : 1 - Math.exp(-((now - play.lastFrame) / 1000) * 12);
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

// pick a device by clicking near its node, or a sensor
canvas.addEventListener("click", (ev) => {
	if (dragged) {
		dragged = false;
		return;
	}
	const r = canvas.getBoundingClientRect();
	const x = ev.clientX - r.left, y = ev.clientY - r.top;
	const sensor = SENSORS.find((s) => s.screen && Math.hypot(s.screen[0] - x, s.screen[1] - y) < 16);
	if (sensor) return openSensor(sensor.name);
	let best = null, bestDist = 24;
	for (const d of DEVICES) {
		const dist = Math.hypot(d.screen[0] - x, d.screen[1] - y);
		if (dist < bestDist) [best, bestDist] = [d, dist];
	}
	// a click on an empty spot closes the panel
	select(best);
});
