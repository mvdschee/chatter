# The sensor

Goal: a C6 near the router counts, per device and per minute, how many frames
and bytes each device on our own Wi-Fi (one or more networks of the same
router) sends and receives. It posts those
counts to a collector on a home server. Over weeks that gives each device an hourly
profile, so "the camera sent 400 MB at 3 a.m." shows up without owning the
router.

## What it can and can't see

- **Sees:** 802.11 headers of data and management frames on one 2.4 GHz
  channel: transmitter, receiver, BSSID, length, RSSI, timing.
- **No payload.** WPA2/WPA3 encrypts it, and we don't want it.
- **2.4 GHz only.** The C6 has no 5 GHz radio, so phones and laptops on 5 GHz
  are invisible. Cheap IoT and cameras are mostly 2.4 GHz, and they are the
  target.
- **MIMO frames arrive as a length only**, with no header (ESP-IDF: "For MIMO
  frames, only the frame length is dumped"). Two-stream clients are partly
  invisible. Most IoT gear is 1x1.
- **Range.** It hears what reaches its antenna. Next to the router it hears
  every downlink frame (the router talks to every device). Uplink from a
  device at the far end of the house may be missed.

## Stack

| Piece                      | Crate                                                                         | Version (2026-10-15) |
| -------------------------- | ----------------------------------------------------------------------------- | -------------------- |
| HAL                        | `esp-hal` (`unstable`)                                                        | 1.2.2                |
| Wi-Fi + sniffer            | `esp-radio` (`wifi`, `sniffer`, `unstable`)                                   | 1.0.0-beta.1         |
| Scheduler / Embassy glue   | `esp-rtos` (`esp-radio`, `embassy`)                                           | 0.4.0                |
| Executor                   | `embassy-executor`                                                            | 0.10.0               |
| Network stack              | `embassy-net`                                                                 | 0.9.1                |
| Heap (the radio needs one) | `esp-alloc`                                                                   | 0.11.0               |
| Bootloader header          | `esp-bootloader-esp-idf`                                                      | 0.6.0                |
| On-device tests            | `embedded-test` (`embassy-010`)                                               | 0.7.2                |
| 802.11 parsing             | none: core reads three fixed offsets (`ieee80211` 0.4 only if decoding grows) | —                    |
| HTTP client                | `reqwless`, or a hand-written POST over `embassy-net` TCP                     | 0.14.0               |

This is the set `esp-generate` 1.4.0 emits. `esp-radio` 1.0.0-beta.2 was
yanked; its only change from beta.1 was a fix for S31 builds, and the source
is otherwise identical, so we pin beta.1.

- **Toolchain:** stable Rust 1.99.0 (esp-hal 1.2 needs ≥ 1.95), target
  `riscv32imac-unknown-none-elf` (the C6 is RISC-V). No `espup`, no ESP-IDF
  download, no nightly. Install steps: `docs/setup.md`.
- **Tools:** `probe-rs` to flash, read logs (RTT), debug and run on-device
  tests over the C6's built-in USB-JTAG. `esp-generate` only as a reference
  for what a fresh project looks like.
- **Pin exact versions.** The sniffer is behind `unstable`, and its API changed
  between releases (`esp-wifi` 0.13 → `esp-radio` 0.16 → 0.18 → 1.0.0-beta).
  Upgrades are a deliberate step, not a `cargo update`.
- **The esp crates release as a set.** `esp-hal`, `esp-radio`, `esp-rtos`,
  `esp-alloc` and `esp-bootloader-esp-idf` each require the matching others
  (`esp-radio` 0.18 wants `esp-hal` ~1.1; 1.0.0-beta wants ~1.2). Take the
  whole set from one `esp-generate` release, never mix.
- **Not all Rust.** The Wi-Fi driver is Espressif's prebuilt blob, shipped
  inside `esp-wifi-sys-esp32c6`. It arrives as a normal crate, with no C
  toolchain or download needed.

## How it's built

```text
radio RX path (driver context)          Embassy tasks
──────────────────────────────          ────────────────────────────────
sniffer callback                        aggregate: Channel → per-MAC counters
  filter: watched BSSIDs        ──►       every 60 s: hand a report over
  take: addrs, len, rssi, flags         send: POST report to the collector
  try_send into a static Channel        wifi: connect, reconnect, keep PS off
  (full → drop, bump a counter)
```

### The esp-radio API (1.0.0-beta.1)

Read from the crate source, not yet run.

- `WifiController::new(peripherals.WIFI, config)` returns
  `Result<WifiController, WifiError>`. The station interface comes from
  `Interface::station()`, the sniffer from `controller.sniffer()`
  (`unstable`). `sniffer()` panics if a sniffer already exists, and Wi-Fi
  stays up while it lives. (0.18 returned both from one `wifi::new` call as
  `Interfaces`.)
- `Sniffer::set_receive_cb(&mut self, cb: fn(PromiscuousPkt<'_>))` takes a
  **plain `fn`, not a closure**. It is stored in a static and called from the
  driver's receive path. Anything it shares has to be a `static`.
- `Sniffer::set_promiscuous_mode(&self, true)` turns it on.
- `PromiscuousPkt { rx_cntl: RxControlInfo, frame_type, len, data: &[u8] }`.
  `data` starts at the 802.11 header.
- esp-radio has no filter setter. ESP-IDF's default filter passes data and
  management frames, which is what we want. Control frames (ACK, RTS/CTS)
  stay out.

### The callback

ESP-IDF on this callback: it "runs directly in the Wi-Fi driver task. Keep it
short … post an event to the application task".

- Call core's decode on the header: frame control, the three addresses, and
  the retry bit (see "Raw bytes or a decoded `Frame`?" below).
- Drop anything whose BSSID isn't one we watch. Do it here: that is the cheapest
  place, and it keeps the neighbours' metadata out of memory entirely.
- `try_send` the decoded `Copy` `Frame` into an `embassy_sync::channel::Channel`
  with a `CriticalSectionRawMutex`. Never block or allocate. When the channel
  is full, drop the frame and increment an `AtomicU32`, so the report says
  how much was lost.

### Which networks

A router can run several SSIDs, for example a main one and one for IoT. On
one 2.4 GHz radio that means one BSSID each, on the same channel, so one
sensor hears them all.

- The sensor **joins one of them** (`CHATTER_WIFI_SSID`), one that can reach
  the collector.
- It **counts all watched networks**. The sniffer learns the BSSID of each SSID in
  `CHATTER_WATCH_SSIDS` from beacons and probe responses, which both carry the
  SSID and the BSSID. While connected, the C6's driver passes almost no beacons
  from other access points, but it does pass their probe responses (the router
  answers every phone that probes). The callback then keeps
  frames from any of them. The filter is a set of up to four MACs, so it costs
  about the same as one. The joined network's BSSID is also known straight away from the
  connect result.
- No scan: esp-radio's `scan_async` returned 0 access points on the C6, both
  before and after connecting (2026-10-15). Beacons need no extra API and
  keep working if the router changes BSSIDs.
- Each device in the report carries `net`, the index of its SSID in
  `CHATTER_WATCH_SSIDS`. The SSID names stay out of the report.
- The SSIDs identify the household. They live in `.env` only: not in the repo,
  the docs or the logs.

### Who is the device in a frame

For frames in our BSS, the station is the address that isn't the BSSID:

| ToDS | FromDS | addr1   | addr2   | addr3       | Station        | Direction        |
| ---- | ------ | ------- | ------- | ----------- | -------------- | ---------------- |
| 1    | 0      | BSSID   | station | destination | addr2          | upload (tx)      |
| 0    | 1      | station | BSSID   | source      | addr1          | download (rx)    |
| 0    | 0      | —       | —       | BSSID       | addr1 or addr2 | management       |
| 1    | 1      | —       | —       | —           | —              | mesh/WDS: ignore |

- **Retries** (retry bit set) are retransmissions of the same frame. Each one
  counts toward `retries`. Its bytes only count when the sensor missed the
  original, that is, when its sequence control differs from the last one seen
  for that device and direction. Dropping every retry undercounted by about
  4%; counting every retry would inflate bytes on a noisy link.
- **Probe requests and responses don't count.** They come from devices
  *looking* for a network. Counting them turned hundreds of passing phones
  (random MACs, receive-only, often on both networks at once) into "devices"
  in the first hour-long run (2026-10-16). Authentication, association,
  deauthentication and action frames still count: those come from members
  joining, leaving or managing the link.
- **Broadcast and multicast** downlink (`addr1` is `ff:…` or has the group
  bit) belongs to no single device.

### The report

Every 60 s, one JSON document:

```json
{
  "sensor": "c6-1",
  "seq": 42,
  "uptime_s": 2520,
  "channel": 6,
  "dropped": 0,
  "overflow": 0,
  "nets_known": [0, 1],
  "devices": [
    {
      "mac": "aa:bb:…",
      "net": 1,
      "tx_frames": 10,
      "tx_bytes": 1200,
      "rx_frames": 8,
      "rx_bytes": 9000,
      "retries": 1,
      "rssi_max": -41
    }
  ],
  "broadcast": [{ "net": 1, "rx_frames": 40, "rx_bytes": 5200 }]
}
```

- **Broadcast and multicast** from the router count toward one total per
  network, not toward a device. A wireless sender's own broadcast already
  counts as its `tx` on the way up to the router; the totals catch storms and
  scans from wired hosts the sensor can't see otherwise.
- **`rssi_max`** comes from uplink frames only: downlink RSSI is the router's
  signal, not the device's.
- **The device table holds 128 devices** and is emptied by each report, so it
  only needs to fit the devices active within one minute. `overflow` counts
  frames from devices that didn't fit.

- **No clock on the board.** `seq` and `uptime_s` let the collector spot
  restarts and gaps, and the collector stamps the receive time. That avoids
  SNTP for now.
- **One report waits at a time.** The aggregate task writes it into a 24 KiB
  buffer (a full 128-device report fits, see core's tests) and a separate task
  posts it, so a slow collector never stalls counting. If the previous report
  is still sending, the new one is skipped, and the gap shows in `seq`.
- **Sending:** the socket's send buffer holds a whole report (24 KiB), the
  connection gives up after 15 s without an ACK, and a failed POST is retried
  once after 2 s. With a 1.5 KB buffer and a 5 s timeout, ~3 KB reports failed
  as soon as the link got slower, because writing had to wait for ACKs midway.
- **Before DHCP completes** nothing can be sent. Newer reports replace the
  waiting one, so the first minutes can show as a gap in `seq`.
- **The outbox:** reports wait in a 32 KiB FIFO (`crates/core/src/models/outbox.rs`,
  about 10 typical reports, at most 16), oldest first. A report only leaves it
  once the collector has answered; after a failure the rest waits for the next
  period. When it's full the oldest goes, and the collector sees that as a gap
  in `seq`.
- **Backlog timing:** a report from the outbox can arrive minutes late, so the
  collector must date it from `uptime_s`, not from when it arrived: written at
  about `received(newest) - (uptime(newest) - uptime(this))`.

## Layout

Same split as `inkday/docs/project-structure.md`, adapted for firmware. Core is
its own crate so its tests run on the Mac (see `DECISIONS.md`):

| Path | What |
| --- | --- |
| `Cargo.toml` | main workspace: `crates/core`, `crates/firmware` |
| `crates/core/` | `chatter-core`: `no_std`, no esp deps, host tests |
| `crates/core/src/models/` | `Frame` decode, `Watched`, `Counters`, the JSON report |
| `crates/core/src/handlers/` | the aggregation: frames in, per-device counters out |
| `crates/firmware/src/main.rs` | init, spawn tasks |
| `crates/firmware/src/config.rs` | SSID, PSK, watched SSIDs and collector address, from `.env` at build time |
| `crates/firmware/src/error.rs` | one `Error` enum |
| `crates/firmware/src/services/wifi.rs` | connect, reconnect, power save off |
| `crates/firmware/src/services/sniffer.rs` | callback plumbing, BSSID learning, static queue |
| `crates/firmware/src/services/net.rs` | IP stack on the station: DHCP, TCP |
| `crates/firmware/src/services/report.rs` | HTTP POST of one report |
| `crates/firmware/src/tasks.rs` | aggregation loop, pending report, report sender |
| `crates/firmware/tests/` | on-device adapter tests (`embedded-test`) |
| `crates/testclient/` | ESP32-C3 download client for validating counts (own workspace) |
| `crates/collector/` | dev collector, plus the test client's download (own workspace, std) |

The package is `chatter-core`, not `core`: a crate named `core` would clash
with Rust's own `core`.

Where the inkday template doesn't carry over:

- **`.cargo/config.toml`:** no `target-cpu=native` or lld. It only sets the
  probe-rs runner for `riscv32imac-unknown-none-elf`; the linker scripts are
  in `crates/firmware/build.rs`. There's no default `build.target`, or
  `cargo test -p chatter-core` would stop running on the Mac. Instead
  `default-members = ["core"]` keeps a plain `cargo build` on the Mac, and
  the makefile passes `--target` for the firmware.
- **Logging:** there is no `println!` in `no_std`. The firmware logs with
  `defmt` over RTT (`defmt::info!` and friends, no `utils.rs` macros), and
  `DEFMT_LOG` in `.cargo/config.toml` sets the level.
- **`unsafe_code = "forbid"`:** works for both crates. The esp macros
  (`esp_app_desc!`, `#[esp_rtos::main]`, `rtt_init_defmt!`) don't trip it.
- **Errors:** core can't use the firmware's `Error`, because the dependency
  runs the other way. So far core needs no error type: decoding returns
  `Option`, and a frame that doesn't fit is dropped.

**Raw bytes or a decoded `Frame`?** Decoded, inside the callback, using a
decode function that lives in core:

- esp-radio doesn't parse 802.11 headers. It decodes only the radio metadata
  (`rx_cntl`). `data` is a raw `&[u8]` that borrows the driver's buffer and
  is gone when the callback returns, so something has to be copied either way.
- A decoded `Frame` is the smaller copy: station MAC, direction, retry flag,
  sequence control, length and RSSI come to 14 bytes, against about 32 for the
  raw header plus metadata. With 64 channel slots that's about 0.9 KB against
  2 KB.
- The callback has to read the header anyway to drop other networks' frames.
  Decoding there means the work happens once.
- The decode is plain fixed offsets: frame control, three addresses, and the
  retry bit. No allocation, and nothing is copied until the final struct.
- Same idea as `TryFrom<PgRow>`: core owns the decode, the adapter only feeds
  it bytes: `decode(data, len, rssi, &watched) -> Option<Frame>` and
  `beacon(data) -> Option<(Mac, &[u8])>` in `crates/core/src/models/frame.rs`,
  tested on the Mac against the address table.

## Pitfalls

- **Antenna.** The board has a U.FL connector and no antenna of its own.
  Never start Wi-Fi without the antenna connected; transmitting into an open
  connector can damage the power amplifier. Flashing and serial are fine.
- **Transmit power.** esp-radio's default maximum is 5 dBm (20 in 0.25 dBm
  steps), too weak for the router to hear the board's full-size frames
  reliably. Small exchanges still work, which makes this look like a flaky
  network: failed handshakes at boot, stretches where it can't join, and
  reports over ~1.5 KB that never arrive, all depending on distance and how
  busy the air is. The firmware sets 60 (15 dBm). esp-radio's docs give
  [8, 84] as the range and report authentication failures above about 65.
- **Power save.** If the station's modem sleep is on, the radio is off between
  beacons and the sniffer misses most frames. Set `PowerSaveMode::None`
  explicitly. The docs list it as the default, but don't rely on that.
- **Channel.** A connected station follows the router's channel. If the router
  changes channel (auto channel), the board follows after a reconnect. Log
  the channel in each report.
- **Sniffer and station at once.** ESP-IDF allows the sniffer "when the
  station is connected to the AP". esp-hal's example only sniffs, it doesn't
  also connect. **Verify on hardware first.**
- **Throughput.** Promiscuous mode slows the board's own connection. That's
  fine for one POST a minute.
- **MAC randomisation.** Phones may use a private MAC per network, and some
  rotate it. Expect "new" devices that aren't.
- **Credentials.** The SSIDs and PSK come from a git-ignored `.env`, which
  `crates/firmware/build.rs` reads at build time. Never commit them, and keep them
  out of logs. They do end up inside the firmware binary in `target/`.
- **Unstable API.** Pin versions (see Stack).

## Still unsure

1. ~~Does the sniffer deliver frames while the station is connected?~~ Yes
   (2026-10-15). Per 10 s it sees about 150–190 management and 530–670 data
   frames, 45–95 of them in our BSS.
2. How much of the router's 802.11ax traffic the C6 decodes, and how much
   arrives as MIMO length-only.
3. ~~Is `RxControlInfo` the same on the C6 as on the ESP32?~~ No. The C6 uses
   the second layout (`wifi_mac_version = "2"`, with 802.11ax fields): `rssi`,
   `rate`, `sig_len`, `channel`, `noise_floor`, `is_group` and more. This was
   read from the esp-radio 1.0.0-beta.1 source and still needs confirming on
   hardware.
4. Callback load on a busy channel: how often the channel fills and frames
   are dropped. The first spike measured about 85 frames/s on the router's
   channel, in a quiet hour; we have no busy-channel numbers yet. No
   header-less (MIMO length-only) frames showed up.
5. ~~Do both SSIDs have their own BSSID on the same channel?~~ Yes: both are
   learned on the router's channel.
6. In the first two hour-long runs, the network the sensor doesn't join was
   only learned at uptime 789–790 s, both far from the router and right next
   to it, so it isn't reception. Beacons from other access points
   rarely get through the driver while connected. Fix (2026-10-16): learn from
   probe responses too. Check: `nets_known` in the report lists both networks
   from the first minutes.

## Plan

One chunk is one reviewable commit. Steps 1–4 don't start Wi-Fi, so they
don't need the antenna. Each install or setup step also goes into
`docs/setup.md` as it happens.

1. **Toolchain:** `rust-toolchain.toml`, then install `probe-rs` and
   `esp-generate`.
   Check: `rustup show` lists the target, and `probe-rs list` sees the C6
   over USB-C.
2. **Workspace:** root `Cargo.toml`, `crates/core/Cargo.toml`, and a `.gitignore`
   with `target/` and `.env` (before the first build).
   `crates/core/src/lib.rs` starts as `#![cfg_attr(not(test), no_std)]`.
   Check: `cargo test -p chatter-core` runs on the Mac.
3. **Firmware boots:** generate a reference project with `esp-generate` in
   a temp dir. Copy into `crates/firmware/` only what's needed (main, `.cargo`
   config, linker setup, logging). Log a heartbeat every second.
   Check: it builds, clippy is clean, and `cargo run` flashes the board and
   shows the heartbeat.
