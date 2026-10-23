# Flagging multi-stream devices

Written 2026-10-20 as theories to test once boards were at hand.
2026-10-23: theory 1 works (test 1 below), and the sensor now sends each
member's count in its report (build step 1 below). Next: the collector keeps
it, a day of data answers test 2, then the panel shows it.

## The problem

The sensor has one receive chain, so it can't decode 2- or 3-stream frames
(`docs/traffic-visibility.md`). A multi-stream device still shows up, through
its single-stream management and keep-alive frames, but its byte counts are
far too low. Without a flag, it looks like a quiet device. Goal: know which
devices *can* use more than one stream, and say so next to their counts.

## Theory 1: read the capabilities a device announces

Association, reassociation and probe requests are sent single-stream at a
basic rate, so the sensor decodes them. Their body carries the device's HT
(802.11n) and HE (802.11ax) capability elements, and those say how many
streams it supports.

### Where the elements start

The elements follow the 24-byte header and the frame's fixed fields:

| Frame                 | Subtype | Fixed fields                         | Elements at |
| --------------------- | ------- | ------------------------------------ | ----------- |
| Association request   | 0       | capability (2), listen interval (2)  | 28          |
| Reassociation request | 2       | as above, plus current AP (6)        | 34          |
| Probe request         | 4       | none                                 | 24          |

Walk the elements the way `frame::announcement()` does: id (1), length (1),
body. Any length is legal, including 0, so a short element must not end the
walk.

### Counting streams

Offsets are into the element body, after id and length. Check them against
the 802.11 tables or Wireshark's dissector before writing tests on them.

- **HT Capabilities, element 45.** Bytes 0–1 are HT Capability Info, byte 2 is
  A-MPDU parameters, and bytes 3–18 are the Supported MCS Set. Its first four
  bytes are the receive MCS bitmask, one byte per stream: MCS 0–7, 8–15,
  16–23, 24–31. Streams = the number of those four bytes that aren't zero.
  Don't take "highest bit / 8 + 1": MCS 32 and the unequal-modulation MCS
  33–76 sit past bit 31 and would read as 5+ streams.
- **HE Capabilities, element 255 with extension id 35** (the first body
  byte). After it come HE MAC Capabilities (6 bytes) and HE PHY Capabilities
  (11 bytes), so the Rx HE-MCS map for ≤ 80 MHz is at bytes 18–19. It has
  2 bits per stream count 1–8, where `0b11` means not supported. Streams = the
  number of entries that aren't `0b11`. Some early 802.11ax drafts had a
  shorter PHY field, so check the length before trusting the offset.
- **VHT Capabilities, element 191** (802.11ac) is 5 GHz only. The sensor
  won't see it in a normal request, so skip it.
- These are **receive** counts, so they cover downlink. HT's Tx fields (byte 15
  of the MCS set: "Tx MCS set defined", "Tx Rx not equal", Tx max streams)
  and HE's Tx map can differ. Take the larger of the two: either way the
  sensor misses frames.

### What the count means

A capability, not proof of use. A device that can do 2 streams uses 1 when:

- the router has fewer streams;
- HT Capability Info bits 2–3 say **static SM power save** (value 0), which
  keeps it to one receive chain;
- it's far away and rate control has fallen back to single-stream MCS 0–7.

So the flag reads "can use N streams, counts may be incomplete", not "N
streams, invisible".

### How often it shows up

- **Association requests are rare.** A device sends one when it joins or
  roams. IoT gear stays associated for days, and the sensor reboots far more
  often than that.
- **Probe requests are frequent** and carry the same elements. `decode()`
  drops them on purpose, because passing phones with random MACs turned into
  "devices" (`docs/sensor.md`). So take capabilities from a probe request only
  when its MAC is already counted as a member. A member probing with a random
  MAC simply doesn't match, which is fine.

### Where it fits

- **Core:** `frame::capabilities()` reads the elements of the three
  requests, and `Capabilities::streams()` makes them one number. The rules
  for which number to keep are `handlers::streams::learn`, on a
  `StreamTable` of up to 128 MACs that lasts the whole boot.
- **Firmware:** the callback queues the capabilities of every request that
  says something (`Frame` stays at 14 B, a test enforces that), and the
  aggregate task learns from them after every frame, while the period's
  device table still says who's a member.
- **Report:** `streams` and `streams_from` on every device the table knows,
  in every report, not only when it changes: a report can be lost, and the
  value costs about 35 bytes.
- **Collector:** sticky per MAC, stored like `device_names`, so it survives
  sensor reboots. The device panel shows it next to the counts, and outlier
  scoring may need to treat such devices differently (`docs/outliers.md`).

### Which count wins

- 0 means unknown and is never kept.
- An association or reassociation request to a watched network makes its
  sender a member and replaces what was known, also with a lower count.
- A probe request counts only from a member (counted this period, or known
  already) and only until an association request is heard. Between two
  probes the larger count stays, as a probe may leave elements out.
