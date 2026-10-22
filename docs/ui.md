# Tidying the UI (research and plan)

Status, 2026-10-22: the mockup is approved, with two changes: icons from
Lucide, and technical wording. Chunk 3 (buttons and the top bar) is built.
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
2. **Layout check:** a script that opens the page in headless Chrome at
   390 and 1440 px wide, on home, with a panel open, and on each identify
   step, and fails when a control leaves the viewport or two text boxes in
   the top bar, the timeline head or identify overlap.
   Check: it fails on the page as it is now (the defects above).
3. **Buttons and the top bar** _(built)_: the three levels in `style.css`,
   the top bar's groups, the sensors as one status item (a list of them in
   the side panel when there's more than one), the camera's buttons as
   icons with the hint on hover, the icon sprite.
   Check: the layout check passes for the top bar; the owner's judgement.
4. **Keep-clear areas from the DOM:** `drawCallouts` and `resize` take the
   controls' rectangles instead of fixed numbers.
   Check: a callout never lands under a control, at both widths.
5. **The timeline:** the head as above, the axis labels inside the canvas,
   the labels' collision rule.
   Check: no two labels overlap at 1h to 30d, at both widths.
6. **The phone:** the home page's phone layout, the bottom sheet.
   Check: the layout check at 390 px; the owner on the phone.
7. **Identify:** as above.
   Check: the layout check on every step; the owner on a walk.

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
2. On a phone, whether Edit plan should be there at all: drawing a floor
   plan wants a bigger screen.
3. Whether the HUD's readouts stay on the phone, folded, or go.