4. **Device tests:** `embedded-test` with one trivial test.
   Check: the test flashes and reports a pass.
5. **Wi-Fi connect** (antenna on): station connects, power save off, and the
   log shows channel and RSSI.
   Check: it connects, and reconnects after the router restarts.
6. **Sniffer spike** _(done)_: sniffer on next to the station; the callback
   counts frames by type, bytes, and frames in our BSS, with atomics only, and
   logs them every 10 s.
   Check: frames arrive while connected.
7. **Core** _(done)_: `Frame` decode (with beacon SSIDs, to learn the watched
   BSSIDs), `Counters`, and the aggregation handler.
   Check: 19 host tests, covering every row of the address table. A mirrored
   upload address makes them fail.
8. **Per-device counts** _(done)_: the callback decodes with core and queues
   `Frame`s; an aggregation task keeps per-MAC counters and logs them every
   10 s.
   Check, 2026-10-16: an ESP32-C3 test client (`crates/testclient/`, single-stream)
   downloaded exactly 10,485,962 bytes from a LAN HTTP server. Over the same
   window the sensor counted 6,884 rx frames, about 10.1 MB, plus 4,400
   retries, with 0 dropped. At about 1,460 B of TCP payload per frame the
   download needs about 7,180 frames, so about 96% were counted as new
   frames: retries whose original was missed lost their bytes.
   Rerun with sequence-number deduplication: 7,448 rx frames, exactly
   11,338,668 B at the link layer, 3,408 retries, 0 dropped. With about
   106 B of headers per frame (802.11, CCMP, LLC, IP, TCP with timestamps,
   FCS), the download should be about 11,253,600 B on air, so the sensor is
   **within 1% (+0.8%)**. The small overcount fits A-MPDU block-ack retries:
   they arrive after later sequence numbers, so remembering only the last
   one misses a few duplicates.
   A multi-stream (MIMO) laptop downloading 100 MB was invisible; see
   `docs/traffic-visibility.md`.
