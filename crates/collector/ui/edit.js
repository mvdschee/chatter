// Drawing rooms (docs/floorplan.md, chunk 3): the plan from straight above,
// in metres, plain lines, with a panel on the right for the tool, the rooms
// (rename, delete) and what the keys do. Box: click two opposite corners.
// Lines: click the corners; click the first one, double-click or press Enter
// to close the room. Snapping and the checks are in geometry.js. The plan is
// saved whole through PUT /v1/plan.
// The underlay (chunk 4): an image of the floor plan to trace over, light
// lines on the dark canvas. Scale: click both ends of a wall the image gives
// the length of, type it. The image goes to PUT /v1/plan/underlay on save,
// where it lies goes in the plan. Turn: the whole plan a quarter round. The
// steps are a bar across the top, the next one lit; the panel shows what the
// tool needs, and is as wide as its edge is dragged.
// Placing (chunk 7): the router, the sensors and the devices, each placed,
// carried by someone, or waiting; saved in the plan as `places`. Things whose
// names would overlap stack, and a stack fans out as a column (chunk 8).
// Editing (chunk 5): drag a corner in Box or Lines, and it moves in every
// room that has it; undo and redo take back any change to the plan.
// Doors (chunk 6): click a wall for a doorway, drag its ends or its middle.

import { atLength, box, canAdd, crossesItself, dist, doorAt, doorSpan, doorsOverlap, fanColumn, followImage, fromImage, inside, labelAt, measure, mm, moveCorner, nearestEdge, resizeDoor, roomAt as roomIn, roomWidth, snap, stacks, turnPlan } from "./geometry.js";
import { DEVICES, SENSORS, renameDevice } from "./data.js";
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
const guideEl = document.getElementById("edit-guide");
const roomsPart = document.getElementById("edit-rooms-part");
const handle = document.getElementById("edit-handle");
const toolBtns = document.querySelectorAll("#editor [data-tool]");
const fileInput = document.getElementById("underlay-file");
const scaleBtn = document.querySelector('#editor [data-tool="scale"]');
const showBtn = document.getElementById("underlay-show");
const removeBtn = document.getElementById("underlay-remove");
const turnBtns = [document.getElementById("turn-left"), document.getElementById("turn-right")];
const stepEls = document.querySelectorAll("#edit-steps [data-step]");
const thingsEl = document.getElementById("edit-things");
const thingsList = document.getElementById("things-list");
const thingDetail = document.getElementById("thing-detail");
const filterEl = document.getElementById("things-filter");
const carriersEl = document.getElementById("carriers");

// The collector's limit on corners per room.
const MAX_CORNERS = 100;
// How near, in screen pixels, a pointer snaps to a corner.
const REACH_PX = 10;
// Narrower than this (roomWidth, metres) isn't a room but a slip of the
// mouse, like a double-click on a corner just past the last one.
const MIN_WIDTH = 0.2;
// The collector's limit on an underlay image, and the types it takes.
const MAX_IMAGE = 20 * 1024 * 1024;
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp"];
// The underlay is drawn from a copy at most this many pixels on its longer
// side, and this see-through.
const GHOST_PX = 4096;
const GHOST_ALPHA = 0.55;
// How many changes undo goes back.
const MAX_UNDO = 100;
// A new doorway's width, and the steps a doorway moves and sizes in, metres.
const DOOR_W = 0.9;
const DOOR_STEP = 0.05;
// The panel's usual and least width, pixels; the CSS keeps it to 60% of the
// editor at most. Where its width is kept between visits.
const SIDE_W = 340;
const SIDE_MIN = 300;
const SIDE_KEY = "chatter.edit-side";

// Each step's name and how to do it, for the top of the panel.
const GUIDE = {
	image: ["Load the floor plan", "An image of it to trace over: PNG, JPEG or WebP (export a PDF page as PNG). No floor plan? Skip to step 4."],
	scale: ["Measure a wall", "Zoom in. If the plan has a scale bar, click its 0 and its last mark and type that length. Otherwise pick the longest size the plan gives, click the inside faces of the two walls it runs between, and type it in metres (4200 mm is 4.2). Press Enter. Measuring again later moves the traced rooms along."],
	turn: ["Turn it", "If you like: turn the plan a quarter at a time until it's the way round you want to see it, the long side across. The app shows it this way. Or go on to step 4."],
	rooms: ["Trace the rooms", "Box for rectangular rooms, Lines for the rest, along the middle of the walls. Rooms that touch share their corners. Drag a corner to move it. Doors: click a wall where a doorway is, drag its ends to size it. Name each room in the list below."],
	place: ["Place things", "Put the router, the sensors and the devices where they stand: pick one in the list, click its spot. A laptop or phone that moves around: say who carries it. What you don't place waits in front of the house."],
};
// The step each tool belongs to.
const STEP_OF = { box: "rooms", lines: "rooms", door: "rooms", scale: "scale", place: "place" };

const HELP = {
	box: [
		["click", "one corner, then the opposite one"],
		["type 4.2x3.1, Enter", "a room that size, towards the pointer"],
		["drag a corner", "move it, in every room that has it"],
		["Esc", "drop the first corner"],
	],
	lines: [
		["click", "a corner"],
		["first corner, double-click, Enter", "close the room"],
		["type 4.2, Enter", "the next wall that long, towards the pointer"],
		["Backspace", "take back a corner"],
		["drag a corner", "move it, in every room that has it"],
		["Esc", "drop the room"],
	],
	scale: [
		["click", "the scale bar's ends, or the inside faces of two walls a size runs between"],
		["type 4.2, Enter", "its length: the image scales to it, and turns square if the wall leans a little"],
		["Esc", "start again"],
	],
	door: [
		["click a wall", `a doorway ${DOOR_W.toFixed(2)} m wide there`],
		["drag a doorway", "slide it along its wall"],
		["drag its end", "make it wider or narrower"],
		["type 0.8, Enter", "the picked doorway that wide"],
		["Backspace", "remove the picked doorway"],
		["Esc", "pick none"],
	],
	place: [
		["click", "put the picked one there; the next one waiting is picked"],
		["drag a placed one", "move it"],
		["click a number", "spread out the things standing there"],
		["Esc", "fold them again, or pick nothing"],
	],
	both: [
		["⌘Z, Ctrl+Z", "undo"],
		["⇧⌘Z, Ctrl+Y", "redo"],
		["Alt", "no snapping"],
		["drag", "move the plan"],
		["scroll", "zoom"],
	],
};

