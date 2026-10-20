// Drawing rooms (docs/floorplan.md, chunk 3): the plan from straight above,
// in metres, plain lines, with a panel on the right for the tool, the rooms
// (rename, delete) and what the keys do. Box: click two opposite corners.
// Lines: click the corners; click the first one, double-click or press Enter
// to close the room. Snapping and the checks are in geometry.js. The plan is
// saved whole through PUT /v1/plan.

import { atLength, box, canAdd, crossesItself, dist, labelAt, mm, roomWidth, snap } from "./geometry.js";
import { closePanel } from "./panel.js";
import { loadPlan, saved } from "./plan.js";
import { resize as resizeView } from "./view.js";

const app = document.querySelector(".app");
const editor = document.getElementById("editor");
const canvas = document.getElementById("edit-plan");
const ctx = canvas.getContext("2d");
const statusEl = document.getElementById("edit-status");
const roomsEl = document.getElementById("edit-rooms");
const helpEl = document.getElementById("edit-help");
const toolBtns = document.querySelectorAll("#edit-tools button");

// The collector's limit on corners per room.
const MAX_CORNERS = 100;
// How near, in screen pixels, a pointer snaps to a corner.
const REACH_PX = 10;
// Narrower than this (roomWidth, metres) isn't a room but a slip of the
// mouse, like a double-click on a corner just past the last one.
const MIN_WIDTH = 0.2;

const HELP = {
	box: [
		["click", "one corner, then the opposite one"],
		["type 4.2x3.1, Enter", "a room that size, towards the pointer"],
		["Esc", "drop the first corner"],
	],
	lines: [
		["click", "a corner"],
		["first corner, double-click, Enter", "close the room"],
		["type 4.2, Enter", "the next wall that long, towards the pointer"],
		["Backspace", "take back a corner"],
		["Esc", "drop the room"],
	],
	both: [
		["Alt", "no snapping"],
		["drag", "move the plan"],
		["scroll", "zoom"],
	],
};

export let editing = false;
// The tool, the rooms and openings so far, the corners of the room being
// drawn, where the next corner would go ({ at, how } from snap, or null), a
// typed size, unsaved changes, the room lit from the list.
let tool = "box";
let rooms = [], openings = [], chain = [], next = null, typed = "", dirty = false, lit = null;
// The last pointer position, to snap again when Alt or the corners change;
// when the last corner was put (a double-click that can't close takes it back).
let pointer = null, addedAt = 0;
// Metres to screen pixels: x * ppm + ox.
const cam = { ppm: 50, ox: 0, oy: 0 };
let W = 0, H = 0, dpr = 1;

const toScreen = ([x, y]) => [x * cam.ppm + cam.ox, y * cam.ppm + cam.oy];
const same = (a, b) => a && b && a[0] === b[0] && a[1] === b[1];

function say(text, bad = false) {
	statusEl.textContent = text;
	statusEl.classList.toggle("bad", bad);
}

function resize() {
	dpr = window.devicePixelRatio || 1;
	const r = canvas.getBoundingClientRect();
	[W, H] = [r.width, r.height];
	canvas.width = Math.round(W * dpr);
	canvas.height = Math.round(H * dpr);
}
window.addEventListener("resize", () => editing && resize());

// The rooms fitted to the screen, or 12 by 8 metres before there are any.
function fitView() {
	const pts = rooms.flatMap((r) => r.points);
	const xs = pts.length ? pts.map((p) => p[0]) : [0, 12];
	const ys = pts.length ? pts.map((p) => p[1]) : [0, 8];
	const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
	cam.ppm = Math.min((W - 120) / (x1 - x0), (H - 120) / (y1 - y0));
	cam.ox = W / 2 - ((x0 + x1) / 2) * cam.ppm;
	cam.oy = H / 2 - ((y0 + y1) / 2) * cam.ppm;
}

