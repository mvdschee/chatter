# The sensor

Goal: a C6 near the router counts, per device and per minute, how many frames
and bytes each device on our own Wi-Fi (both the main and the IoT network)
sends and receives. It posts those
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

The router runs two SSIDs, a main one and an IoT one. On one 2.4 GHz radio
that should mean two BSSIDs on the same channel, so one sensor hears both.

- The sensor **joins the IoT network**. That's where the devices it watches
  are, and the IoT network can reach the home server.
- It **counts both**. The sniffer learns the BSSID of each SSID in
  `CHATTER_WATCH_SSIDS` from beacons, which carry both. The callback then keeps
  frames from any of them. The filter is a set of two MACs, so it costs no more
  than one. The joined network's BSSID is also known straight away from the
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

- **Retries** (retry bit set) are retransmissions of the same frame. Count
  them separately, or byte counts inflate on a noisy link.
- **Broadcast and multicast** downlink (`addr1` is `ff:…` or has the group
  bit) belongs to no single device.

### The report

Every 60 s, one JSON document:

```json
{
  "sensor": "c6-1",
  "seq": 42,
  "uptime_s": 2520,
  "dropped": 0,
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
  "broadcast": [{ "net": 1, "rx_frames": 40, "rx_bytes": 5200 }],
  "overflow": 0
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
- **When a POST fails,** keep a few reports in a small ring buffer and send
  them later. Drop the oldest first.

## Layout

Same split as `inkday/docs/project-structure.md`, adapted for firmware. Core is
its own crate so its tests run on the Mac (see `DECISIONS.md`):

| Path                               | What                                                |
| ---------------------------------- | --------------------------------------------------- |
| `Cargo.toml`                       | workspace: `core`, `firmware`                       |
| `core/Cargo.toml`                  | `chatter-core`, `no_std`, no esp deps               |
| `core/src/models/`                 | `Frame` (decoded header), `Counters`, `Report`      |
| `core/src/handlers/`               | the aggregation: frames in, a report per minute out |
| `firmware/src/main.rs`             | init, spawn tasks                                   |
| `firmware/src/config.rs`           | SSID, PSK, watched SSIDs and collector URL, from `.env` at build time |
| `firmware/src/error.rs`            | one `Error` enum                                    |
| `firmware/src/services/wifi.rs`    | connect, reconnect, power save off                  |
| `firmware/src/services/sniffer.rs` | callback plumbing, BSSID filter, static channel     |
| `firmware/src/services/report.rs`  | HTTP POST, retry ring buffer                        |
| `firmware/tests/`                  | on-device adapter tests (`embedded-test`)           |

The package is `chatter-core`, not `core`: a crate named `core` would clash
with Rust's own `core`.

Where the inkday template doesn't carry over:

- **`.cargo/config.toml`:** no `target-cpu=native` or lld. It only sets the
  probe-rs runner for `riscv32imac-unknown-none-elf`; the linker scripts are
  in `firmware/build.rs`. There's no default `build.target`, or
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
  length and RSSI come to about 12 bytes, against about 32 for the raw header
  plus metadata. With 64 channel slots that's about 0.8 KB against 2 KB.
- The callback has to read the header anyway to drop other networks' frames.
  Decoding there means the work happens once.
- The decode is plain fixed offsets: frame control, three addresses, and the
  retry bit. No allocation, and nothing is copied until the final struct.
- Same idea as `TryFrom<PgRow>`: core owns the decode, the adapter only feeds
  it bytes: `decode(data, len, rssi, &watched) -> Option<Frame>` and
  `beacon(data) -> Option<(Mac, &[u8])>` in `core/src/models/frame.rs`,
  tested on the Mac against the address table.

## Pitfalls

- **Antenna.** The board has a U.FL connector and no antenna of its own.
  Never start Wi-Fi without the antenna connected; transmitting into an open
  connector can damage the power amplifier. Flashing and serial are fine.
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
  `firmware/build.rs` reads at build time. Never commit them, and keep them
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
   are dropped. The first spike measured about 85 frames/s on channel 6, an
   evening with light use; we have no busy-channel numbers yet. No
   header-less (MIMO length-only) frames showed up.
5. Do both SSIDs really have their own BSSID on the same channel? The
   beacons show it. If they share one BSSID, the filter can't tell the networks
   apart, and `net` would have to come from somewhere else.

## Plan

One chunk is one reviewable commit. Steps 1–4 don't start Wi-Fi, so they
don't need the antenna. Each install or setup step also goes into
`docs/setup.md` as it happens.

1. **Toolchain:** `rust-toolchain.toml`, then install `probe-rs` and
   `esp-generate`.
   Check: `rustup show` lists the target, and `probe-rs list` sees the C6
   over USB-C.
2. **Workspace:** root `Cargo.toml`, `core/Cargo.toml`, and a `.gitignore`
   with `target/` and `.env` (before the first build).
   `core/src/lib.rs` starts as `#![cfg_attr(not(test), no_std)]`.
   Check: `cargo test -p chatter-core` runs on the Mac.
3. **Firmware boots:** generate a reference project with `esp-generate` in
   a temp dir. Copy into `firmware/` only what's needed (main, `.cargo`
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
   Check, 2026-10-16: an ESP32-C3 test client (`testclient/`, single-stream)
   downloaded exactly 10,485,962 bytes from a LAN HTTP server. Over the same
   window the sensor counted 6,884 rx frames, about 10.1 MB, plus 4,400
   retries, with 0 dropped. At about 1,460 B of TCP payload per frame the
   download needs about 7,180 frames, so about 96% were counted as new
   frames. A MacBook (MIMO) downloading 100 MB was invisible; see
   `docs/traffic-visibility.md`.
9. **Report** to a collector stub on the Mac.
   Check: reports arrive every 60 s, with `seq` going up and `dropped`
   reported.
10. **Collector on the home server.** It keeps the long-term device list:
    a device with no traffic for 14 days counts as gone, and a MAC it has
    never seen before is flagged as new.
    Check: per-device minute rows in SQLite.

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