export let editing = false;
// The tool, the rooms and openings so far, the corners of the room being
// drawn, where the next corner would go ({ at, how } from snap, or null), a
// typed size, unsaved changes, the room lit from the list.
let tool = "box", drawTool = "box";
let rooms = [], openings = [], chain = [], next = null, typed = "", dirty = false, lit = null;
// Where the underlay lies ({ pixels_per_m, offset, angle }, or null), the
// image to draw (from ghostOf), a new image not saved yet, whether it shows;
// loads counts image loads, so only the latest one lands. Whether the image
// was measured and the plan turned, for the steps; stepsKey is what they
// last showed.
let underlay = null, ghost = null, imageFile = null, shown = true, loads = 0;
let measured = false, turned = false, stepsKey = "";
// The saved underlay's image once loaded, which undo brings back with it.
let stored = null;
// The doorway picked in Door, an index into openings, or null.
let door = null;
// The plan before each change, for undo, and the changes undone, for redo.
let history = [], future = [];
// Where things stand ({ router, sensors, devices }, devices not seen lately
// kept), the one picked in the list (a key from things()), the name last
// given as who carries a device; a thing whose stack was clicked open.
let places = noPlaces(), picked = null, carrier = "", opened = null;
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

// The rooms and the underlay fitted to the screen, or 12 by 8 metres before
// there are any.
function fitView() {
	const { w, h } = ghost ?? {};
	const image = ghost && underlay ? [[0, 0], [w, 0], [w, h], [0, h]].map((p) => fromImage(underlay, p)) : [];
	const pts = [...rooms.flatMap((r) => r.points), ...image];
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
	[underlay, ghost, imageFile, shown] = [structuredClone(saved?.underlay ?? null), null, null, true];
	[measured, turned] = [Boolean(underlay), false];
	places = { ...noPlaces(), ...structuredClone(saved?.places ?? {}) };
	[picked, opened, filterEl.value] = [null, null, ""];
	[history, future, stored, door] = [[], [], null, null];
	if (underlay) {
		const n = ++loads;
		loadImage(`/v1/plan/underlay?${Date.now()}`)
			.then((g) => {
				stored = g;
				if (n !== loads) return;
				ghost = g;
				fitView();
				aim();
			})
			.catch(() => n === loads && say("The underlay image didn't load: pick it again, or remove it.", true));
	}
	editing = true;
	editor.hidden = false;
	setSideWidth(Number(localStorage.getItem(SIDE_KEY)) || SIDE_W);
	// the timeline makes room for the editor
	app.classList.add("editing");
	resize();
	fitView();
	setTool(tool);
	renderRooms();
	renderThings();
	canvas.focus();
	say("");
}

function closeEditor() {
	editing = false;
	editor.hidden = true;
	app.classList.remove("editing");
	resizeView();
}

function setTool(t) {
	if (t === "scale" && !underlay) t = drawTool;
	if (t !== "scale") drawTool = t;
	[tool, chain, typed, door] = [t, [], "", null];
	toolBtns.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.tool === t)));
	thingsEl.hidden = t !== "place";
	roomsPart.hidden = STEP_OF[t] !== "rooms";
	// into Place with nothing picked: the first one waiting
	if (t === "place" && !picked) pick(things().find((x) => !placeOf(x.key))?.key ?? null);
	helpEl.replaceChildren(...[...HELP[t], ...HELP.both].flatMap(([key, what]) => [el("dt", key), el("dd", what)]));
	aim();
}

// The panel as wide as asked, from SIDE_MIN on; the plan keeps its place.
function setSideWidth(w) {
	w = Math.round(Math.min(Math.max(w, SIDE_MIN), editor.clientWidth * 0.6));
	editor.style.setProperty("--edit-side-w", `${w}px`);
	localStorage.setItem(SIDE_KEY, String(w));
	resize();
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
			// one undo for a whole rename, not one per letter
			let typing = false;
			name.addEventListener("input", () => {
				if (!typing) remember();
				typing = true;
				r.name = name.value;
				dirty = true;
			});
			name.addEventListener("change", () => (typing = false));
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
	remember();
	const [gone] = rooms.splice(i, 1);
	// a doorway goes with the last room that has its edge
	const isEdge = (o, pts) => pts.some((a, k) => {
		const b = pts[(k + 1) % pts.length];
		return (same(a, o.from) && same(b, o.to)) || (same(a, o.to) && same(b, o.from));
	});
	openings = openings.filter((o) => rooms.some((r) => isEdge(o, r.points)));
	[lit, dirty, door] = [null, true, null];
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
	// a doorway goes on the wall in reach, where along it the pointer is
	if (tool === "door") {
		const edge = nearestEdge(rooms, p, REACH_PX / cam.ppm);
		return (next = edge && { at: atLength(edge.from, edge.to, edge.t), how: "edge", edge });
	}
	if (pointer.alt || tool === "scale" || tool === "place") return (next = { at: p.map(mm), how: "free" });
	const walls = rooms.flatMap((r) => r.points);
	const reach = REACH_PX / cam.ppm;
	// a corner being dragged snaps to the others, never to itself
	if (drag?.corner) {
		const others = walls.filter((c) => !same(c, drag.corner));
		return (next = snap(p, { corners: others, reach }));
	}
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
	remember();
	const ids = new Set(rooms.map((r) => r.id));
	let k = rooms.length + 1;
	while (ids.has(`room-${k}`)) k++;
	rooms.push({ id: `room-${k}`, name: `Room ${rooms.length + 1}`, points });
	[chain, typed, dirty] = [[], "", true];
	renderRooms();
	say(`Room ${rooms.length} drawn: name it on the right, or draw the next.`);
}