function openEditor() {
	closePanel();
	rooms = structuredClone(saved?.rooms ?? []);
	openings = structuredClone(saved?.openings ?? []);
	[chain, next, typed, dirty, pointer, lit] = [[], null, "", false, null, null];
	editing = true;
	editor.hidden = false;
	// the timeline makes room for the editor
	app.classList.add("editing");
	resize();
	fitView();
	setTool(tool);
	renderRooms();
	canvas.focus();
	say(rooms.length ? "" : "No plan saved yet: draw your first room.");
}

function closeEditor() {
	editing = false;
	editor.hidden = true;
	app.classList.remove("editing");
	resizeView();
}

function setTool(t) {
	[tool, chain, typed] = [t, [], ""];
	toolBtns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.tool === t)));
	helpEl.replaceChildren(...[...HELP[t], ...HELP.both].flatMap(([key, what]) => [el("dt", key), el("dd", what)]));
	aim();
}

function el(tag, text) {
	const e = document.createElement(tag);
	if (text !== undefined) e.textContent = text;
	return e;
}

// The rooms, each a name to edit and a delete button; pointing at one lights
// it up on the plan.
function renderRooms() {
	roomsEl.replaceChildren(
		...rooms.map((r, i) => {
			const li = el("li");
			const name = el("input");
			Object.assign(name, { value: r.name, maxLength: 64 });
			name.setAttribute("aria-label", "Room name");
			name.addEventListener("input", () => {
				r.name = name.value;
				dirty = true;
			});
			name.addEventListener("keydown", (ev) => ev.key === "Enter" && canvas.focus());
			const del = el("button", "×");
			del.className = "del";
			del.title = "Delete room";
			del.setAttribute("aria-label", `Delete ${r.name}`);
			del.addEventListener("click", () => deleteRoom(i));
			li.append(name, del);
			li.addEventListener("pointerenter", () => (lit = i));
			li.addEventListener("pointerleave", () => (lit = null));
			li.addEventListener("focusin", () => (lit = i));
			li.addEventListener("focusout", () => (lit = null));
			return li;
		}),
	);
	if (!rooms.length) roomsEl.append(el("li", "None yet."));
}

function deleteRoom(i) {
	const [gone] = rooms.splice(i, 1);
	// a doorway goes with the last room that has its edge
	const isEdge = (o, pts) => pts.some((a, k) => {
		const b = pts[(k + 1) % pts.length];
		return (same(a, o.from) && same(b, o.to)) || (same(a, o.to) && same(b, o.from));
	});
	openings = openings.filter((o) => rooms.some((r) => isEdge(o, r.points)));
	[lit, dirty] = [null, true];
	renderRooms();
	aim();
	say(`${gone.name} deleted.`);
}

// Where the next corner goes for the pointer: snapped, or with Alt held, as
// it is. The first corner of the room being drawn is a target once it can
// close; the last one always, so a double-click's second click lands on it.
// A box's second corner doesn't line up with its first (no width).
function aim() {
	if (!pointer) return (next = null);
	const p = [(pointer.x - cam.ox) / cam.ppm, (pointer.y - cam.oy) / cam.ppm];
	if (pointer.alt) return (next = { at: p.map(mm), how: "free" });
	const walls = rooms.flatMap((r) => r.points);
	const reach = REACH_PX / cam.ppm;
	if (tool === "box") return (next = snap(p, { corners: walls, reach }));
	const corners = [...walls, ...(chain.length >= 3 ? [chain[0]] : []), ...chain.slice(-1)];
	next = snap(p, { corners, align: [...walls, ...chain], last: chain.at(-1), prev: chain.at(-2), reach });
}

// Whether these corners make a room worth keeping; says why not.
function fine(points) {
	if (crossesItself(points)) return say("The room's lines cross: take back a corner (Backspace).", true), false;
	if (roomWidth(points) < MIN_WIDTH) return say("Too narrow for a room: carry on, or take back a corner.", true), false;
	return true;
}

