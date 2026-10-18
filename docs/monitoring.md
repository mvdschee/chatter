# The monitoring app (plan)

Status, 2026-10-17: a plan, no code yet. The collector grows into a network
monitoring app with a live floor-plan view. This doc is the plan; the first
thing built is a static mockup, so the look can be judged before anything
else exists.

## Goal

One place that shows what every device on the network is doing, laid out on
a floor plan of the house, and that sends an email only when something is out
of the ordinary. It should be worth opening for its own sake: a dark,
"inside the system" view in which traffic visibly flows.

Not the goal: scanning (nmap stays outside, see Sources), exact positions, or
seeing payloads.

## Sources

The app reads; it doesn't probe the network itself.

| Source | What it gives | How it arrives |
|---|---|---|
| Sensors (this repo's firmware) | per device per minute: frames, bytes, retries, RSSI; broadcast totals | `POST /report`, as now |
| Port scan (nmap, run outside the app) | per device: open ports, services, vendor | the scanner posts its result, or the app reads its output file. **To decide** |
| DNS server's query log | per device: names looked up | the DNS server's API or log. **To decide**; last in the plan |

More sensors (one per room, for example on ESP32-C3s) report to the same app,
each with its own `sensor` name.

## What's possible, and what isn't

- **Walls from Wi-Fi: no.** Drawing walls from reflections needs radar-like
  hardware: many antennas sampled together and a very wide band, for
  centimetre distances. An ESP32 has one antenna and 20 MHz, about 15 m of
  range resolution. Many sensors measuring each other (radio tomography) give a
  blurry attenuation picture, good enough to notice a person, not to draw a
  wall. **The floor plan is drawn by hand** in the app, or imported from a
  phone/tablet LiDAR scan (Apple's RoomPlan exports walls and openings).
- **Room per device: yes, roughly.** With a sensor in several rooms, the one
  that hears a device strongest suggests its room (how ESPresense does room
  presence over Bluetooth). Signal strength alone can't give an exact spot:
  walls, people and reflections make it jump by metres. So the app suggests a
  room, and **you drag the device to its real place**. Most IoT devices never
  move; phones are the exception.
- **Phones and laptops:** their data is MIMO and invisible to the sensors,
  but their small control frames aren't. So they get a location and presence,
  not traffic numbers.
- **People (later, optional):** CSI (channel state information) between two
  ESP32s changes when someone moves between them; esp-radio has a `csi`
  feature. That's motion per room, not identity, and it's a separate
  measurement. Agree on it with the household before it's switched on.

## Architecture

The collector (`crates/collector`) becomes the app, in the hexagonal layout of
the other services:

- `core/models`: devices, rooms and positions, minute counters, scan results,
  events. `core/handlers`: ingest a report, place a device, compute baselines,
  detect outliers.
- `repository`: SQLite. A row per device, sensor and minute is about 36k rows
  a day for 25 devices, roughly 13M a year: fine with an index on
  (device, time). Downsample to hourly after a few months.
- `services/web`: axum. Ingest endpoints, the UI (static HTML/JS), and a live
  stream (server-sent events) for new minutes.
- `services/mail`: outlier emails.

The UI is a single page with a canvas. Canvas 2D first; WebGL (for example
PixiJS) only if the particle count needs it.

## The look

Inspired by the "city as data" views in games like Watch Dogs. Inspiration
only: no logos, fonts or assets from the game, since this repo is public.

| Element | Look | Data |
|---|---|---|
| Background | dark, faint dot grid, subtle scanlines | — |
| Walls | rows of glowing cyan/white dots | the drawn floor plan |
| Rooms | dim filled areas with a small label | the floor plan |
| Devices | glowing nodes with a type icon and a short label; they pulse with traffic | inventory + sensors |
| Data flows | packet streaks riding an arc from each device to the router, at one slow, steady pace, in bursts | sensor minutes |
| Floor glow | a soft glow on the floor under each device, bigger and redder with traffic | the minute on screen |
| Device panel | "profiler", opens on a click: MAC, vendor, open ports, DNS names, 24 h sparkline, first seen | all sources |
| Timeline | a heatmap of the last 24 hours at the bottom: a row per room, a column per 5 minutes. Drag it or press play and the plan shows that moment; it marks the peak, the scheduled scan and outliers | stored minutes |
| Outlier | node turns red and glitches; the event goes into the Timeline panel, with a count on its button | outlier rules |

### Data flow animation

- The last few minutes of sensor data (for example 5) play **on a loop**.
  Each minute's numbers set the flow for its slice of the loop, so the view
  shows the recent rhythm instead of one frozen number.
- **Colour: white is low traffic, red is high.** The same number sets how
  often packet bursts leave, how many packets a burst has and how long they
  are. The pace stays the same for every flow, so it reads as a stream.
- When a new minute arrives, it joins the loop and the oldest drops out. The
  change blends in over a second or two, so nothing jumps.
- The loop shows real minutes only and never invents activity between them.
  A quiet device stays quiet.

Open: is "high" absolute (bytes per minute, on a log scale, the same for
every device) or relative to the device's own normal? See Still unsure.

## Outliers and email

The app emails only when something stands out. Candidate rules (the
thresholds are domain decisions for the owner, and the app shows them so they
can be tuned):

- **Volume:** a device sends or receives far more than usual for that hour of
  the week. Baseline: median and spread (MAD) per device per hour of the week,
  over the last 2–4 weeks. Robust against a single busy evening.
- **Timing:** activity at an hour the device is normally silent.
- **New device:** a MAC never seen before.
- **Gone:** no traffic for 14 days.
- **Ports:** a new open port on a known device (from the scan results).
- **Scan:** most devices spike at the same time. That's the signature of a
  port scan. The first day-long run showed it exactly: the owner's own
  scheduled scan made every device run about 10× its usual for its 25
  minutes. The app knows when the scheduled scan runs (from its results), so it
  marks that window as expected, and emails only when a scan happens outside
  it.
- **Sensor health:** a sensor stops reporting, or `dropped`/`overflow` stay
  above zero.

Emails are rate-limited: one per event, plus at most one digest a day.

## Plan

One reviewable chunk each, with its check:

1. **Mockup** _(built: `crates/collector/ui/mockup.html`, `make mockup`)_:
   one static HTML file with fake data: a sketched floor plan,
   about 10 devices, looping flows (white to red), one outlier.
   Check: the owner looks at it and says what feels right or wrong.
2. **Storage** _(built)_: reports go into SQLite instead of JSONL; the
   collector is axum with the usual layout (`core`, `repository`,
   `services/web`).
   Check: a day of reports, queried per device and hour.
3. **Floor plan editor:** draw walls and rooms, saved in SQLite.
   Check: draw the real floor plan in a few minutes.
4. **Live view:** the mockup's look on real data over server-sent events,
   with the loop, and the day timeline from storage. Devices are placed by
   hand.
   Check: the owner's judgement, plus "a busy device turns red" with the C3
   test client.
5. **Scan results:** the port scan's output in the device panel.
   Check: a known open port shows up on the right device.
6. **More sensors:** C3 support in the firmware, and a room suggestion from
   the strongest sensor.
   Check: carry a test device from room to room.
7. **Outliers and email:** baselines plus the rules above.
   Check: the C3 test client's download triggers exactly one email.
8. **DNS names** in the device panel.

## Validation

Checked automatically:

- builds, clippy, host tests for ingest, baselines and outlier rules;
- the loop never shows a minute that didn't happen (a test on the animation's
  data, not its pixels);
- one simulated spike gives exactly one email.

Only the owner can judge:

- whether the look is fun and readable, or just busy;
- the white-to-red scale: when something should look "hot";
- which outliers deserve an email, and which are noise;
- whether the floor plan and device placement match the house.

## Still unsure

1. **Colour scale:** absolute traffic, or relative to each device's normal?
   Absolute shows the big talkers; relative shows the unusual. Possibly both:
   colour absolute, outlier glitch relative.
2. How the port-scan results reach the app (push or read a file), and in what
   format.
3. How the DNS log is read.
4. Whether a phone/tablet LiDAR scan is worth supporting, or drawing is
   enough.
5. What Home Assistant's floor-plan cards and NetAlertX already cover. They
   could save work, but they won't give this look.
6. Whether 60 s reports are lively enough for the view, or the sensor should
   report more often.
7. Local or internet? The sensor can't tell whether a device's traffic stays
   on the LAN (a home automation server polling it) or goes to the internet:
   the addresses are encrypted. "Is this a lot?" needs that split. The DNS log
   gives a hint (who it looks up); the gateway route in
   `docs/traffic-visibility.md` would give the bytes.