function click(p) {
	if (tool === "place") return placePicked(p);
	if (tool === "scale") {
		chain = chain.length === 1 ? [chain[0], p] : [p];
		typed = "";
		if (chain.length === 2) say("How long is that wall? Type it in metres, then Enter.");
		return;
	}
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
	if (tool === "scale") return measureWall();
	if (tool === "door") return sizeDoor();
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

// Scale: the two points are the ends of a wall as long as typed.
function measureWall() {
	const len = parseFloat(typed);
	if (chain.length < 2 || !(len > 0)) return say("Click both ends of a wall, then type its length.", true);
	if (same(chain[0], chain[1])) return say("Those are the same point: click the wall's other end.", true);
	const before = underlay;
	remember();
	underlay = measure(underlay, chain[0], chain[1], len);
	// measured before: what's traced is on the image, and goes with it
	const carried = measured && rooms.length > 0;
	if (carried) ({ rooms, openings, places } = followImage({ rooms, openings, places }, before, underlay));
	[measured, dirty] = [true, true];
	setTool(drawTool);
	// the whole plan in sight, to see which way round it should go
	fitView();
	aim();
	say(carried ? "Measured again: the rooms moved with the image. Point at a room to check its walls." : "Measured. Measuring a longer wall makes it more exact.");
}

// The image as light lines on nothing, for the dark canvas: white paper goes
// clear, dark lines light. { canvas, w, h }: w and h are the image's own
// size, which the underlay's pixels count in.
function ghostOf(img) {
	const [w, h] = [img.naturalWidth, img.naturalHeight];
	const s = Math.min(1, GHOST_PX / Math.max(w, h));
	const c = document.createElement("canvas");
	[c.width, c.height] = [Math.round(w * s), Math.round(h * s)];
	const g = c.getContext("2d");
	g.drawImage(img, 0, 0, c.width, c.height);
	const data = g.getImageData(0, 0, c.width, c.height);
	const d = data.data;
	for (let i = 0; i < d.length; i += 4) {
		// paper a little off white (a photo) goes clear too
		const ink = Math.max(0, 215 - (0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2])) / 215;
		[d[i], d[i + 1], d[i + 2], d[i + 3]] = [232, 244, 246, ink * d[i + 3]];
	}
	g.putImageData(data, 0, 0);
	return { canvas: c, w, h };
}

async function loadImage(src) {
	const img = new Image();
	img.src = src;
	await img.decode();
	return ghostOf(img);
}

// A new image, laid over the middle of the screen until it's measured.
async function pickImage(file) {
	if (!file) return;
	if (!IMAGE_TYPES.includes(file.type)) return say("Use a PNG, JPEG or WebP image: export a PDF page as PNG first.", true);
	if (file.size > MAX_IMAGE) return say("That image is over 20 MB: save it smaller first.", true);
	const url = URL.createObjectURL(file);
	const n = ++loads;
	let g;
	try {
		g = await loadImage(url);
	} catch {
		return say("This browser can't read that image.", true);
	} finally {
		URL.revokeObjectURL(url);
	}
	if (n !== loads) return;
	const k = Math.max(g.w / ((W * 0.8) / cam.ppm), g.h / ((H * 0.8) / cam.ppm));
	const mid = [(W / 2 - cam.ox) / cam.ppm, (H / 2 - cam.oy) / cam.ppm];
	remember();
	underlay = { pixels_per_m: k, offset: [mm(mid[0] - g.w / k / 2), mm(mid[1] - g.h / k / 2)], angle: 0 };
	[ghost, imageFile, shown, dirty, measured] = [g, file, true, true, false];
	setTool("scale");
	say("");
}

function removeUnderlay() {
	remember();
	[underlay, ghost, imageFile, dirty, measured] = [null, null, null, true, false];
	loads++;
	if (tool === "scale") setTool(drawTool);
	say("Underlay removed.");
}

// The whole plan a quarter round, clockwise for 1: rooms, doorways, places
// and the image together, so what's traced stays on it.
function turn(dir) {
	if (chain.length && tool !== "scale") return say("Finish the room being drawn first, or drop it (Esc).", true);
	remember();
	({ rooms, openings, underlay, places } = turnPlan({ rooms, openings, underlay, places }, dir));
	[chain, typed, turned, dirty] = [[], "", true, true];
	fitView();
	aim();
	say("");
}

// The steps in the panel: each done or not, the next one lit, and the
// buttons that can't do anything yet off. The next step is the first not
// done; with rooms and no image, the image steps are skipped.
function updateSteps() {
	const now = !underlay && !rooms.length ? "image"
		: underlay && !measured ? "scale"
		: !turned && !rooms.length ? "turn"
		: !rooms.length ? "rooms"
		: !anyPlaced() ? "place"
		: "save";
	const done = { image: Boolean(underlay), scale: Boolean(underlay) && measured, turn: turned, rooms: rooms.length > 0, place: anyPlaced(), save: false };
	const key = JSON.stringify([now, done, shown, tool]);
	if (key === stepsKey) return;
	stepsKey = key;
	// the tool's step, after the next one while the plan is being set up
	const own = STEP_OF[tool];
	const guide = ["image", "scale", "turn"].includes(now) && now !== own ? [now, own] : [own];
	guideEl.replaceChildren(...guide.map((step) => {
		const d = el("div");
		d.classList.toggle("next", step === now);
		d.append(el("b", GUIDE[step][0]), el("p", GUIDE[step][1]));
		return d;
	}));
	stepEls.forEach((li) => {
		li.classList.toggle("now", li.dataset.step === now);
		li.classList.toggle("done", li.dataset.step !== now && done[li.dataset.step]);
		if (li.dataset.step === now) li.setAttribute("aria-current", "step");
		else li.removeAttribute("aria-current");
	});
	scaleBtn.disabled = showBtn.disabled = removeBtn.disabled = !underlay;
	showBtn.setAttribute("aria-pressed", String(shown));
	turnBtns.forEach((b) => (b.disabled = !underlay && !rooms.length));
}

// --- undo ---

// Everything a change to the plan can touch. A new image not saved yet goes
// along with its file; the saved one is `stored`.
function snapshot() {
	const plan = structuredClone({ rooms, openings, underlay, places });
	return { ...plan, measured, turned, imageFile, ghost: imageFile ? ghost : null };
}

// Called before each change: the plan as it was, for undo. A new change
// can't be redone over.
function remember() {
	history.push(snapshot());
	if (history.length > MAX_UNDO) history.shift();
	future = [];
}