- A full table drops the oldest probe count first, then the oldest
  association count. A probe never pushes out an association count.
- **The collector** keeps the same order across boots: an `assoc` count
  replaces anything, a `probe` count replaces only a `probe` count, and only
  when it's larger. Right after a reboot the sensor may only have a probe's
  count, or none, and that mustn't overwrite what an association request
  said before.

## Theory 2: count the Block Acks

A device that receives a burst of multi-stream data answers with a Block Ack
(control frame, type 1, subtype 9). Control responses go out at a basic rate,
so the sensor may decode them even when it couldn't decode the data.

- Layout: frame control (2), duration (2), RA (6, bytes 4–9), TA (6, bytes
  10–15), BA control (2), starting sequence control (2), then a bitmap, 8 bytes
  in the usual compressed form. Each set bit is one MPDU received.
- TA is the device for downlink, the router for uplink. That gives a **frame
  count** for traffic the sensor can't see, though not bytes. Windows overlap,
  so dedupe by starting sequence, like the data path already does.
- Plain ACKs carry only RA, so they can't be attributed to a sender.
- Control frames stay out by default, and esp-radio has no filter setter.
  `esp-wifi-sys-esp32c6` does bind `esp_wifi_set_promiscuous_filter` and
  `esp_wifi_set_promiscuous_ctrl_filter`, so this needs an `unsafe` call. ACKs
  and Block Acks are many: watch the queue's `dropped`.

## Already known

- The download test (`docs/sensor.md`, step 8): a multi-stream laptop moved
  100 MB, and the sensor saw only its small keep-alive frames.
- No header-less (MIMO length-only) frames have shown up so far. The driver
  may drop them before the callback, so they can't give even a total for the
  channel.

## Tests for when the boards are back

1. **Spike, theory 1:** the callback logs MAC, subtype, HT streams, HE streams
   and SM power save for every association, reassociation and probe request
   from a member. Reconnect the test clients to trigger association requests.
   - The spare C6, which joins the network too: HT 1, HE 1, so it
     exercises the HE path.
   - A multi-stream laptop or phone on 2.4 GHz: 2 or more.

   Done 2026-10-23, about five minutes on the dev board, with the other C6
   rebooted and a multi-stream laptop rejoining. The firmware logs each
   request's raw parts once per MAC and value (`caps …`), and every period
   how many requests it heard, how many were to a watched BSSID, and how
   many carried HT and HE (`requests: …`).
   - **Association requests from other stations arrive** while the sensor
     is connected, on both watched networks.
   - **The multi-stream laptop:** 2 streams in HT and in HE, in its
     association request and its probes alike, SM power save off. Most of
     its probes came from random MACs, a new one each scan, and a few from
     its own.
   - **The other C6:** HT 1 and no HE element at all in its association
     request, so the HE path is only tested by other devices. Its probe
     request carried neither element: probes can be trimmed.
   - **The raw parts:** HT's Tx parameters were 0 (no Tx set given) or 1
     (the same as Rx), never "not equal"; HE's Tx always matched its Rx.
     The HE element was 26 to 30 bytes, never the short draft form. One
     faint probe had an HT element with an empty Rx mask, so 0 streams has
     to mean "unknown", not 1. One passing device's probes had static SM
     power save.
   - Requests are few: nothing was dropped from either queue.

   So the number is the larger of HT's and HE's Rx counts, 0 when neither
   says; the association request is the source to trust, and a member's
   probe only fills in until one is heard.
2. **How often:** over a day, how many members show up at all, and how long
   until each one is seen, with and without probe requests. Answered from the
   collector's stored counts and their `streams_from` once it keeps them.
3. **Spike, theory 2:** turn on control frames and log Block Acks per TA.
   While the multi-stream laptop downloads, do Block Acks from it show up, and
   does the bitmap count come near the frame count the download needs? Watch
   `dropped`.

## Still unsure

1. ~~Whether the driver passes other stations' association requests to
   the callback while the sensor is connected~~: it does (test 1).
2. ~~Whether real devices put the same capabilities in probe requests as in
   association requests~~: the laptop did, the C6 left them out (test 1).
3. Whether Block Acks to an HE router are sent in a format the C6 decodes.
   802.11ax can acknowledge several stations in one multi-user frame, where
   the per-station layout differs.
4. How much control frames add to the callback load on a busy channel.

## Build

One commit each.

1. **Sensor** _(built)_: the stream table and its rules in core, the
   firmware learning from every request it hears, `streams` and
   `streams_from` in the report.
   Check: host tests on the rules and the report's JSON; on the board, the
   `streams:` log lines when a member joins.
2. **Collector:** keep the count per MAC across reports and boots, by the
   order in "Which count wins".
   Check: a report with `streams` is stored, a later one without it or with
   a lower probe count doesn't change it.
3. **A day on the home sensor:** test 2 from the stored counts.
4. **Panel:** "can use 2 streams, counts may be incomplete" next to the
   counts.
   Check: the layout check; the owner reads it.