function addRoom(points) {
	const ids = new Set(rooms.map((r) => r.id));
	let k = rooms.length + 1;
	while (ids.has(`room-${k}`)) k++;
	rooms.push({ id: `room-${k}`, name: `Room ${rooms.length + 1}`, points });
	[chain, typed, dirty] = [[], "", true];
	renderRooms();
	say(`Room ${rooms.length} drawn: name it on the right, or draw the next.`);
}

function click(p) {
	if (tool === "box") {
		if (!chain.length) return void (chain = [p]);
		const points = box(chain[0], p);
		if (fine(points)) addRoom(points);
		return;
	}
	if (chain.length >= 3 && same(p, chain[0])) return closeRoom();
	// the second click of a double-click
	if (same(p, chain.at(-1))) return;
	if (!canAdd(chain, p)) return say("That corner would cross the room's own lines.", true);
	if (chain.length >= MAX_CORNERS) return say(`A room has at most ${MAX_CORNERS} corners.`, true);
	chain.push(p);
	[typed, addedAt] = ["", performance.now()];
	say("");
}

// Whether the room being drawn was closed.
function closeRoom() {
	if (tool !== "lines" || chain.length < 3 || !fine(chain)) return false;
	addRoom(chain);
	return true;
}

// Enter: the typed size towards the pointer; in Lines with nothing typed,
// close the room.
function enter() {
	const last = chain.at(-1);
	if (tool === "lines" && !typed) return closeRoom();
	if (!typed || !last) return;
	if (!next || same(next.at, last)) return say("Point where it goes, then type its size.", true);
	if (tool === "lines") {
		const len = parseFloat(typed);
		if (len > 0) click(atLength(last, next.at, len));
		return;
	}
	// a box: width x height, or a width with the pointer's height
	const [w, h] = typed.split("x").map(parseFloat);
	const sx = Math.sign(next.at[0] - last[0]) || 1, sy = Math.sign(next.at[1] - last[1]) || 1;
	const hh = h > 0 ? h : Math.abs(next.at[1] - last[1]);
	if (!(w > 0) || !(hh > 0)) return say("Type a width and a height, like 4.2x3.1.", true);
	click([mm(last[0] + sx * w), mm(last[1] + sy * hh)]);
}

async function save() {
	if (chain.length) return say("Finish the room being drawn first, or drop it (Esc).", true);
	if (!rooms.length) return say("Draw a room first.", true);
	if (rooms.some((r) => !r.name.trim())) return say("Every room needs a name.", true);
	say("Saving…");
	try {
		const res = await fetch("/v1/plan", {
			method: "PUT",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ rooms, openings }),
		});
		if (!res.ok) return say(`Not saved: ${(await res.text()) || `HTTP ${res.status}`}`, true);
	} catch (e) {
		return say(`Not saved: ${e.message}`, true);
	}
	await loadPlan();
	closeEditor();
}

function cancel() {
	if (dirty && !window.confirm("Drop your changes to the plan?")) return;
	closeEditor();
}