function restore(s) {
	({ rooms, openings, underlay, places, measured, turned, imageFile } = s);
	ghost = imageFile ? s.ghost : underlay ? stored : null;
	// a lit room, an open stack or the picked doorway may be gone
	[chain, typed, lit, opened, dirty, door] = [[], "", null, null, true, null];
	if (tool === "scale" && !underlay) setTool(drawTool);
	renderRooms();
	renderThings();
}

// Undo, or redo with `back` false. While a room is being drawn, undo takes
// back its last corner instead, as Backspace does.
function undo(back = true) {
	if (back && chain.length) return void chain.pop();
	const [from, to] = back ? [history, future] : [future, history];
	if (!from.length) return say(back ? "Nothing to undo." : "Nothing to redo.");
	to.push(snapshot());
	restore(from.pop());
	say(back ? "Undone." : "Redone.");
}

// --- doorways ---

// Where the pointer falls along a doorway's wall, metres from its `from`.
function alongDoor(o) {
	const p = [(pointer.x - cam.ox) / cam.ppm, (pointer.y - cam.oy) / cam.ppm];
	const len = dist(o.from, o.to);
	return ((p[0] - o.from[0]) * (o.to[0] - o.from[0]) + (p[1] - o.from[1]) * (o.to[1] - o.from[1])) / len;
}

const doorStep = () => (pointer?.alt ? 0.001 : DOOR_STEP);
const inSteps = (v) => Math.round(v / doorStep()) * doorStep();
// Whether doorway o, as openings[i] or a new one (i -1), would overlap another.
const overlapsDoor = (o, i = -1) => openings.some((x, k) => k !== i && doorsOverlap(o, x));

// A doorway's ends on screen.
function doorEnds(o) {
	const len = dist(o.from, o.to);
	return doorSpan(o).map((s) => toScreen([o.from[0] + ((o.to[0] - o.from[0]) * s) / len, o.from[1] + ((o.to[1] - o.from[1]) * s) / len]));
}

// The doorway at (x, y) on screen: { i, end } with end 0 or 1 for one of its
// ends, null for its middle; or null.
function doorAtScreen(x, y) {
	let best = null, bestD = REACH_PX;
	openings.forEach((o, i) => {
		const [a, b] = doorEnds(o);
		[a, b].forEach(([ex, ey], end) => {
			const d = Math.hypot(ex - x, ey - y);
			if (d <= bestD) [best, bestD] = [{ i, end }, d];
		});
		if (best?.i === i) return;
		const { off, t, len } = alongScreen([x, y], a, b);
		if (t > 0 && t < len && off <= bestD) [best, bestD] = [{ i, end: null }, off];
	});
	return best;
}

function alongScreen([x, y], [ax, ay], [bx, by]) {
	const len = Math.hypot(bx - ax, by - ay);
	const [dx, dy] = [(bx - ax) / len, (by - ay) / len];
	return { t: (x - ax) * dx + (y - ay) * dy, off: Math.abs((x - ax) * dy - (y - ay) * dx), len };
}

// A click in Door: a doorway on the wall pointed at, or, off the walls, none
// picked.
function addDoor() {
	if (!next) return void (door = null);
	const o = doorAt(next.edge, inSteps(next.edge.t), DOOR_W);
	if (overlapsDoor(o)) return say("There's a doorway there already: drag it, or its ends.", true);
	remember();
	openings.push(o);
	[door, dirty] = [openings.length - 1, true];
	say(`A doorway ${o.width.toFixed(2)} m wide: drag its ends to size it, or type its width.`);
}

// The picked doorway dragged: its end to the pointer, or its middle, the
// spot where it was grabbed staying under the pointer. One that would overlap
// another stays where it was.
function dragDoor() {
	const { i, end, grab } = drag.door;
	const o = openings[i];
	const t = alongDoor(o);
	const moved = end === null ? doorAt(o, inSteps(t - grab), o.width) : resizeDoor(o, end, t, doorStep());
	if (overlapsDoor(moved, i)) return say("Doorways can't overlap.", true);
	openings[i] = moved;
	dirty = true;
	say(`${moved.width.toFixed(2)} m wide.`);
}

// Enter in Door: the picked doorway as wide as typed, about its middle.
function sizeDoor() {
	const w = parseFloat(typed);
	typed = "";
	if (door === null) return say("Pick a doorway first.", true);
	if (!(w > 0)) return say("Type a width in metres, like 0.8.", true);
	const o = openings[door];
	const sized = doorAt(o, doorSpan(o).reduce((a, b) => a + b) / 2, w);
	if (overlapsDoor(sized, door)) return say("That would overlap another doorway.", true);
	remember();
	[openings[door], dirty] = [sized, true];
	say(sized.width < w ? `As wide as its wall: ${sized.width.toFixed(2)} m.` : `${sized.width.toFixed(2)} m wide.`);
}

function removeDoor() {
	remember();
	openings.splice(door, 1);
	[door, dirty] = [null, true];
	say("Doorway removed.");
}

// --- placing ---

function noPlaces() {
	return { router: null, sensors: {}, devices: {} };
}

const anyPlaced = () => Boolean(places.router) || Object.keys(places.sensors).length > 0 || Object.keys(places.devices).length > 0;

// What can be placed: the router, the sensors and the devices seen lately,
// each with a key: "router", "sensor:<name>" or the device's MAC. Devices
// with a name first.
function things() {
	const devices = [...DEVICES].sort((a, b) => (a.hostname ? 0 : 1) - (b.hostname ? 0 : 1) || a.name.localeCompare(b.name));
	return [
		{ key: "router", name: "Router", sub: "where the traffic goes" },
		...SENSORS.map((x) => ({ key: `sensor:${x.name}`, name: x.name, sub: "sensor" })),
		...devices.map((d) => ({ key: d.mac, name: d.name, sub: [d.given && d.hostname?.split(".")[0], d.vendor, d.ip, d.name === d.mac ? null : d.mac].filter(Boolean).join(" · "), device: true })),
	];
}

// Where a thing is: { at } or { carried_by }, or null while it waits.
function placeOf(key) {
	if (key === "router") return places.router && { at: places.router };
	if (key.startsWith("sensor:")) {
		const at = places.sensors[key.slice(7)];
		return at ? { at } : null;
	}
	return places.devices[key] ?? null;
}