9. **Report** to a collector stub on the Mac _(done)_.
   Check, 2026-10-16: reports arrived every 60 s (about 2.6–3.5 KB for 20–28
   devices), `seq` went up, and `dropped` was reported (0). With the stub
   stopped for one period, that report logged `not sent: Report(connect)`;
   counting went on, and the next report arrived, leaving the gap 0, 1, 2, 4.
9b. **Outbox** _(done)_: keep the last unsent reports and send them once the
   collector is back. 33 host tests.
   Check, 2026-10-16: with the collector stopped for 3 periods, reports 3–6
   waited in the outbox and arrived in order right after it restarted; `seq`
   0–6 with no gap.
10. **Collector on the home server.**
    - 10a _(done)_: `crates/collector/Dockerfile` and the `collector-image`
      workflow push `ghcr.io/<owner>/chatter-collector`. Checked locally with
      podman: `POST /report` answers 204 and the report lands in the data
      volume, on a read-only root and as uid 65532.
    - 10b _(staged)_: an Ansible role in the separate infrastructure repo runs
      the image with docker compose on the home server, LAN only. Rerunning
      the playbook deploys a new image.
    - 10c: the sensor's `.env` gets the home server's address, then the
      day-long data run.
    Check: reports arrive on the home server every 60 s.
