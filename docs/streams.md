# Flagging multi-stream devices

Written 2026-10-20. Nothing here has been run yet: these are theories to test
once boards are at hand.

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

- **Core:** a parser next to `announcement()`, something like
  `capabilities(data) -> Option<(Mac, streams)>` for the three subtypes, with
  host tests on made-up frames (random MACs).
- **Firmware:** the callback calls it like it calls `announcement()`. `Frame`
  stays at 14 B (a test enforces that), so capabilities travel as a separate,
  rare event, kept per MAC by the aggregation task and sent in the report.
- **Report:** an optional `streams` per device, only when known.
- **Collector:** sticky per MAC, stored like `device_names`, so it survives
  sensor reboots. The device panel shows it next to the counts, and outlier
  scoring may need to treat such devices differently (`docs/outliers.md`).

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
   - The ESP32-C3 test client: HT 1 stream, no HE element.
   - An ESP32-C6 as test client: HT 1, HE 1. This exercises the HE path,
     which the C3 can't (`crates/testclient/` builds for the C3 only today).
   - A multi-stream laptop or phone on 2.4 GHz: 2 or more.
2. **How often:** over a day, how many members show up at all, and how long
   until each one is seen, with and without probe requests.
3. **Spike, theory 2:** turn on control frames and log Block Acks per TA.
   While the multi-stream laptop downloads, do Block Acks from it show up, and
   does the bitmap count come near the frame count the download needs? Watch
   `dropped`.

## Still unsure

1. Whether the driver passes other stations' association requests to the
   callback while the sensor is connected. It passes their probe requests.
2. Whether real devices put the same capabilities in probe requests as in
   association requests. Some trim probes.
3. Whether Block Acks to an HE router are sent in a format the C6 decodes.
   802.11ax can acknowledge several stations in one multi-user frame, where
   the per-station layout differs.
4. How much control frames add to the callback load on a busy channel.