// Puts a thing at { at }, with { carried_by }, or back to waiting (null).
function setPlace(key, place) {
	if (key === "router") places.router = place?.at ?? null;
	else if (key.startsWith("sensor:")) {
		if (place?.at) places.sensors[key.slice(7)] = place.at;
		else delete places.sensors[key.slice(7)];
	} else if (place) places.devices[key] = place;
	else delete places.devices[key];
	dirty = true;
}

const roomAt = (p) => roomIn(rooms, p);

function pick(key) {
	picked = key;
	renderThings();
	thingsList.querySelector('[aria-pressed="true"]')?.scrollIntoView({ block: "nearest" });
}

// Click in Place: the picked thing goes there.
function placePicked(p) {
	const t = things().find((x) => x.key === picked);
	if (!t) return say("Pick something in the list first.", true);
	const waited = !placeOf(picked);
	remember();
	setPlace(picked, { at: p });
	const room = roomAt(p);
	say(`${t.name} placed${room ? ` in ${room}` : ""}.`);
	handOn(waited);
}

// After placing the picked thing or saying who carries it: one that was
// waiting hands over to the next one waiting, so a run of devices goes click
// by click.
function handOn(waited) {
	if (!waited) return renderThings();
	const all = things();
	const i = all.findIndex((t) => t.key === picked);
	pick([...all.slice(i + 1), ...all.slice(0, i)].find((t) => !placeOf(t.key))?.key ?? null);
}

const NAME_FONT = "11px ui-monospace, Menlo, monospace";

// The placed things on screen as stacks, each { things, spots } (screen
// points). In Place, things whose names would overlap stack (stacks), and a
// stack holding the opened or the picked thing fans out: `rows`, where its
// things are drawn instead. The thing being dragged stands alone.
function onScreen() {
	const placed = things().flatMap((t) => {
		const at = placeOf(t.key)?.at;
		return at ? [{ t, at: toScreen(at) }] : [];
	});
	const alone = (p) => ({ things: [p.t], spots: [p.at] });
	if (tool !== "place") return placed.map(alone);
	const still = placed.filter((p) => p.t.key !== drag?.thing);
	ctx.font = NAME_FONT;
	const nameW = (t) => ctx.measureText(t.name).width;
	const boxes = still.map(({ t, at: [x, y] }) => [x - 6, y - 8, x + 9 + nameW(t), y + 8]);
	const mark = (g) => {
		const [x, y] = still[g[0]].at;
		return [x - 10, y - 10, x + 14 + ctx.measureText(stackName(g.map((i) => still[i].t))).width, y + 10];
	};
	const out = stacks(boxes, mark).map((group) => {
		const s = { things: group.map((i) => still[i].t), spots: group.map((i) => still[i].at) };
		if (s.things.length > 1 && s.things.some((t) => t.key === opened || t.key === picked)) {
			Object.assign(s, fanColumn(s.spots, { nameW: Math.max(...s.things.map(nameW)) + 9, w: W, h: H }));
		}
		return s;
	});
	return [...out, ...placed.filter((p) => p.t.key === drag?.thing).map(alone)];
}

const stackName = (ts) => `${ts[0].name} +${ts.length - 1}`;

// What a press at (x, y) lands on in Place: a thing's key, standing alone or
// a row of a fanned stack (its dot or its name), or { stack: key } for a
// folded stack's number, or null.
function hitAt(x, y) {
	let best = null, bestD = REACH_PX + 4;
	ctx.font = NAME_FONT;
	for (const s of onScreen()) {
		if (s.rows) {
			s.rows.forEach(([rx, ry], i) => {
				const w = ctx.measureText(s.things[i].name).width + 9;
				const onName = (s.left ? x >= rx - w && x <= rx + 8 : x >= rx - 8 && x <= rx + w) && Math.abs(y - ry) < 9;
				const d = onName ? 0 : Math.hypot(rx - x, ry - y);
				if (d < bestD) [best, bestD] = [s.things[i].key, d];
			});
			continue;
		}
		const d = Math.hypot(s.spots[0][0] - x, s.spots[0][1] - y);
		if (d < bestD) [best, bestD] = [s.things.length > 1 ? { stack: s.things[0].key } : s.things[0].key, d];
	}
	return best;
}

const carriers = () => [...new Set(Object.values(places.devices).map((d) => d.carried_by).filter(Boolean))].sort();

// The list (filtered), each thing with where it is, and the picked one's
// details and what can be done with it.
function renderThings() {
	const all = things();
	const q = filterEl.value.trim().toLowerCase();
	const shown = q ? all.filter((t) => `${t.name} ${t.sub} ${t.key}`.toLowerCase().includes(q)) : all;
	thingsList.replaceChildren(
		...shown.map((t) => {
			const place = placeOf(t.key);
			const b = el("button");
			b.setAttribute("aria-pressed", String(t.key === picked));
			const state = el("span", place?.at ? roomAt(place.at) ?? "placed" : place?.carried_by ?? "waiting");
			state.className = `state${place?.at ? " placed" : place?.carried_by ? " carried" : ""}`;
			b.append(el("span", t.name), state, el("small", t.sub));
			b.addEventListener("click", () => pick(t.key === picked ? null : t.key));
			const li = el("li");
			li.append(b);
			return li;
		}),
	);
	if (!shown.length) thingsList.append(el("li", "Nothing found."));
	carriersEl.replaceChildren(...carriers().map((name) => Object.assign(el("option"), { value: name })));
	renderDetail(all.find((t) => t.key === picked));
}

