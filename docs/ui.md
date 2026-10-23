# Tidying the UI (research and plan)

Status, 2026-10-22: the mockup is approved, with two changes: icons from
Lucide, and technical wording. Every chunk is built: 2 (the layout check),
3 (buttons and the top bar), 4 (keep-clear areas), 5 (the timeline), 6
(the phone) and 7 (identify).
The owner finds the home page and the identify page busy and badly
organised: controls were added one feature at a time, each one the same
bordered, uppercase button.

## What's on screen now

| Where | What | Kind |
|---|---|---|
| Plan, top left | sniffer readouts, channel occupancy bars | canvas (`drawHud`) |
| Plan, top right | a pill per sensor, Identify, Devices, Timeline (with a badge), Edit plan, the clock | DOM, all the same button |
| Plan, bottom left | yaw, tilt and zoom readout | canvas |
| Plan, bottom right | a hint line, rotate left and right, zoom out and in, Reset | DOM, the same button |
| Side panel | the device list, a device, the events, a sensor | DOM, one at a time |
| Timeline head | Play, Live, five ranges, the moment's readout, Zoom out, the peak | DOM, the same button |
| Timeline graph | the axis, peak labels, port-scan and identify bands with labels | canvas (`timeline.js`) |
| Identify | Back, the title, four steps, the step's text, the time left with +10 min and Stop, two tabs, a search, rows with "I carry this" on each | DOM, a full-screen layer |
| Edit plan | a toolbar of fourteen buttons in six steps, a side panel | DOM, full screen |

Four different jobs share one look: showing status (sensors, clock),
opening a panel (Devices, Timeline), starting a task that takes over the
screen (Identify, Edit plan), and moving the camera. Nothing tells them
apart, so the eye has nowhere to start.

## Defects, apart from taste

- At phone width the top bar runs off the screen and the sensor pills sit on
  the HUD's readouts.
- At phone width the timeline head runs off the right edge and the peak
  labels collide.
- On a wide screen the graph's axis labels are cut off on the left and the
  identify bands' labels stack on top of each other.
