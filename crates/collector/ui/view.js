// The 3D view: plan units in, screen pixels out; the camera, and moving it
// with the mouse, the keys and the buttons.

import { fold, hudBoxes, openStack, thingAt } from "./draw.js";
import { fitFrame } from "./geometry.js";
import { select, side } from "./panel.js";
import { PLAN, WALL_H } from "./plan.js";
import { play } from "./playback.js";
import { openSensor } from "./sensors.js";
import { redrawGraph } from "./timeline.js";

export const canvas = document.getElementById("plan");
export const ctx = canvas.getContext("2d");
const topBar = document.querySelector(".top");
// the buttons, not .view-ctl: its hint is see-through but still takes room
const cameraBtns = document.querySelector(".view-ctl .icon-group");
export const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
// a phone's layout, the same width as style.css's
export const narrow = window.matchMedia("(max-width: 900px)");

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
	// setting a canvas's size clears it, even to the same size: a refit
	// alone, after the frame is drawn, would blank it until the next
	const [cw, ch] = [Math.round(W * dpr), Math.round(H * dpr)];
	if (canvas.width !== cw || canvas.height !== ch) [canvas.width, canvas.height] = [cw, ch];
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
	// clear of the HUD and the controls, but not the side panel: the plan
	// would jump each time it opens
	const { top, camera } = controls();
	const f = fitFrame(maxX - minX, maxY - minY, { w: W, h: H, clear: [...hudBoxes(performance.now()), top, camera] });
	fit = f.fit;
	cx = f.x - ((minX + maxX) / 2) * fit;
	cy = f.y - ((minY + maxY) / 2) * fit;
	redrawGraph();
}
window.addEventListener("resize", resize);
// the top bar widens once the fonts and the sensors' status are in
const watch = new ResizeObserver(() => resize());
watch.observe(topBar);
watch.observe(cameraBtns);
// on a phone the bottom sheet takes room from the plan
watch.observe(canvas);

// The controls over the plan, as boxes on its canvas ({ x, y, w, h }): the
// top bar, the camera's buttons and the side panel while it's open.
export function controls() {
	const c = canvas.getBoundingClientRect();
	const box = (el) => {
		const r = el.getBoundingClientRect();
		return { x: r.left - c.left, y: r.top - c.top, w: r.width, h: r.height };
	};
	return { top: box(topBar), camera: box(cameraBtns), side: side.classList.contains("open") ? box(side) : null };
}

// What a label on the plan keeps clear of: the HUD and the controls.
export function keepClear(now) {
	const { top, camera, side } = controls();
	return [...hudBoxes(now), top, camera, ...(side ? [side] : [])];
}

export function moveCamera(now) {
	if (!touched) camT.yaw = reduced ? 0 : Math.sin(now / 21000) * 0.12;
	const a = reduced ? 1 : 1 - Math.exp(-((now - play.lastFrame) / 1000) * 12);
	yaw += (camT.yaw - yaw) * a;
	for (const k of ["zoom", "panX", "panY"]) cam[k] += (camT[k] - cam[k]) * a;
}

// Where the camera is headed, once it's there: stacks are worked out again
// when this changes, not while the yaw drifts by itself or the camera eases,
// or names at the edge of overlapping would stack and unstack. Null while
// it's moving.
export function cameraKey() {
	const there = Math.abs(cam.zoom - camT.zoom) < 0.005 && Math.hypot(cam.panX - camT.panX, cam.panY - camT.panY) < 1
		&& (!touched || Math.abs(yaw - camT.yaw) < 0.002);
	return there ? [camT.zoom, camT.panX, camT.panY, touched ? camT.yaw : "drift", W, H, fit].join() : null;
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

// Drag to pan; shift-drag or right-drag to rotate. Two fingers pinch to
// zoom, move to pan and twist to rotate, about their middle. A drag or a
// pinch isn't a click.
const pointers = new Map();
let drag = null, dragged = false;
canvas.addEventListener("pointerdown", (ev) => {
	canvas.setPointerCapture(ev.pointerId);
	pointers.set(ev.pointerId, [ev.clientX, ev.clientY]);
	if (pointers.size === 1) drag = { x: ev.clientX, y: ev.clientY, rotate: ev.shiftKey || ev.button === 2, moved: false };
	else if (drag) drag.moved = true;
});
canvas.addEventListener("pointermove", (ev) => {
	if (!drag || !pointers.has(ev.pointerId)) return;
	if (pointers.size > 1) {
		const [a, b] = [...pointers.values()];
		pointers.set(ev.pointerId, [ev.clientX, ev.clientY]);
		const [a1, b1] = [...pointers.values()];
		pinch(a, b, a1, b1);
		return;
	}
	pointers.set(ev.pointerId, [ev.clientX, ev.clientY]);
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
const lift = (ev) => {
	pointers.delete(ev.pointerId);
	// the finger left after a pinch drags on from where it is
	if (pointers.size === 1 && drag) [drag.x, drag.y] = [...pointers.values()][0];
	if (pointers.size) return;
	dragged = drag?.moved ?? false;
	drag = null;
	canvas.classList.remove("dragging");
};
canvas.addEventListener("pointerup", lift);
canvas.addEventListener("pointercancel", lift);

// Two fingers from a, b to a1, b1 (client px): the zoom, the twist and the
// move of their middle, applied at once so the plan follows the fingers.
function pinch([ax, ay], [bx, by], [ax1, ay1], [bx1, by1]) {
	const d0 = Math.hypot(bx - ax, by - ay), d1 = Math.hypot(bx1 - ax1, by1 - ay1);
	if (d0 < 1 || d1 < 1) return;
	const r = canvas.getBoundingClientRect();
	const [mx, my] = [(ax + bx) / 2 - r.left, (ay + by) / 2 - r.top];
	const [mx1, my1] = [(ax1 + bx1) / 2 - r.left, (ay1 + by1) / 2 - r.top];
	zoomAt(mx, my, d1 / d0);
	camT.panX += mx1 - mx;
	camT.panY += my1 - my;
	// the shorter way round: across ±180° the angles jump by a full turn
	const twist = Math.atan2(by1 - ay1, bx1 - ax1) - Math.atan2(by - ay, bx - ax);
	camT.yaw += twist - Math.round(twist / (2 * Math.PI)) * 2 * Math.PI;
	[cam.zoom, cam.panX, cam.panY, yaw] = [camT.zoom, camT.panX, camT.panY, camT.yaw];
}
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

// pick a device by clicking near its node, or a sensor; a click on a stack
// fans it, and one on a fanned row picks that thing
canvas.addEventListener("click", (ev) => {
	if (dragged) {
		dragged = false;
		return;
	}
	const r = canvas.getBoundingClientRect();
	const hit = thingAt(ev.clientX - r.left, ev.clientY - r.top);
	if (hit?.stack) return openStack(hit.stack);
	if (hit?.router) return;
	if (hit?.sensor) {
		openSensor(hit.sensor.name);
		return openStack(hit.key);
	}
	// a click on an empty spot closes the panel and folds the fan
	fold();
	select(hit?.device ?? null);
});
document.addEventListener("keydown", (ev) => ev.key === "Escape" && fold());