function renderDetail(t) {
	if (!t) return thingDetail.replaceChildren();
	const place = placeOf(t.key);
	const room = place?.at && roomAt(place.at);
	const where = place?.at ? `Stands ${room ? `in ${room}` : "outside the rooms"}: click a new spot, or drag it on the plan.`
		: place?.carried_by ? `Moves around with ${place.carried_by}.`
		: "Waiting: click where it stands.";
	const parts = [el("b", t.name), el("small", t.sub), el("p", where)];
	if (t.device) {
		// a name of its own, saved at once (not with the plan), shown everywhere
		const d = DEVICES.find((x) => x.mac === t.key);
		const named = el("div");
		named.className = "row";
		const name = Object.assign(el("input"), { value: d?.given ?? "", placeholder: "give it a name", maxLength: 64 });
		name.setAttribute("aria-label", "Name");
		const rename = el("button", "Rename");
		rename.className = "tl-btn";
		const go = async () => {
			const why = await renameDevice(t.key, name.value);
			if (why) return say(`Not renamed: ${why}`, true);
			say(name.value.trim() ? `Named ${name.value.trim()}.` : "Name taken away.");
			renderThings();
		};
		rename.addEventListener("click", go);
		name.addEventListener("keydown", (ev) => ev.key === "Enter" && go());
		named.append(name, rename);
		parts.push(named);
	}
	if (t.device) {
		const row = el("div");
		row.className = "row";
		const who = Object.assign(el("input"), { value: place?.carried_by ?? carrier, placeholder: "who carries it", maxLength: 64 });
		who.setAttribute("list", "carriers");
		who.setAttribute("aria-label", "Who carries it");
		const set = el("button", "Moves with");
		set.className = "tl-btn";
		const go = () => {
			const name = who.value.trim();
			if (!name) return say("Type who carries it.", true);
			carrier = name;
			const waited = !place;
			remember();
			setPlace(t.key, { carried_by: name });
			say(`${t.name} moves around with ${name}.`);
			handOn(waited);
		};
		set.addEventListener("click", go);
		who.addEventListener("keydown", (ev) => ev.key === "Enter" && go());
		row.append(who, set);
		parts.push(row);
	}
	if (place) {
		const back = el("button", t.device ? "Back to waiting" : "Not placed");
		back.className = "tl-btn";
		back.addEventListener("click", () => {
			remember();
			setPlace(t.key, null);
			renderThings();
			say(`${t.name} ${t.device ? "waits in front of the house" : "isn't placed"}.`);
		});
		parts.push(back);
	}
	thingDetail.replaceChildren(...parts);
}

// Everything placed, on the plan: the router and sensors as squares, devices
// as dots; names while placing; the picked one ringed, and where it would go.
// A folded stack is its first thing's spot with the count, the others' spots
// faint; a fanned one draws its things in its rows, each joined to its spot.
function drawPlaces() {
	const placing = tool === "place";
	ctx.font = NAME_FONT;
	for (const s of onScreen()) {
		if (s.things.length === 1) drawThing(s.things[0], s.spots[0], placing);
		else if (s.rows) {
			ctx.strokeStyle = "rgba(127,231,242,0.45)";
			ctx.lineWidth = 1;
			s.spots.forEach(([x, y], i) => {
				ctx.beginPath();
				ctx.moveTo(x, y);
				ctx.lineTo(s.rows[i][0] + (s.left ? 6 : -6), s.rows[i][1]);
				ctx.stroke();
				ctx.fillStyle = "rgba(127,231,242,0.7)";
				ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
			});
			s.things.forEach((t, i) => drawThing(t, s.rows[i], true, s.left));
		} else drawStack(s);
	}
	if (!placing || !picked || !next || drag) return;
	const [x, y] = toScreen(next.at);
	ctx.setLineDash([3, 3]);
	ring([x, y], 6, "rgba(232,244,246,0.7)");
	ctx.setLineDash([]);
}

function drawThing(t, [x, y], named, left = false) {
	const lit = t.key === picked;
	ctx.fillStyle = lit ? "#e8f4f6" : t.device ? "rgba(232,244,246,0.75)" : "#7fe7f2";
	if (t.device) {
		ctx.beginPath();
		ctx.arc(x, y, 4, 0, Math.PI * 2);
		ctx.fill();
	} else ctx.fillRect(x - 5, y - 5, 10, 10);
	if (lit) ring([x, y], 9, "#7fe7f2");
	if (named || lit) {
		ctx.fillStyle = lit ? "#e8f4f6" : "rgba(232,244,246,0.6)";
		ctx.textAlign = left ? "right" : "left";
		ctx.fillText(t.name, x + (left ? -9 : 9), y + 4);
		ctx.textAlign = "left";
	}
}

function drawStack({ things: ts, spots }) {
	ctx.fillStyle = "rgba(127,231,242,0.5)";
	for (const [x, y] of spots.slice(1)) ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
	const [x, y] = spots[0];
	ctx.fillStyle = "#04090c";
	ctx.beginPath();
	ctx.arc(x, y, 9, 0, Math.PI * 2);
	ctx.fill();
	ring([x, y], 9, "#7fe7f2");
	ctx.fillStyle = "#e8f4f6";
	ctx.textAlign = "center";
	ctx.fillText(String(ts.length), x, y + 4);
	ctx.textAlign = "left";
	ctx.fillStyle = "rgba(232,244,246,0.6)";
	ctx.fillText(stackName(ts), x + 14, y + 4);
}