export function drawEditor() {
	ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	ctx.clearRect(0, 0, W, H);

	// a line every metre, every 5 when zoomed far out
	const step = cam.ppm < 12 ? 5 : 1;
	ctx.strokeStyle = "rgba(127,231,242,0.07)";
	ctx.lineWidth = 1;
	ctx.beginPath();
	for (let x = Math.ceil(-cam.ox / cam.ppm / step) * step; x * cam.ppm + cam.ox < W; x += step) {
		const sx = Math.round(x * cam.ppm + cam.ox) + 0.5;
		ctx.moveTo(sx, 0);
		ctx.lineTo(sx, H);
	}
	for (let y = Math.ceil(-cam.oy / cam.ppm / step) * step; y * cam.ppm + cam.oy < H; y += step) {
		const sy = Math.round(y * cam.ppm + cam.oy) + 0.5;
		ctx.moveTo(0, sy);
		ctx.lineTo(W, sy);
	}
	ctx.stroke();

	ctx.font = '600 12px "Avenir Next Condensed", "Arial Narrow", sans-serif';
	rooms.forEach((r, i) => {
		const pts = r.points.map(toScreen);
		polygon(pts, true);
		ctx.fillStyle = i === lit ? "rgba(127,231,242,0.2)" : "rgba(127,231,242,0.05)";
		ctx.fill();
		ctx.strokeStyle = i === lit ? "#7fe7f2" : "rgba(127,231,242,0.7)";
		ctx.lineWidth = i === lit ? 2.5 : 1.5;
		ctx.stroke();
		ctx.fillStyle = "rgba(127,231,242,0.9)";
		for (const [x, y] of pts) ctx.fillRect(x - 2, y - 2, 4, 4);
		const [lx, ly] = labelAt(pts);
		ctx.fillStyle = i === lit ? "#e8f4f6" : "rgba(92,122,134,0.95)";
		ctx.fillText(r.name.toUpperCase().split("").join(" "), lx, ly);
	});

	const last = chain.at(-1);
	if (tool === "box") drawBox(last);
	else drawLines(last);
	if (!next) return;
	const [nx, ny] = toScreen(next.at);
	if (next.how === "corner") ring([nx, ny], 7, "#7fe7f2");
	else {
		ctx.fillStyle = "#7fe7f2";
		ctx.fillRect(nx - 2, ny - 2, 4, 4);
	}
}

// The box from its first corner to the pointer, and its size.
function drawBox(first) {
	if (!first) return;
	const [fx, fy] = toScreen(first);
	ctx.fillStyle = "#e8f4f6";
	ctx.fillRect(fx - 3, fy - 3, 6, 6);
	if (!next) return;
	const points = box(first, next.at);
	const ok = !crossesItself(points) && roomWidth(points) >= MIN_WIDTH;
	const pts = points.map(toScreen);
	polygon(pts, true);
	ctx.fillStyle = ok ? "rgba(232,244,246,0.06)" : "rgba(255,59,59,0.08)";
	ctx.fill();
	dashed(ok);
	const [w, h] = [Math.abs(next.at[0] - first[0]), Math.abs(next.at[1] - first[1])];
	const [x, y] = pts[2];
	label(typed ? `${typed}▏ m` : `${w.toFixed(2)} × ${h.toFixed(2)} m`, x + 10, y + (y > fy ? 18 : -10));
}

// The room being drawn, and the line to where the next corner would go.
function drawLines(last) {
	const pts = chain.map(toScreen);
	ctx.strokeStyle = "#e8f4f6";
	ctx.lineWidth = 2;
	polygon(pts, false);
	ctx.stroke();
	ctx.fillStyle = "#e8f4f6";
	for (const [x, y] of pts) ctx.fillRect(x - 3, y - 3, 6, 6);
	if (chain.length >= 3) ring(pts[0], 8, "rgba(232,244,246,0.5)");
	if (!last || !next) return;
	const closes = chain.length >= 3 && same(next.at, chain[0]);
	const ok = closes ? !crossesItself(chain) && roomWidth(chain) >= MIN_WIDTH : canAdd(chain, next.at);
	const [lx, ly] = toScreen(last);
	const [nx, ny] = toScreen(next.at);
	ctx.beginPath();
	ctx.moveTo(lx, ly);
	ctx.lineTo(nx, ny);
	dashed(ok);
	label(typed ? `${typed}▏ m` : `${dist(last, next.at).toFixed(2)} m`, (lx + nx) / 2 + 10, (ly + ny) / 2 - 10);
}

function polygon(pts, closed) {
	ctx.beginPath();
	pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
	if (closed) ctx.closePath();
}

// Strokes the current path dashed: white, or red when it can't be placed.
function dashed(ok) {
	ctx.strokeStyle = ok ? "rgba(232,244,246,0.8)" : "#ff3b3b";
	ctx.lineWidth = 1.5;
	ctx.setLineDash([6, 4]);
	ctx.stroke();
	ctx.setLineDash([]);
}

function label(text, x, y) {
	ctx.font = "12px ui-monospace, Menlo, monospace";
	ctx.fillStyle = typed ? "#7fe7f2" : "#e8f4f6";
	ctx.fillText(text, x, y);
}