11. **Interface:** a web interface over the collected data, replacing the
    weekly email: research and `docs/interface.md` first. It keeps the
    long-term device list: a device with no traffic for 14 days counts as
    gone, and a MAC it has never seen before is flagged as new.

## Validation

Checked automatically:

- it builds; clippy is clean;
- host unit tests for header decoding and the address table;
- the spike's download test;
- `dropped` stays near 0 on a normal evening.

Judged by hand, not by tests:

- whether the counts find anything that the per-device DNS log doesn't;
- where the board lives (near the router, near the IoT devices) and whether
  that placement hears the devices that matter;
- what counts as "hot" later.

## Sources

- [esp-hal `examples/wifi/sniffer`](https://github.com/esp-rs/esp-hal/tree/main/examples/wifi/sniffer)
  (Cargo.toml with `esp32c6`, `sniffer`, `unstable`, `ieee80211`). It
  tracks unreleased `main` through path dependencies, so its code can be
  ahead of 1.0.0-beta.1.
- esp-radio 1.0.0-beta.1 source (`src/wifi/sniffer.rs`, `src/wifi/mod.rs`),
  read from the crates.io tarball.
- `esp-generate` 1.4.0 template (`src/template_files.rs`): the release set and
  the probe-rs runner line.
- [ESP-IDF Wi-Fi sniffer mode (C6)](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c6/api-guides/wifi-driver/wifi-modes.html)
- [Espressif forum: transmitting without an antenna](https://esp32.com/viewtopic.php?p=44218)
