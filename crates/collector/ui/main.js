// The monitoring page (docs/monitoring.md): starts it and runs the frame loop.
// Each module is one part of the page; their state lives with them.

import { DEVICES, LOOP, buckets, loadOverview, loadTimeline } from "./data.js";
import { draw } from "./draw.js";
import { drawEditor, editing } from "./edit.js";
import { TIME_S } from "./format.js";
import { identifyOpen } from "./identify.js";
import { onData, updateMoment } from "./panel.js";
import { loadPlan } from "./plan.js";
import { MINUTE_MS, PASS_S, newMinute, outlierNow, play } from "./playback.js";
import { updateSensorAges } from "./sensors.js";
import { dragging, drawTimeline, renderControls, seek } from "./timeline.js";
import { moveCamera, reduced, resize } from "./view.js";

let lastPlayed = -1;

// Each frame: the live loop's clock, the playhead, then everything drawn.
function step(now) {
	if (now - play.loopStart >= MINUTE_MS * LOOP) {
		play.loopStart += MINUTE_MS * LOOP;
		newMinute(now);
	}
	const played = Math.floor((now - play.loopStart) / MINUTE_MS);
	if (played !== lastPlayed) {
		lastPlayed = played;
		// the outlier's minute starts: the whole view tears for a moment
		if (DEVICES.some((d) => outlierNow(d, now))) play.glitchUntil = now + 700;
	}
	// held while the playhead is dragged; playing carries on from where it's dropped
	if (play.playing && !dragging) {
		play.cursor += ((now - play.lastFrame) / 1000) * (buckets() / PASS_S);
		if (play.cursor >= buckets() - 1) play.cursor = 0;
	}
	// drawing rooms covers the view and hides the timeline: draw only the
	// editor; identify's view covers both
	if (editing) drawEditor();
	else if (!identifyOpen) {
		moveCamera(now);
		draw(now);
		drawTimeline(now);
	}
	updateMoment(now);
	play.lastFrame = now;
	requestAnimationFrame(step);
}

// the clock in the top bar, and the sensors' ages
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
	play.playing = true;
	renderControls();
});
requestAnimationFrame((now) => {
	play.lastFrame = play.loopStart = play.ringStart = now;
	requestAnimationFrame(step);
});