function ring([x, y], r, color) {
	ctx.strokeStyle = color;
	ctx.lineWidth = 1.5;
	ctx.beginPath();
	ctx.arc(x, y, r, 0, Math.PI * 2);
	ctx.stroke();
}

// Click to put a corner, drag to move the plan, scroll to zoom.
let drag = null;
canvas.addEventListener("pointerdown", (ev) => {
	canvas.focus();
	drag = { x: ev.clientX, y: ev.clientY, moved: false };
	canvas.setPointerCapture(ev.pointerId);
});
canvas.addEventListener("pointermove", (ev) => {
	const r = canvas.getBoundingClientRect();
	pointer = { x: ev.clientX - r.left, y: ev.clientY - r.top, alt: ev.altKey };
	if (drag) {
		const dx = ev.clientX - drag.x, dy = ev.clientY - drag.y;
		if (drag.moved || Math.hypot(dx, dy) >= 4) {
			drag.moved = true;
			cam.ox += dx;
			cam.oy += dy;
			[drag.x, drag.y] = [ev.clientX, ev.clientY];
		}
	}
	aim();
});
canvas.addEventListener("pointerup", (ev) => {
	const isClick = drag && !drag.moved && ev.button === 0;
	drag = null;
	if (!isClick) return;
	const r = canvas.getBoundingClientRect();
	pointer = { x: ev.clientX - r.left, y: ev.clientY - r.top, alt: ev.altKey };
	aim();
	click(next.at);
	aim();
});
// The double-click's first click put a corner; if the room can't close, it
// goes again, so the double-click leaves nothing behind.
canvas.addEventListener("dblclick", () => {
	if (tool === "lines" && !closeRoom() && performance.now() - addedAt < 600) chain.pop();
	aim();
});
canvas.addEventListener("pointerleave", () => {
	pointer = null;
	next = null;
});
canvas.addEventListener("wheel", (ev) => {
	ev.preventDefault();
	const r = canvas.getBoundingClientRect();
	const [mx, my] = [ev.clientX - r.left, ev.clientY - r.top];
	const ppm = Math.max(5, Math.min(500, cam.ppm * Math.exp(-ev.deltaY * 0.0015)));
	cam.ox = mx - ((mx - cam.ox) * ppm) / cam.ppm;
	cam.oy = my - ((my - cam.oy) * ppm) / cam.ppm;
	cam.ppm = ppm;
	aim();
}, { passive: false });

// A typed number is the next size (a box's as 4.2x3.1); Backspace takes back
// a character or the last corner, Esc the typed size or the room being drawn.
document.addEventListener("keydown", (ev) => {
	if (!editing || ev.target.closest?.("button, input, textarea")) return;
	const key = ev.key === "," ? "." : ev.key === "*" || ev.key === "X" ? "x" : ev.key;
	const part = typed.split("x").at(-1);
	if (/^[0-9]$/.test(key) || (key === "." && !part.includes(".")) || (key === "x" && tool === "box" && typed && !typed.includes("x"))) {
		if (chain.length) typed += key;
	} else if (key === "Enter") enter();
	else if (key === "Backspace") typed ? (typed = typed.slice(0, -1)) : chain.pop();
	else if (key === "Escape") typed ? (typed = "") : (chain = []);
	else if (key === "Alt") pointer && (pointer.alt = true);
	else return;
	ev.preventDefault();
	aim();
});
document.addEventListener("keyup", (ev) => {
	if (editing && ev.key === "Alt" && pointer) {
		pointer.alt = false;
		aim();
	}
});

document.getElementById("edit-btn").addEventListener("click", openEditor);
document.getElementById("edit-save").addEventListener("click", save);
document.getElementById("edit-cancel").addEventListener("click", cancel);
toolBtns.forEach((b) => b.addEventListener("click", () => {
	setTool(b.dataset.tool);
	canvas.focus();
}));