- A tall side panel (a sensor's) covers the camera's buttons.
- The callout box and the plan's fit keep clear of the buttons and the HUD
  by fixed numbers (`draw.js` `drawCallouts`, `view.js` `resize`), so they
  are wrong as soon as a control moves.

## How others do it

- **HUDs rank by urgency.** Game HUD guides rank every item: what's needed
  all the time stays on screen, what's needed sometimes can be hidden, and
  the rest (maps, inventories) lives off the HUD and shows when asked. Equal
  visual weight for everything is the named mistake: hierarchy comes from
  size, position and contrast. Keep the middle of the view clear.
- **One primary action per screen.** Design systems (Carbon, Morningstar)
  allow a single filled button per view, secondary actions as outlined, and
  the rest as ghost (text only) buttons. For toolbars, lists and dashboards
  with many actions they recommend the ghost style, so borders don't pile up.
  Ghost buttons still need a clear hover and pressed state.
- **Exclusive choices are one control.** The ranges, and Devices against
  Timeline (they share the side panel), are each one choice out of a few: a
  segmented control, not separate buttons.
- **Maps keep the map.** Map apps put details in a side sheet on a wide
  screen and a bottom sheet on a phone, with the map still usable behind it.
  Controls float small at the edges.

Sources: [Material bottom sheets](https://m1.material.io/components/bottom-sheets.html),
[Carbon button usage](https://carbondesignsystem.com/components/button/usage),
[Morningstar buttons](https://design.morningstar.com/systems/marketing/business-website/components/buttons),
[readable HUDs](https://bugnet.io/blog/how-to-design-a-readable-hud),
[HUD beginner mistakes](https://bootcamp.uxdesign.cc/7-obvious-beginner-mistakes-with-your-games-hud-from-a-ui-ux-art-director-d852e255184a).

## The approach

Sort every control by its job, and give each job one place and one look.

- **Three button levels.** Primary: filled, at most one per screen (Start
  in identify). Secondary: outlined, rare. Everything else: ghost, the
  uppercase label alone, underlined or lit when on. Exclusive choices are a
  segmented control. The camera's buttons are icons.
- **The top bar has three groups**, apart by space, not boxes: panels
  (Devices, Events) as one segmented control; tasks (Identify, Edit plan)
  as ghost buttons; status (sensors, clock) at the end.
- **The sensors are one status item**, a dot and a word ("all live", "1
  silent"), that opens the sensor list in the side panel. A pill per
  sensor doesn't fit four sensors, and a buyer may have that many.
- **The camera's controls** are one small group of icons; the hint shows
  on hover, not all the time.
- **The timeline head**: Play and Live as a pair, the ranges as one
  segmented control, the readout, then the peak as a ghost link.
- **On a phone**: the plan full width, the HUD's readouts folded into one
  line, the side panel as a bottom sheet, the timeline under the plan. The
  top bar keeps the status and the panels; the tasks go in a menu. Identify
  is the task made for the phone, so it may deserve its own place there.
- **Identify**: a slim header (back, the sensor, the time left; tapping the
  time gives +10 min and Stop), the steps as a thin bar, one primary
  button per step, What's here and Find as a segmented control, and "I
  carry this" only on a device's own view.
- **Icons** come from [Lucide](https://lucide.dev) (ISC licence), as one
  sprite, `ui/icons.svg`, built by `crates/collector/icons.sh` from a list
  of names (`make icons`). The page uses `<use href="icons.svg#name">`, and
  the stroke comes from `.icon` in the CSS, so an icon takes its colour from
  the text around it.
- **Technical wording**, like the hacking HUD in the games that inspired the
  look (`docs/monitoring.md`): "offline", not "silent"; states and numbers,
  not sentences. Each chunk rewrites the words it touches.
- **Keep the look.** This is a HUD, not a generic dashboard: the dark
  ground, the cyan, the condensed uppercase labels stay. Fewer borders, not
  a different style.

This is not CSS only. The canvas draws parts of the layout (the HUD's
readouts, the yaw, tilt and zoom line, the timeline's labels), and the
callout and the plan's fit dodge the controls by fixed numbers. Those
numbers go: the areas to keep clear come from the controls' own
`getBoundingClientRect`, so moving a control can't put a label under it.
The timeline's labels get a collision rule (a label that would overlap the
one before it is dropped or merged), not new offsets.

## Plan

One reviewable chunk each, with its check:

1. **Mockup:** one static HTML file, made-up data, a dim placeholder for
   the plan: the home page on a wide screen and on a phone, and identify's
   Find step on a phone.
   Check: the owner says what feels right or wrong.
2. **Layout check** _(built)_: a script that opens the page in headless Chrome at
   390 and 1440 px wide, on home, with a panel open, and on each identify
   step, and fails when a control leaves the viewport or two text boxes in
   the top bar, the timeline head or identify overlap.
   Check: it fails on the page as it is now (the defects above).
   `make layout` (`ui-tests/layout.mjs`; `FILTER=phone`, `SHOTS=dir`). It
   serves `ui/` itself with made-up data, busy on purpose, so it needs no
   collector. It fails on: the page scrolling sideways, a control or a line
   of text past the screen's edge, a control under another layer (a
   corner is enough), and two lines of text overlapping in the same layer.
   On the page as it was, it found the phone's top bar, camera buttons and
   timeline head off the screen (one wide row widens the whole page), and
   on a wide screen the device list, a device and a sensor's panel over the
   camera's buttons. Identify passed at both widths.
   Text drawn on a canvas isn't in the page: the timeline hands the check
   the labels it drew (`drawnLabels`), the HUD's readouts aren't seen. A
   phone also widens its page to fit what runs off it, so the check
   measures against the screen's width, not `innerWidth`; and something cut
   off by `overflow: hidden` (the plan cutting off the top bar) is a fault,
   unless an ellipsis says so.
3. **Buttons and the top bar** _(built)_: the three levels in `style.css`,
   the top bar's groups, the sensors as one status item (a list of them in
   the side panel when there's more than one), the camera's buttons as
   icons with the hint on hover, the icon sprite.
   Check: the layout check passes for the top bar; the owner's judgement.
4. **Keep-clear areas from the DOM** _(built)_: `drawCallouts` and `resize` take the
   controls' rectangles instead of fixed numbers.
   Check: a callout never lands under a control, at both widths.
   `view.js` `controls()` measures the top bar, the camera's buttons (not
   `.view-ctl`, whose see-through hint takes room too) and the side panel
   while it's open, as boxes on the canvas. The HUD's boxes (the readouts,
   the view line, the corner brackets) come from `drawHud`'s own geometry.
   The callout and a fanned stack's column keep clear of all of them. The
   plan's fit (`fitFrame` in `geometry.js`) keeps clear of all but the
   panel, so it doesn't jump when the panel opens; a ResizeObserver
   fits it again when the top bar or the buttons change size. On a wide
   screen the camera's buttons move beside the open panel instead of under
   it. The layout check gets the callouts drawn (`drawnCallouts`) and, in
   "every device's callout", selects each device in turn; emptying the
   keep-clear list makes it fail. After it, the wide screen passes every
   scene. The phone still fails on the top bar and the side panel (6), and
   there the camera's buttons cover the HUD's view line, which the check
   can't see (canvas text).
5. **The timeline** _(built)_: the head as above, the axis labels inside the canvas,
   the labels' collision rule.
   Check: no two labels overlap at 1h to 30d, at both widths.
   The head: Play and Live as one pair, the ranges as one segmented
   control, the readout (live in short: "last 5 min at 10× · next in 30
   s"), the peak as a ghost link at the end. It wraps on a phone: the
   readout and the peak each get a row, the peak cut short with an
   ellipsis. The graph's left margin comes from its widest amount, so the
   y axis is never cut off. Every label on the graph goes through
   `placeLabels` (`ui/labels.js`): in order of importance (the axes, port
   scans with alerts, the peaks, the other port scans and identify
   sessions), each at the first of its spots that is on the canvas and
   clear of those before it, else left out. A peak tries above its dot,
   then right, left and under it; a band's label starts at the band, else
   ends at it. The page's grid column is `minmax(0, 1fr)`, so a row that
   can't wrap no longer widens the whole page.
   After it, the layout check passes every range at both widths. What still
   failed was for later chunks: the phone's top bar cut off by the plan
   (6), and on both widths a tall side panel over the camera's buttons, and
   on a phone over the timeline head (4 and 6).
6. **The phone** _(built)_: the home page's phone layout, the bottom sheet.
   Check: the layout check at 390 px; the owner on the phone.
   At 900 px and under the page is one screen, no page scroll: the plan's
   canvas takes every swipe, so a page that scrolls couldn't be scrolled
   from it. The top bar sits above the plan, not on it: Devices and
   Events, the sensors' status, and a menu with Identify; the clock goes
   (the phone shows the time). The editor isn't on a phone at all, Edit
   plan nor a device's Place: drawing a plan wants a bigger screen (the
   owner's call, after using it). The page's height is the browser's
   `innerHeight` (`--app-h`), as `100vh` runs under the toolbar on older
   iOS and in some in-app browsers; a double tap doesn't zoom the page
   (`touch-action: manipulation`). On the plan, two fingers pinch to zoom,
   move to pan and twist to rotate, about their middle (`view.js`
   `pinch`). The timeline's amounts drop "/min" on a phone, so the graph
   gets the width. On the timeline, two fingers pinch to zoom the time and
   move to slide it: the graph, without its labels, stretches under them,
   and the stretch they leave is loaded when one lifts (no later than now,
   at least 10 minutes, back to the range once wider than it). The walls'
   dots are circles, drawn from a small image of one, not squares, which
   showed once zoomed in. The side panel is a bottom
   sheet of half the screen that takes the timeline's place while it's
   open, so it covers nothing; the plan fits again into what's left (the
   ResizeObserver watches the canvas too). The HUD folds into one line at
   the top (the channel, frames a second, throughput, devices, dropped), the
   occupancy goes, and the view line keeps only the yaw and zoom; the
   readouts and their boxes come from one `hudLayout` in `draw.js`, so
   what's drawn and what's kept clear can't drift apart.
   The layout check now sees the HUD's text (`hudBoxes`), against the
   controls and the clock, and fails when the plan's canvas is blank a
   frame later. That caught the timeline throwing on its hidden, empty
   canvas, which stopped the frame loop and left the plan blank whenever
   the sheet was open. A "tasks menu" scene opens the menu. After it,
   every scene passes at both widths.
   Not checked: real iOS Safari (`dvh`, its toolbar, the safe areas), and
   the callout, which on a phone often has no room beside its device and
   isn't drawn (the sheet shows the same).
7. **Identify** _(built)_: as above.
   Check: the layout check on every step; the owner on a walk.
   The header is Back (an icon), the sensor's name (cut short with an
   ellipsis) and the time left; tapping the time opens +10 min and Stop
   under it, and any other tap or Esc closes them. The steps are a thin
   bar that names only the step you're on ("3/4 Find"). Each step has one
   filled button; "Identify again", Name it and "I carry this" are ghost
   buttons. What's here and Find are one segmented control. "I carry this"
   is only on a device's own view: a row in What's here opens that view in
   Find (it used to open the device's panel), and the lists show "carried"
   or "this phone" instead. The checklists' details are states and numbers
   ("counting paused", "every 3 s, once switched"). On a phone, a device's
   panel opened from identify is the bottom sheet, and identify ends where
   it starts, so the sheet covers none of it. On a wide screen the steps
   bar now lines up with the rest of the column (its own margin had
   overridden the column's centring). The layout check adds three scenes:
   a device from What's here, its panel, and the time's menu; "put back"
   now stops from that menu.
   Not checked: whether the wording still says enough on a first walk, and
   real iOS Safari.

## Validation

Checked automatically:

- builds, and the layout check at both widths: nothing off screen, no
  overlapping text in the top bar, the timeline head or identify;
- the callout and the timeline's labels never overlap a control or each
  other.

Only the owner can judge:

- whether it feels less busy, and where the eye goes first;
- whether it still feels like the HUD;
- whether Identify and Edit plan are still easy to find once they're quieter;
- whether the phone layout works one-handed;
- whether the status item says enough about the sensors at a glance.

## Still unsure

1. Whether the editor's toolbar is part of this, or stays as it is.
2. ~~On a phone, whether Edit plan should be there at all~~: it isn't
   (chunk 6).
3. Whether the HUD's readouts stay on the phone, folded, or go.
