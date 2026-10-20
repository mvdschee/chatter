# The floor plan editor (research and plan)

Status, 2026-10-20: research done; chunks 1 to 4 and the first half of 7 built.
Plan step 3 in `docs/monitoring.md`. The view draws the saved plan, or a
made-up example (`EXAMPLE_PLAN` in `crates/collector/ui/plan.js`) until
one with rooms is saved.

## Goal

Anyone who buys this draws their own home in a few minutes, on a laptop or a
phone, with nothing but the app: no CAD skills, no LiDAR phone. The plan then
drives:

- the view: wall dot curtains with doorways, room labels, the floor;
- which room a device or sensor is in (the heatmap per room, the room
  suggestion from the strongest sensor, plan step 6);
- where devices and sensors sit, placed by hand.

## How others do it

### Walls first, rooms worked out (react-planner)

[react-planner](https://github.com/cvdlab/react-planner) (MIT, a React floor
plan editor) keeps a scene of **vertices** (x, y), **lines** (walls, two
vertex ids), **holes** (doors and windows: an offset along one line) and
**areas** (rooms, a list of vertex ids). Vertices keep back-references to
their lines and areas, so moving one corner moves every wall on it. Units are
centimetres; undo is a list of whole-scene snapshots.

Rooms come from the walls: snap the endpoints, split walls where they cross,
build a half-edge graph and walk its faces, dropping the outer one. That's
what the literature on room detection does too: rooms as the faces of the
planar wall graph ([arXiv 2608.25608](https://arxiv.org/pdf/2608.25608)),
built from a half-edge structure and updated locally
([CTU thesis](https://dspace.cvut.cz/handle/10467/114629)), or as a minimum
cycle basis of the segment graph ([arXiv 2312.16363](https://arxiv.org/html/2312.16363v1)).

Pitfalls:

- A doorway drawn as a gap breaks the room's face. Doors have to be objects on
  a wall (react-planner's holes), not gaps, or face detection needs "open
  edges" across the gaps.
- Walls that almost meet, cross slightly or overlap give wrong faces: it
  needs snapping and intersection splitting that are exactly right.
- Face walking, splitting and keeping it all consistent while the user drags
  a corner is the bulk of such an editor.

### Rooms as polygons (ESPresense)

[ESPresense's companion](https://espresense.com/companion/configuration/), the
closest relative in home presence, has the user describe each **room** as a
list of corners in metres, `[x, y]`, from one origin (the building's bottom
left), walked the same way round for every room; the last corner joins the
first. **Floors** have bounds `[[left, bottom, z], [right, top, z]]`. Sensors
(**nodes**) are a point `[x, y, z]` and a room. There is no wall model: walls
are the rooms' edges. It's typed as YAML, with a separate "Floorplan Creator"
to help.

What this gives for free: which room a point is in is a point-in-polygon test;
there's no face detection to get wrong.

### Snapping

From the products' docs, not from one source: snap to an existing corner
within a few pixels first, else keep the line at 90° to the last one, else
snap to the grid, with a modifier key to turn it off
([Archilogic](https://help.archilogic.com/knowledge/disable-snapping-for-structural-objects):
corners, edges and 90°, Alt to disable, tolerances follow the zoom;
[3ds Max](https://help.autodesk.com/cloudhelp/2017/ENU/3DSMax/files/GUID-066537A8-3268-4725-82E4-ED4E31CF8B49.htm):
ortho mode for right-angled plans; [Gridsnap](https://alternativeto.net/software/gridsnap/about/):
endpoints, corners, intersections).

### Scale from an image

Tracing over a photo or a scanner app's export is the standard path for people
without measurements: put two points on the image, type the real distance
between them ([Layoutr](https://www.getapp.co.uk/software/2104092/layoutr),
[PlanSnapper](https://www.getapp.ca/software/2085189/plansnapper),
[Trace 3D Plus](https://trace3dplus.help.trane.com/floor_plan_image_import_tools.html)).
Keep the two points far apart (one guide says over 5 m) to keep the error
small, and store the scale as image pixels per metre, not screen pixels.

## The approach for chatter

**Rooms as polygons, with doorways on their edges.** It's easier than walls
first, and that's deliberate:

- The app needs rooms more than walls: which room a device is in, the
  heatmap per room, the room suggestion. With rooms as polygons that's a
  point-in-polygon test, not face detection that has to be exactly right.
- The view's walls are the rooms' edges: each edge drawn once, even where two
  rooms share it (same two corners, either direction).
- Adjacent rooms share corners by snapping to them, so a shared wall stays
  shared. Dragging a corner moves it in every room that has it.
- A doorway is an opening on an edge (where along it, how wide), like
  react-planner's holes: the room stays closed, the wall curtain gets a gap.
- What it can't do: a free-standing wall inside a room, or walls that aren't
  room edges. Fine for homes; a later "wall" tool could add them.

### Data

Metres, origin at the plan's top left, y down (the canvas's way round; the
view rotates as it likes).

```json
{
  "rooms": [
    { "id": "r1", "name": "Kitchen", "points": [[0, 0], [4.2, 0], [4.2, 3.1], [0, 3.1]] }
  ],
  "openings": [
    { "from": [4.2, 0], "to": [4.2, 3.1], "at": 0.4, "width": 0.9 }
  ],
  "underlay": { "pixels_per_m": 41.7, "offset": [-0.3, 0.8], "angle": 0.021 }
}
```

- An opening names its edge by its two corners (either order), so it survives
  renaming or reordering rooms; `at` is where its middle is, from `from`, as
  a share of the edge's length.
- The plan is one JSON document in SQLite, saved whole: a home is a few dozen
  corners, and it's always loaded and edited as a whole. Each save is a new
  row, so an older version can be restored.
- Positions of devices and sensors are their own table (MAC or sensor name,
  x, y), apart from the plan: they change on their own, and they join with
  devices by MAC.
- The underlay image is one file in the data directory (`plan-underlay`, next
  to the database), not in the database: a new image replaces it. The plan
  keeps where it lies: image pixel (u, v) is at
  `offset + turn(angle) * (u, v) / pixels_per_m`.

### Editing

A separate "edit plan" mode on the same page: the view straight from above,
no tilt, no animation, plain lines.

- **Room:** click the corners, click the first one (or double-click) to close.
  Snapping as above: a corner within a few pixels, else 90° to the last line,
  else a 10 cm grid; Alt turns it off. The length of the line being drawn is
  shown, and typing a number sets it (how most people will draw: "this wall is
  4.20").
- **Box:** two opposite corners, or a typed size (`4.2x3.1`): a rectangular
  room in two clicks.
- **Select:** drag a corner (it moves in every room that has it), drag a
  room, rename it, delete it.
- **Door:** click an edge, drag the opening's width.
- **Underlay:** load an image, click both ends of a wall, type its length;
  then draw over it.
- **Place:** drag devices and sensors from the list onto the plan.
- **Undo/redo:** snapshots of the plan, like react-planner.

Touch: the same tools with a tap for a click; drawing on a phone is harder,
but placing devices there matters most.

## Chunks

One reviewable commit each:

1. Done: storage and API: a `floorplans` table, the plan as one JSON document and
   a new row per save, behind `GET /v1/plan` (`null` before the first save)
   and `PUT /v1/plan`. A save is checked: a room has 3 to 100 corners, no
   corner twice, no edges that cross or touch except neighbours at their
   shared corner, room ids unique; an opening sits on an edge of a room and
   fits on it. The underlay comes with chunk 7.
2. Done: the view draws from data: walls from the rooms' edges, each edge
   split at any corner on it so a shared wall (whole or in part) is drawn
   once, openings as gaps, room labels inset from the top left, the floor and
   the devices' spots only inside rooms. The plan is scaled to fit 1000 by 600
   plan units and the view's frame takes its shape, so the look holds for any
   size or shape of home. The router and the sensor keep their spot when it's
   in a room, else go to the middle of the first room, until chunk 7. Until a
   plan with rooms is saved the page shows the example, now a plan in metres
   like any other.
3. Done: draw rooms. "Edit plan" opens `edit.js` over the view, the
   timeline hidden: the plan from above in metres, its own zoom and pan, the
   saved rooms to draw against (none before the first save; the example
   isn't anyone's home), and a panel on the right with the tool, what the
   keys do, and the rooms, each renamed or deleted there (a doorway goes with
   the last room that has its edge). Two tools, as the owner asked after a
   first try: **Box**, two opposite corners or a typed `4.2x3.1`, for the
   rectangular rooms most homes are; **Lines**, corner by corner with typed
   wall lengths, for the rest.
   Snapping in `geometry.js`: a corner within 10 px; else square to the last
   line (the axes for the first) within 8°, lined up with any corner within
   10 px, else in 10 cm steps; else the 10 cm grid, x and y each in line
   with a corner within 10 px; Alt for none. Lining up was added after a test
   run: at the starting zoom 10 cm is about 4 px, so a rectangle's first
   corner often lands a step off and its last wall closed slanted, and a new
   room started beside a wall off the grid missed it by 5 cm. Corners are
   rounded to the millimetre, so shared ones are the same numbers. A corner
   that would cross the room drawn so far is refused, and so is a room that
   crosses itself or is under 20 cm wide (twice its area over its
   perimeter): the owner's first try made a sliver by double-clicking just
   past a corner, with no way to remove it. A double-click that can't close
   the room takes back the corner its first click put. Save sends the whole
   plan. The first chunk for the owner to try by hand.
4. Done: the underlay, moved up from last: drawing a real home's rooms by
   hand from guessed sizes was the slow part of the owner's first try, and
   an original floor plan was at hand. "Image" in the panel loads a PNG, JPEG
   or WebP (a PDF page exported as PNG first) and lays it over the middle of
   the screen; **Scale**, which it switches to, takes two clicks on the ends
   of a wall and its typed length. The image scales about the first click,
   which stays put, and a wall leaning under 10° from level or upright in
   the image turns square, so a crooked scan lines up with the grid and the
   snapping; measuring the same wall again changes nothing. It's drawn as
   light lines on nothing (white paper clear, a photo's off-white too), at
   most 4096 px on its longer side, 55% opaque, and can be hidden or
   removed. The image goes to `PUT /v1/plan/underlay` on save, before the
   plan, so cancelling leaves the stored one alone; the collector takes PNG,
   JPEG or WebP only, by their first bytes (SVG can carry script), up to
   20 MB, and serves it with `nosniff`. Reopening the editor loads it and
   fits the view to it and the rooms. A plan may be saved with an underlay
   and no rooms yet. Perspective in a photo isn't corrected: a flat scan or
   a straight-on photo. Nothing moves the image onto rooms drawn before it:
   trace them again.
   After the owner's first try, where it wasn't clear that measuring came
   next, the panel became a list of steps, the next one lit and the others
   ticked when done: load the floor plan, measure a wall, turn it, trace the
   rooms, name them and save. Without an image but with rooms, the image
   steps are skipped. **Turn** (Left, Right) turns the whole plan a quarter
   round, rooms, doorways and image together, so what's traced stays on the
   image and the view shows the plan the way round the owner wants (the long
   side across); turning only the image would leave traced rooms behind.
   Then the owner's traced rooms came out about 11% too big: on a plan that
   writes each size between two walls, with no ticks at its ends, the first
   measurement had been clicked to the front of a kitchen counter instead of
   the wall behind it. So measuring again, once the image has been measured,
   carries the traced rooms and doorways along with the image (each corner
   stays on its image pixel, to the millimetre), step 2 says to use the
   scale bar if there is one and otherwise the inside faces of the walls a
   size runs between, and the room pointed at, or lit from the list, shows
   its walls' lengths, to check against the plan's sizes. The first
   measurement after picking an image leaves rooms alone: they weren't
   traced on it.
5. Edit: drag corners (a shared one moves in every room), undo and redo.
   (Renaming and deleting came with chunk 3.)
6. Doors on edges.
7. Place the router, the sensors and the devices, in two commits. The
   owner chose (2026-10-20): placing is a step in the editor, top-down; a
   device not placed yet waits in a marked place on the ground in front of
   the house, not on a made-up spot inside; and a device that moves around
   with someone (a laptop, a phone) is marked as theirs instead of placed.
   Positions are saved in the plan, not in a table of their own as first
   planned: turning the plan and measuring the underlay again move them with
   the rooms, and one save keeps rooms and positions together.
   ```json
   "places": {
     "router": [3.1, 2.0],
     "sensors": { "study": [5.2, 4.4] },
     "devices": {
       "fa:42:31:98:3e:b5": { "at": [1.2, 0.9] },
       "c2:d7:2c:93:34:d2": { "carried_by": "Robin" }
     }
   }
   ```
   A device is named by its MAC, a sensor by its name. Places of devices
   not seen lately are kept, so a device that's off for a week keeps its
   spot.
   1. Done: storage and the editor: `places` checked and saved with the plan
      (points are numbers, MACs look like MACs, names 1 to 64 characters,
      at most 1000 devices); Turn and measuring again move them. A Place
      step before saving: the router, the sensors and the devices in a list
      with a filter (name, vendor, IP, MAC), each placed, carried by someone
      or waiting; pick one, click where it stands, and the next one waiting
      is picked; drag a placed one to move it; the picked one can be marked
      as carried by a person (a name, the last one offered again) or sent
      back to waiting. A device's panel in the view gets "Place", which
      opens the editor on it. Going into Place picks the first one waiting;
      a press on a placed thing picks it rather than placing the picked one
      on top of it. The view doesn't use the places yet.
   2. Done: the view: placed things where they stand, every sensor drawn;
      in front of the house, on the ground, a dashed area of the things
      still waiting and one per person with the devices they carry, each
      named just behind it; a device's panel says its room, or who carries
      it, or that it waits. The router not placed yet stands in the middle
      of the first room (the arcs need it somewhere); a sensor not placed
      yet waits with the devices, after them. Placed points count in the
      plan's bounds, so a router in the garden stays in the frame. The
      areas sit past the house's front wall and the frame grows to take
      them in; the view refits only when that changes its size. A first
      try gave every waiting device a labelled slot: with 48 made-up
      devices waiting the strip came out twice as deep as the house, which
      shrank to a corner.
      So the waiting ones are a compact block of dots without labels (a
      click still opens one, and the picked one is named) that takes the
      width the people's areas leave; a person's devices keep their labels,
      up to three rows deep. With no saved plan every device waits in front
      of the example: it isn't anyone's home, so nothing stands in it.

## Validation

Checked automatically (`node --test crates/collector/ui-tests/*.test.mjs` for
the page's geometry, `cargo test` for the save checks):

- a room is at least three corners and doesn't cross itself (rejected on save);
- point-in-polygon: a device's room, including points on an edge;
- shared edges are drawn once; an opening cuts the right stretch of its edge;
- snapping: a corner near an existing one becomes that one; 90° holds;
- the scale from two points and a distance.

Only the owner can judge:

- whether drawing the real home takes "a few minutes", and how it feels;
- whether snapping helps or gets in the way;
- whether the walls and doorways look right in the view;
- whether tracing over the underlay is fast, and the image clear enough to
  trace yet faint enough to see the rooms;
- placing devices on a phone.

## Still unsure

1. **Several floors.** ESPresense has floors with a height. One floor first;
   floors later as a list of these plans.
2. **Rooms that aren't rectangles** (an L-shaped living room): fine as
   polygons, but is drawing them by clicking good enough, or does it need
   "split room" and "merge rooms"?
3. **Doorways on shared edges** belong to both rooms: stored once on the edge,
   which is why an opening names its corners rather than a room.
4. **Moving a shared corner** moves both rooms. Is that always wanted, or does
   it need a way to pull rooms apart?
5. **Phone editing:** how much of the drawing works with fingers.
6. **Private MACs:** a phone that makes up a new MAC per network, or now
   and then, comes back as a new device, waiting again, and its old place
   is kept under a MAC that's gone. Carried devices are the ones that do
   this most; matching them up needs more than the MAC.
