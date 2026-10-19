# Colour by a device's own normal (research and plan)

Status, 2026-10-19: chunk 1 built (the baselines, stored hourly); nothing on
the page yet. This is the colour half of
`docs/monitoring.md` plan step 7 ("baselines plus the rules"); the emails
come after it.

## The problem

The plan colours every device by its traffic on one absolute scale: white
below 1 KiB a minute, red at 5 MiB, on a log scale (`heat()` in `app.js`).
On a real network most devices have steady background traffic of tens of
KiB a minute, so most of the plan shows pale red all the time. The one
device that is really busy stands out, but on an ordinary minute everything
looks warm and nothing says where to look.

Absolute traffic answers "who talks most". The question the colour should
answer is "who is doing something unusual for them".

## How others do it

- **Robust z-score (modified z-score).** Score a value by its distance from
  the median, in units of the median absolute deviation (MAD):
  `0.6745 × (x − median) / MAD`. The 0.6745 makes the MAD comparable to a
  standard deviation, and Iglewicz and Hoaglin's usual cutoff is a score
  above 3.5. Median and MAD resist being dragged along by the outliers
  themselves (a 50% breakdown point), which a mean and standard deviation
  don't. Pitfall: when more than half the values are equal, for example
  zero for a device that is mostly idle, the MAD is 0 and the score is
  undefined
  ([MetricGate](https://metricgate.com/docs/robust-z-score-modified/),
  [Logtalk](https://logtalk.org/manuals/libraries/modified_z_score_anomaly_detector.html)).
- **Seasonality.** Datadog's anomaly monitors offer hourly, daily and weekly
  seasonality and use up to six weeks of history. Their "basic" algorithm
  is a rolling quantile with no seasonality, and the seasonal ones need
  enough history to cover a whole season
  ([Datadog](https://docs.datadoghq.com/monitors/types/anomaly/)).
  A Cisco patent on seasonal network patterns notes that the
  weekday/weekend difference takes longer to learn than day/night
  ([US10659333](https://patents.google.com/patent/US10659333)).
- **Cold start, per host.** The Slips IPS keeps a model per host and only
  learns for a set training period before it scores anything. It buckets
  hours by the traffic's own timestamps, not the wall clock, so replayed
  data trains the same way
  ([Slips](https://stratospherelinuxips.readthedocs.io/en/develop/https_anomaly_detection.html)).
- **Short intervals.** A daily-average baseline hides short bursts, so use
  5 to 15 minute intervals for anomaly detection, and keep separate
  baselines for separate purposes
  ([ManageEngine](https://www.manageengine.com/products/netflow/how-to-baseline-network-traffic.md)).

## The plan for chatter

### The score

- **Per device, per hour of the day**, a baseline from its minutes over the
  last 14 days: the median and the MAD of `ln(1 + bytes a minute)`, with
  sent and received added. The log scale fits traffic, where "3× usual"
  matters and "+3 KiB" doesn't. Hour of the week replaces hour of the day
  once there are about 3 weeks of data (open question 1).
- **A minute the sensor reported but the device didn't appear in counts as
  0 bytes.** `device_minutes` only has rows for devices that were heard, so
  the zeros must come from `reports`. Otherwise an idle device's baseline
  is built from its busy minutes only. The zeros start at the device's
  first minute in the 14 days: before that it wasn't on the network, and
  zeros there would make a new device red whenever it talks.
- **A floor under the MAD**, derived from the result it should give rather
  than picked: a device with no spread at all reaches red (3.5) at 10× its
  usual. That gives `FLOOR = 0.6745 × ln 10 / 3.5 ≈ 0.44`, at which 2× its
  usual scores 1.05 (white) and white ends (2) at about 3.7×. This handles
  the MAD of 0 for mostly idle devices. If the owner wants red at 5×
  instead, the 10 changes and the floor follows.
- **Only upward counts.** `score = 0.6745 × (ln(1 + x) − median) /
  max(MAD, FLOOR)`, and anything below 0 is 0. A device that goes quiet is
  the "Gone" rule, not a colour.
- **Learning:** with fewer than 60 minutes in that hour's baseline, fall
  back to the device's baseline over all hours. With less than a day of
  minutes in total, the device is "learning", which shows as a neutral
  colour and the word in its panel.
- **Port scan windows are left out** of the baselines. They make every
  device run about 10× its usual for half an hour, and the app knows when
  they ran (`scans`).

### The plan view

- **Colour = score:** white up to 2, then ramps to red at 3.5. Both are named
  constants for the owner to tune.
- **Size = traffic:** how much a device sends stays visible, without colour.
  The floor glow's size, the packet density and the pulse keep using
  absolute traffic (`heat(bytes)`), so a big talker still looks busy, just
  not alarming.
- **The panel** shows the device's usual traffic for this hour next to the
  current value ("usual now: 40–90 KiB/min").

### The timeline

Timeline buckets are averages over 1 minute to 2 hours. Scoring an average
against a per-minute baseline understates how unusual it is, because
averages vary less than minutes do. To start, score each bucket's average
with the baseline for its hour and accept that long ranges look calmer. The
graph's peak band stays absolute.

### Data and where it lives

- Compute the baselines in the collector once an hour (in Rust: SQLite has
  no median) and keep them in a `baselines` table: `mac`, `hour`, `median`,
  `mad`, `n`, `computed_at`. 14 days of minutes for a home is well under a
  million rows. That's a few seconds at most, run in the background.
- `core/models/baseline.rs`: the score, the MAD floor and the fallback, as
  pure functions with tests. `core/handlers/baseline.rs`: reading the
  minutes, adding the zeros, leaving out scans, storing the result.
- The overview and the timeline carry each device's 24 `[median, mad]`
  pairs. The page computes the score, so the playhead can use the
  baseline for its bucket's hour without another request.

## Chunks

1. Done: baseline computation, the table, and the hourly job, with tests:
   an idle device, a steady device, one spike, zero minutes filled in from
   the first one heard, the fallback at 60 minutes, scans left out. The
   hours are the collector's, so the page gets the hour of each bucket
   rather than working it out in the browser's time zone.
2. Baselines in `/v1/overview` and `/v1/timeline`, the learning state, and
   the score and its MAD floor in Rust with their tests (the floor goes to
   the page from there, so it's derived in one place).
3. The plan colours by score, size and particles by traffic, and the
   learning state; "usual now" in the panel.
4. The owner looks at it on real data, then the thresholds get tuned.
5. Then step 7's emails, which use the same score.

## Validation

Checked automatically:

- the score: a steady device at its median scores 0; with a MAD of 0,
  doubling stays under 2 and 10× scores 3.5 (the floor's constraint, as a
  test);
- the zeros: a device heard in 1 of 60 minutes has a median of 0;
- the fallback and the learning state at their thresholds;
- scan minutes don't change a baseline.

Only the owner can judge:

- whether an ordinary minute now looks calm, and the one unusual device
  stands out;
- the thresholds (2 and 3.5) and "10× usual is red" behind the floor;
- whether size for traffic still shows who the big talkers are.

## Still unsure

1. When to switch to hour of the week, and whether weekends need their own
   baseline before that.
2. Score sent and received separately (an upload burst is a different signal
   from a download)? Starting with their sum.
3. Whether timeline buckets need their own baselines (bucket averages
   instead of minutes) once long ranges look too calm.
4. Whether a device's baseline should leave out today, so today's spike
   doesn't count toward its own normal. The median mostly absorbs that.
5. A report is a minute for every device, also one on a network whose
   BSSID the sensor doesn't know yet (`nets_known`). Those minutes count as
   zeros now; with one sensor that rarely matters.