async function save() {
	if (chain.length && tool !== "scale") return say("Finish the room being drawn first, or drop it (Esc).", true);
	if (!rooms.length && !underlay) return say("Draw a room first.", true);
	if (rooms.some((r) => !r.name.trim())) return say("Every room needs a name.", true);
	say("Saving…");
	try {
		if (imageFile) {
			const res = await fetch("/v1/plan/underlay", {
				method: "PUT",
				headers: { "content-type": imageFile.type },
				body: imageFile,
			});
			if (!res.ok) return say(`Image not saved: ${(await res.text()) || `HTTP ${res.status}`}`, true);
			imageFile = null;
		}
		const res = await fetch("/v1/plan", {
			method: "PUT",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({ rooms, openings, underlay, places }),
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
	updateSteps();
	ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
	ctx.clearRect(0, 0, W, H);

	if (ghost && underlay && shown) {
		const [x, y] = toScreen(underlay.offset);
		const s = cam.ppm / underlay.pixels_per_m;
		ctx.save();
		ctx.translate(x, y);
		ctx.rotate(underlay.angle);
		ctx.scale(s, s);
		ctx.globalAlpha = GHOST_ALPHA;
		ctx.drawImage(ghost.canvas, 0, 0, ghost.w, ghost.h);
		ctx.restore();
	}

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
	// the room pointed at, or lit from the list, shows its walls' lengths;
	// so do the rooms of a corner being dragged; not in Door, where they'd
	// cover the doorways
	const at = pointer && !chain.length && tool !== "place" && tool !== "door" && [(pointer.x - cam.ox) / cam.ppm, (pointer.y - cam.oy) / cam.ppm];
	const sized = drag?.corner && drag.moved ? rooms.flatMap((r, i) => (r.points.some((c) => same(c, drag.corner)) ? [i] : []))
		: [lit ?? (at ? rooms.findIndex((r) => inside(at, r.points)) : -1)];
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
	for (const i of sized) if (i >= 0) wallLengths(rooms[i].points);
	drawDoors();
	drawPlaces();

	const last = chain.at(-1);
	if (tool === "box") drawBox(last);
	else if (tool === "scale") drawScale();
	else if (tool === "lines") drawLines(last);
	if (!next || tool === "place" || tool === "door") return;
	const [nx, ny] = toScreen(next.at);
	if (next.how === "corner") ring([nx, ny], 7, "#7fe7f2");
	else {
		ctx.fillStyle = "#7fe7f2";
		ctx.fillRect(nx - 2, ny - 2, 4, 4);
	}
}

// The doorways: a bright stretch of wall between two ticks across it, the
// picked one lit with its width. In Door, where a click would put one, unless
// the pointer is on one already.
function drawDoors() {
	const mark = ([a, b], color, width) => {
		const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
		const [nx, ny] = [((a[1] - b[1]) / len) * 6, ((b[0] - a[0]) / len) * 6];
		ctx.strokeStyle = color;
		ctx.lineWidth = width;
		ctx.beginPath();
		ctx.moveTo(...a);
		ctx.lineTo(...b);
		for (const [x, y] of [a, b]) {
			ctx.moveTo(x - nx, y - ny);
			ctx.lineTo(x + nx, y + ny);
		}
		ctx.stroke();
	};
	const width = (ends, w) => {
		const [x, y] = [(ends[0][0] + ends[1][0]) / 2, (ends[0][1] + ends[1][1]) / 2];
		label(typed ? `${typed}▏ m` : `${w.toFixed(2)} m`, x + 10, y - 10);
	};
	openings.forEach((o, i) => i !== door && mark(doorEnds(o), "#e8f4f6", 2.5));
	if (door !== null) {
		const ends = doorEnds(openings[door]);
		mark(ends, "#7fe7f2", 3.5);
		width(ends, openings[door].width);
	}
	if (tool !== "door" || !next || drag?.door || (pointer && doorAtScreen(pointer.x, pointer.y))) return;
	const o = doorAt(next.edge, inSteps(next.edge.t), DOOR_W);
	const ends = doorEnds(o);
	ctx.setLineDash([4, 3]);
	mark(ends, overlapsDoor(o) ? "#ff3b3b" : "rgba(232,244,246,0.8)", 2.5);
	ctx.setLineDash([]);
	if (door === null) width(ends, o.width);
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

// Each wall's length at its middle, on a dark patch so it reads over the
// image.
function wallLengths(points) {
	ctx.font = "12px ui-monospace, Menlo, monospace";
	ctx.textAlign = "center";
	ctx.textBaseline = "middle";
	points.forEach((a, i) => {
		const b = points[(i + 1) % points.length];
		const [ax, ay] = toScreen(a), [bx, by] = toScreen(b);
		if (Math.hypot(bx - ax, by - ay) < 40) return;
		const text = dist(a, b).toFixed(2);
		const [x, y] = [(ax + bx) / 2, (ay + by) / 2];
		const w = ctx.measureText(text).width + 8;
		ctx.fillStyle = "rgba(4,7,10,0.85)";
		ctx.fillRect(x - w / 2, y - 9, w, 18);
		ctx.fillStyle = "#e8f4f6";
		ctx.fillText(text, x, y);
	});
	ctx.textAlign = "start";
	ctx.textBaseline = "alphabetic";
}

// The wall being measured, from its first end to the pointer or its second.
function drawScale() {
	const pts = [...chain, ...(chain.length === 1 && next ? [next.at] : [])];
	if (!pts.length) return;
	const [a, b] = pts.map(toScreen);
	ctx.strokeStyle = "#7fe7f2";
	ctx.lineWidth = 1.5;
	for (const [x, y] of b ? [a, b] : [a]) {
		ctx.beginPath();
		ctx.moveTo(x - 6, y);
		ctx.lineTo(x + 6, y);
		ctx.moveTo(x, y - 6);
		ctx.lineTo(x, y + 6);
		ctx.stroke();
	}
	if (!b) return;
	ctx.beginPath();
	ctx.moveTo(...a);
	ctx.lineTo(...b);
	dashed(true);
	const text = typed ? `${typed}▏ m` : chain.length === 2 ? "type its length" : `${dist(pts[0], pts[1]).toFixed(2)} m now`;
	label(text, (a[0] + b[0]) / 2 + 10, (a[1] + b[1]) / 2 - 10);
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

// The room corner within reach of (x, y) on screen, when corners can be
// dragged: in Box and Lines with no room being drawn.
function cornerAt(x, y) {
	if ((tool !== "box" && tool !== "lines") || chain.length) return null;
	let best = null, bestD = REACH_PX;
	for (const c of rooms.flatMap((r) => r.points)) {
		const [cx, cy] = toScreen(c);
		const d = Math.hypot(cx - x, cy - y);
		if (d <= bestD) [best, bestD] = [c, d];
	}
	return best;
}

// The corner dragged to where the pointer snaps, if every room that has it
// stays one the collector takes; otherwise it stays at its last good spot.
function dragCorner() {
	if (!next || same(next.at, drag.corner)) return;
	const moved = moveCorner({ rooms, openings }, drag.corner, next.at, MIN_WIDTH);
	if (moved.why) return say(moved.why, true);
	({ rooms, openings } = moved);
	drag.corner = next.at;
	say("");
}

// Click to put a corner, drag to move the plan, scroll to zoom. In Box and
// Lines, dragging a corner moves it. In Place, a press on a placed thing
// picks it, and dragging moves it instead.
let drag = null;
canvas.addEventListener("pointerdown", (ev) => {
	canvas.focus();
	const r = canvas.getBoundingClientRect();
	const [x, y] = [ev.clientX - r.left, ev.clientY - r.top];
	const hit = tool === "place" ? hitAt(x, y) : null;
	const thing = typeof hit === "string" ? hit : null;
	// a folded stack pressed: it fans out
	if (hit?.stack) opened = hit.stack;
	// a corner is only dragged once the pointer moves; a click on it still
	// starts a room there
	drag = { x: ev.clientX, y: ev.clientY, moved: false, thing, opening: Boolean(hit?.stack), corner: cornerAt(x, y) };
	if (thing) pick(thing);
	// a doorway pressed: picked, and dragged once the pointer moves
	const d = tool === "door" ? doorAtScreen(x, y) : null;
	if (d) {
		pointer = { x, y, alt: ev.altKey };
		const o = openings[d.i];
		drag.door = { ...d, grab: alongDoor(o) - doorSpan(o).reduce((a, b) => a + b) / 2 };
		[door, typed] = [d.i, ""];
	}
	canvas.setPointerCapture(ev.pointerId);
});
canvas.addEventListener("pointermove", (ev) => {
	const r = canvas.getBoundingClientRect();
	pointer = { x: ev.clientX - r.left, y: ev.clientY - r.top, alt: ev.altKey };
	canvas.style.cursor = (drag?.corner || drag?.door) && drag.moved ? "grabbing" : !drag && (cornerAt(pointer.x, pointer.y) || (tool === "door" && doorAtScreen(pointer.x, pointer.y))) ? "grab" : "";
	if (drag) {
		const dx = ev.clientX - drag.x, dy = ev.clientY - drag.y;
		if (drag.moved || Math.hypot(dx, dy) >= 4) {
			// one undo for the whole drag
			if (!drag.moved && (drag.thing || drag.corner || drag.door)) remember();
			drag.moved = true;
			if (drag.thing) {
				// no rendering of the list while dragging; it catches up on release
				const at = [(pointer.x - cam.ox) / cam.ppm, (pointer.y - cam.oy) / cam.ppm].map(mm);
				setPlace(drag.thing, { at });
			} else if (drag.corner) {
				aim();
				dragCorner();
				dirty = true;
			} else if (drag.door) dragDoor();
			else {
				cam.ox += dx;
				cam.oy += dy;
			}
			[drag.x, drag.y] = [ev.clientX, ev.clientY];
		}
	}
	aim();
});
canvas.addEventListener("pointerup", (ev) => {
	const isClick = drag && !drag.moved && ev.button === 0;
	const { thing, moved, opening, corner, door: pressed } = drag ?? {};
	drag = null;
	if (opening || pressed) return;
	if (corner && moved) {
		canvas.style.cursor = "";
		return aim();
	}
	// a placed thing pressed: picked, or dragged to its new spot
	if (thing) {
		const room = moved && roomAt(placeOf(thing).at);
		if (moved) say(`${things().find((t) => t.key === thing)?.name} moved${room ? ` to ${room}` : ""}.`);
		return renderThings();
	}
	if (!isClick) return;
	// a click beside a fanned stack folds it, and places the picked thing if any
	if (tool === "place" && opened) {
		opened = null;
		if (!picked) return;
	}
	const r = canvas.getBoundingClientRect();
	pointer = { x: ev.clientX - r.left, y: ev.clientY - r.top, alt: ev.altKey };
	aim();
	if (tool === "door") addDoor();
	else click(next.at);
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
	const z = ev.key.toLowerCase();
	if ((ev.metaKey || ev.ctrlKey) && (z === "z" || z === "y")) {
		ev.preventDefault();
		if (!drag) undo(z === "z" && !ev.shiftKey);
		return aim();
	}
	const key = ev.key === "," ? "." : ev.key === "*" || ev.key === "X" ? "x" : ev.key;
	const part = typed.split("x").at(-1);
	if (/^[0-9]$/.test(key) || (key === "." && !part.includes(".")) || (key === "x" && tool === "box" && typed && !typed.includes("x"))) {
		if (tool === "scale" ? chain.length === 2 : tool === "door" ? door !== null : chain.length) typed += key;
	} else if (key === "Enter") enter();
	else if (key === "Backspace") typed ? (typed = typed.slice(0, -1)) : tool === "door" ? door !== null && removeDoor() : chain.pop();
	else if (key === "Delete" && tool === "door" && door !== null) removeDoor();
	else if (key === "Escape") typed ? (typed = "") : tool === "place" ? (opened ? (opened = null) : pick(null)) : tool === "door" ? (door = null) : (chain = []);
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
document.getElementById("underlay-load").addEventListener("click", () => fileInput.click());
fileInput.addEventListener("change", () => {
	pickImage(fileInput.files[0]);
	// the same file again is a change too
	fileInput.value = "";
	canvas.focus();
});
showBtn.addEventListener("click", () => {
	shown = !shown;
	canvas.focus();
});
filterEl.addEventListener("input", renderThings);
// "Place" in a device's panel: the editor, on that device
document.addEventListener("place", (ev) => {
	openEditor();
	setTool("place");
	pick(ev.detail);
});
turnBtns.forEach((b, i) => b.addEventListener("click", () => {
	turn(i ? 1 : -1);
	canvas.focus();
}));
removeBtn.addEventListener("click", () => {
	removeUnderlay();
	canvas.focus();
});
toolBtns.forEach((b) => b.addEventListener("click", () => {
	setTool(b.dataset.tool);
	canvas.focus();
}));
handle.addEventListener("pointerdown", (ev) => {
	handle.setPointerCapture(ev.pointerId);
	handle.classList.add("dragging");
});
handle.addEventListener("pointermove", (ev) => {
	if (handle.hasPointerCapture(ev.pointerId)) setSideWidth(editor.getBoundingClientRect().right - ev.clientX - handle.offsetWidth / 2);
});
handle.addEventListener("lostpointercapture", () => handle.classList.remove("dragging"));
handle.addEventListener("dblclick", () => setSideWidth(SIDE_W));
handle.addEventListener("keydown", (ev) => {
	if (ev.key !== "ArrowLeft" && ev.key !== "ArrowRight") return;
	ev.preventDefault();
	setSideWidth(document.querySelector(".edit-side").offsetWidth + (ev.key === "ArrowLeft" ? 20 : -20));
});
