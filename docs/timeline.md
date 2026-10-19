# The timeline and the heartbeat (research and plan)

Status, 2026-10-19: built as planned below. Before it, the page had a
graph of the last 24 hours in 5-minute buckets (fixed), Play ran from the
playhead to the end and stopped, and the sensor panel's heartbeat was a strip
of 288 bars, which on a healthy day is one solid block that says nothing.

## How monitoring dashboards do it

### The time range (Grafana)

- **A range picker**: a list of relative ranges ("Last 1 hour", "Last 24
  hours", "Last 7 days", …), or absolute from/to
  ([Grafana: use dashboards](https://grafana.com/docs/grafana/latest/visualizations/dashboards/use-dashboards/)).
- **Drag over a graph to zoom in** to that stretch; a zoom-out control widens
  it again (same page).
- **Resolution follows the width**: "max data points" defaults to the panel's
  width in pixels; the bucket ("interval") is the range divided by that, never
  below a minimum interval set to how often data comes in. Coarser buckets are
  consolidated, typically by average or max
  ([Grafana: query options](https://grafana.com/docs/grafana/v9.0/panels/query-options);
  the "range / max points" formula is from a
  [forum answer](https://community.grafana.com/t/interval-and-max-data-points-calculation-in-prometheus-queries/56665),
  not the docs).

### Keeping long history (RRDtool, MRTG)

- The classic network-traffic tiers: **5 minutes for 2 days, 30 minutes for
  2 weeks, 2 hours for 2 months, 1 day for 2 years**, each built from the one
  below ([Cacti/RTM explanation](https://community.ibm.com/community/user/discussion/rtm-understanding-license-feature-usage)).
- **Keep the maximum, not only the average.** Averages can't be turned back
  into peaks, so MRTG stores the peaks for each tier and graphs them
  ([rrd-users](https://lists.oetiker.ch/pipermail/rrd-users/2013-July/019413.html),
  [CERN](https://cixp.web.cern.ch/node/37),
  [mrtg list](https://lists.oetiker.ch/pipermail/mrtg/2000-June/006268.html)).
  A 30-minute average turns a 5-minute port scan into a bump; its maximum
  keeps it a spike.

### Heartbeats (status pages)

Uptime monitors such as Uptime Kuma show a **row of bars, one per check**,
green for up and red for down, with an uptime percentage next to it
([Uptime Kuma widget](https://ca.unraid.net/apps/uptime-kuma-dashboard-widget-1bemfxg08a2qds)).
Status pages do the same over longer periods, one segment per period. The
search found no design document; this is the common convention, and
timestamps on the bars were a requested feature
([issue list](https://opencollective.ecosyste.ms/collectives/uptime-kuma/issues?page=26)).

## The plan for chatter

### Heartbeat: every beat, then the day as segments

1. **Last 60 minutes, one tick per minute**: red where a report should have
   come and didn't, cyan otherwise. Every beat is visible, like Uptime Kuma's
   bars. "Should have come" is worked out from the gaps between reports (one
   due a period after the one before, missing when the next is over half a
   period late), not by counting reports per minute: reports drift a second
   now and then, so a minute can pass without one while none is missing.
2. **Last 24 hours, 48 half-hour segments**: full (all 30 reports) cyan,
   partial amber, none red, before the first report grey. Hover shows
   "13:30–14:00 · 30 of 30 reports". Times under the strip ("24 h ago",
   "12 h", "now").
3. **A summary**: "99.9% of expected reports, 1 missing, 0 restarts".

### Timeline: a range, a resolution that fits, peaks kept

- **Range buttons**: 1 h, 6 h, 24 h, 7 d, 30 d, ending at the newest report.
  **Shift-drag over the graph to zoom in** to that stretch (a plain drag
  already moves the playhead); a "zoom out" button, double-click or Esc goes
  back.
- **Buckets from the range**, like the RRD tiers and about 300 to a graph:
  1 h and 6 h in minutes, 24 h in 5 minutes, 7 d in 30 minutes, 30 d in
  2 hours. The collector computes them from the minutes it stores (SQLite
  handles that at a home's scale; rollup tables later, when a year of minutes
  makes it slow).
- **Average and peak**: the line is the average bytes a minute in each
  bucket; a faint band above it reaches the bucket's busiest minute, so a
  short spike stays visible at 30 days. Hotspots use the peak.
- **Play loops**: from the playhead to the end of the range, then from its
  start again, until Pause or Live. A full pass takes about a minute at any
  range. Dropping the playhead while playing carries on from there. The page
  opens playing (not with reduced motion). While it plays, the plan eases
  toward each bucket over about 0.3 s instead of jumping, and a device's
  pulse speeds up or slows down with its traffic without skipping.
- **The plan follows the playhead** as now; each bucket shows its average.
  So does the device panel: the moment's sent, received and total next to
  the usual range for its hour, and its traffic bars mark the playhead's
  bar. A click on a bar moves the playhead there. Signal is the best of
  the last 24 hours, so it sits with the device's details.

### Data

`GET /v1/timeline?range=7d` (or `from`/`to` for a zoomed stretch): the bucket
size, and per bucket the network's average and peak, plus each device's
average for the plan's replay. The overview keeps the live part (the loop's
minutes, the sensors, the devices); the day's data moves out of it.

## Validation

Checked automatically (Rust tests in `core/models/timeline.rs`):

- the bucket size per range; a bucket's average and peak from known minutes;
- that a 5-minute spike shows in the peak at 30 days.

Checked once by hand in node, not kept as a test (the page has no JS tests):

- the heartbeat's slots: healthy, one report missing (one red tick, one amber
  block), a sensor that started 3 hours ago (grey before), and reports
  drifting 59 or 61 seconds apart (no false red), on made-up beats and on a
  real day's report times;
- `/v1/timeline` for every range against a copy of a real database (each under
  0.2 s), and that a bad range or from/to gets a 400.

Not checked: Play wrapping at the end of the range, shift-drag zoom and its
from/to, dragging the playhead while playing. None of it has run in a browser
yet.

Only the owner can judge:

- whether the heartbeat now reads at a glance;
- whether the peak band helps or clutters;
- the playback speed per range, and whether dragging to zoom feels right.

## Still unsure

1. Whether 30 days in 2-hour buckets is enough, or a year view is wanted
   (then rollup tables).
2. Absolute dates (a calendar) next to the relative ranges.
3. Whether the heartbeat needs a longer view (7 days) too.
