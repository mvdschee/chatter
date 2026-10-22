# Identify mode (research and plan)

Status, 2026-10-22: planned; chunk 1 (the dev board) built and checked,
and the resend fix it turned up is in. Chunks 2 (core), 3 (firmware) and
4 (the collector) are built and checked on the board; 5 (sessions) is
built. The plan changed: the sensor stops counting while it
identifies, rather than doing both. This is the identify
part of `docs/monitoring.md` plan step 6.

## Goal

Hold the sensor next to a device (or the device next to the sensor) and the
page says which device it is, so it can be named and placed. Most devices
on the page are a MAC, a vendor and maybe a hostname, which often isn't
enough to tell which plug or speaker it is.

It works on the one sensor that also counts, but not at the same time: a
sensor carried around counts the wrong part of the house, so it stops
counting while it identifies. A second board only adds something once a
home has several access points far apart (plan step 6).

The way it's used: the sensor is plugged into a phone, which powers it, and
the page is open on that phone. Walking around, the page answers two
questions:

- **What's here?** The devices the sensor hears, strongest first.
- **Where is that one?** Pick a device and the page shows its signal live,
  getting stronger as the sensor gets closer.

Only devices that are online are worth looking for, so the page shows
which ones are.

## How others do it

- **Walk closer and watch the signal.** airodump-ng's `PWR` column is the
  signal of a station's frames, and it "gets higher as you get closer". Its
  rough scale: about −40 dBm is strong, −55 average, below −70 weak, and
  readings near −80 to −90 are noise. A station that shows nothing is one
  the adapter only hears half of: the access point's frames to it, not its
  own ([airodump-ng(8)](https://man.archlinux.org/man/airodump-ng.8.en)).
- **Close range is the reliable range.** ESPresense works out distance from
  the signal at one metre (`rssi@1m`) and an absorption factor for walls,
  both tuned per device type
  ([ESPresense calibration](https://espresense.com/configuration/calibration)).
  Distance from signal is rough at room scale; next to the sensor the
  difference is large enough that ranking by strongest is enough, with no
  calibration.

## The approach

- **The collector keeps the mode and says it in its answers**, as
  `docs/monitoring.md` planned: `204` is "count", `200` with the body
  `identify 600` is "identify mode for ten minutes". The sensor has no other
  way to hear from the collector.
- **A session, step by step:**
  1. Identify is started on the page. The collector answers the sensor's
     next minute report with `identify`.
  2. The sensor stops counting, sends what's left in its outbox, and stops
     sending minute reports. The collector notes the last report before the
     session, so the gap after it has a reason.
  3. The page says what to do: unplug the sensor and plug it into the phone.
  4. The sensor boots on the phone. Before it counts anything it asks the
     collector what to do (`GET /mode/{sensor}`, the same answers), hears
     `identify`, and goes straight to it. If the collector can't be
     reached, it counts.
  5. In identify mode the sensor fills a small table instead of the device
     table. Every 3 s it posts the devices it heard in those seconds,
     strongest first, with each one's strongest signal and its frame count,
     to `POST /identify`. The answer carries the mode too, so "stop" or "ten
     more minutes" lands within 3 s.
  6. When the session ends (stopped or timed out) the sensor stops posting
     and waits; it only counts again after a reboot. The page says to plug
     it back in its place, and that boot asks `GET /mode/{sensor}`, hears "count",
     and counts.
- **Identify readings aren't stored.** The collector keeps the newest ones in
  memory, per sensor, and the page reads them from `GET /v1/identify`. They
  are only worth anything live, and the minute tables stay one row per
  device per minute.
- **The session is stored, like a port scan.** No minutes are counted
  during it, so there's nothing to leave out of the baselines; the collector
  stores each identify session's start and end and shows it as a band on
  the day graph and in the Timeline, so the gap there has a reason.
- **Make the devices talk.** The signal only comes from frames a device
  sends. An idle plug may send nothing for a minute, so while identify mode
  is on, the collector runs its host discovery (`nmap -sn`, ARP on the LAN)
  every few seconds. Every device answers ARP, even one with a firewall, so
  each sweep gets one frame from every device at least.
- **Online is what answered the newest sweep.** Each sweep says which
  devices are up; the page marks those online and greys the rest, and a
  device the sensor heard in the last few seconds is online too.
- **A page for the phone.** Identify starts from the sensor's panel, where
  the mode already shows, and opens a view of its own that fits a phone:
  - "What's here": the devices heard, strongest first, each with its name
    and a signal bar, the top one marked when it stands clearly above the
    next.
  - "Find": every online device, searchable by name, vendor or IP. Picking
    one shows its signal as a large bar and a number, updated every 3 s,
    with "not heard" when its frames stop.
  - From either, a device opens its panel, where renaming is, and from
    there the editor's Place step.
- **The phone isn't a result.** The phone holding the sensor would always
  be the strongest device. The collector knows the page's IP, and the sweep
  maps IPs to MACs, so that device is left out of the list (or shown as
  "this phone").
- **Nor is anything else you carry.** A smartwatch is always near the
  sensor too, and the collector can't find the phone when it doesn't see
  the page's real IP (a container behind Docker's port mapping). So the
  page lets you mark the devices you carry, and leaves them out of "What's
  here". It's the page's own list, kept in the browser, since it belongs to
  the person holding the phone; the collector's `own_mac` is only marked
  for you to start with.

## Pitfalls

- **Antenna:** the board must have its antenna on before Wi-Fi starts.
- **Power from a phone.** The board runs from the phone's USB port (USB
  OTG). Plugging it in boots it, so it needs the Wi-Fi join and up to a
  minute before identify starts, every time it's plugged in.
- **Starting takes up to a minute.** The mode starts with the answer to the
  next minute report. If that's too slow, the sensor can ask `GET /mode`
  every 10 s; that's a tiny request, but it's always on, so only if the
  minute is a problem. Moving the sensor to the phone takes about that long
  anyway.
- **A reboot repeats a report's `seq` and uptime.** Each boot starts both
  at 0 and reaches its first report at about the same second, so the
  collector took a new boot's reports for resends of the last boot's and
  dropped them. Identify reboots the sensor twice per session, so this had
  to go first. A resend window of an hour fixed it only for reboots an hour
  apart; identify's reboots are minutes apart and still matched. Now each
  boot picks a random id and puts it in every report, and a resend is the
  same `seq` from the same boot. Firmware without it keeps the window.
- **Multi-stream devices** send most of their data in frames the sensor
  can't decode (`docs/streams.md`). Their ARP answers and management
  frames are usually sent at basic rates the sensor does decode, but that
  isn't checked.
- **Private MACs:** a phone may show up under a MAC it made up for this
  network, which is still the MAC the page knows it by. One that rotates
  shows up as a new device anyway.
- **Not on `/report` faster.** Shorter reports to `/report` would break the
  minute tables: `device_minutes` assumes a minute per report, and the
  baselines are built on that.
- **One socket.** The report task already has a 24 KiB send buffer.
  Identify posts and `GET /mode` go through the same task rather than a
  second socket with its own buffers.
- **The dev board must never report to the home server.** Two sensors
  reporting there would count every device twice: the collector adds up
  device minutes without looking at the sensor. Counting with several
  sensors is plan step 6.

## A second board for testing

A second C6 runs the same firmware against a collector on the Mac, with
its own database, so testing never touches the sensor that counts.

- `.env` names the board `make run` flashes, where it reports and its name
  (`c6-dev` for this one); the other board's values sit there commented out
  (`docs/setup.md`, step 6).

## Chunks

One commit each.

1. **Dev board:** the `.env` keys and a way to flash it.
   Check: the dev board's reports show up in the Mac's collector, and the
   home server's heartbeat for the counting sensor doesn't change.
   Done; it turned up the resend bug, fixed with a migration that drops the
   `UNIQUE (sensor, seq, uptime_s)` and a resend window of an hour.
2. **Core:** the answer parser (`identify <secs>`, anything else is "carry
   on"), the identify table (per MAC: strongest signal, frames), and its JSON
   writer, strongest first, at most 16 devices.
   Check: host tests for the parser (a body cut short, a number too big,
   `204`), for the order, and that a full table fits the buffer.
   Done. The answer is `identify <secs>\n`: the newline ends the number, so
   a body cut short inside it is unreadable rather than a shorter session.
   An unreadable answer keeps the mode the sensor is in, so a garbled answer
   doesn't end a session; only an empty body (`204`) means "count". The
   table holds 64 devices and posts the strongest 16. Only the device's own
   frames count, and when the table is full a stronger newcomer replaces the
   weakest, so the device next to the sensor is never the one left out.
3. **Firmware:** `report::post` returns the answer's body; `GET /mode` at
   boot; on `identify` the outbox is sent and counting stops, the aggregate
   task fills the identify table instead of the device table, and the
   report task posts it every 3 s; when the mode ends the sensor waits for
   a reboot.
   Check, on the dev board: started by hand from the Mac's collector, a
   phone held against the sensor is first in the posts, no minute reports
   arrive during the session, and after a reboot they come back.
   Built: every request reads the whole answer (it sends `Connection:
   close`, so the collector's close ends it) and applies it, whichever
   request it answers. A boot waits up to 30 s for an address, asks
   `GET /mode/{sensor}` (the sensor's name in the path, since a GET has no
   body; the firmware takes the collector's rule for names), and counts if
   that fails. A switch mid-minute drops that part of a minute, since the
   collector would store it as a whole one. The board check needs chunk 4.
4. **Collector:** in two commits.
   - **4a, sessions and answers:** the mode per sensor (until when, in
     memory), the answers on `/report`, `/mode/{sensor}` and `/identify`,
     the newest readings per sensor, and
     `POST /v1/sensors/{name}/identify` with `{"secs": 600}` to start or
     extend it, `0` to stop.
     Check: tests for the answers (on, off, expired) and the bytes the
     firmware reads; on the dev board, a session started with `curl`
     switches it within a minute, a phone held against it comes first in
     the logged posts, no minute reports arrive during it, it waits when
     stopped, and counts again after a reboot. Built and checked.
   - **4b, the page's data:** `GET /v1/identify` (readings, who is online,
     which device is the page's own), and the host discovery every few
     seconds while a session runs.
     Check: tests for readings that go stale and for the page's own device
     left out; on the dev board, an idle plug held against it comes
     first.
     Built: readings older than 10 s show nothing heard, a sweep older than
     30 s says nobody is online, and online is who answered the newest
     sweep plus whatever a sensor hears now. The sweeps (`nmap -sn -n`,
     every 5 s while a session runs) aren't stored. The page's own device
     is found by the IP it asks from, so it needs the collector to see the
     client's real address: a container behind Docker's port mapping may
     see the bridge's instead, and then nothing is left out.
5. **Sessions:** identify sessions stored with their start and end, and a
   band on the day graph and in the Timeline.
   Check: a session shows as a band over the gap in the minute reports.
   Built: one row per session (`identify_sessions`), written when it
   starts and its end moved when it's extended or stopped early; a muted
   band labelled with the sensor on the day graph, and an event in the
   Timeline. The band covers the session, not the whole gap: the gap starts
   at the next minute report and lasts until the sensor is back in its
   place. A collector restart forgets a running session, so its row keeps
   the planned end although the sensor was told to stop.
6. **Page:** Identify from the sensor's panel, the steps (move the sensor
   to the phone, plug it back after), a phone-sized view with "What's here"
   and "Find", online and offline marked, a click opening the device's
   panel, and "I carry this" on a device to leave it out (the phone found
   by `own_mac` already marked).
   Check: the owner walks around with the sensor on the phone and finds a
   few devices with it.

## Validation

Checked automatically:

- builds, clippy, host tests for the parser, the identify table and the
  collector's answers;
- no minute reports during a session, and counting back after the reboot;
- a reboot's reports are stored, not taken for resends.

Only the owner can judge:

- whether the right device comes first at arm's length, and when touching;
- whether Find's signal tells "warmer" from "colder" while walking;
- whether the wait to start and the 3 s updates feel fast enough;
- whether the view works on the phone, one-handed, while walking;
- whether going from the list to naming and placing a device is smooth.

## Still unsure

1. Whether a minute to start is acceptable, or the 10 s poll is needed.
   The owner says a minute is fine for now.
2. Whether multi-stream devices show up at all (see Pitfalls).
3. Whether 3 s is often enough for Find while walking, or the signal needs
   smoothing over a few posts so it doesn't jump.
4. How long a host discovery sweep takes on a busy network, and so how
   often it can run.
5. Whether waiting for a reboot after a session is right. It keeps a
   sensor still on the phone from counting there, but a sensor that never
   left its place stays quiet until it's replugged.
