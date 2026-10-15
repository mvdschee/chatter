# The sensor

Goal: a C6 near the router counts, per device and per minute, how many frames
and bytes each device on our own Wi-Fi sends and receives. It posts those
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

| Piece | Crate | Version (2026-10-15) |
|---|---|---|
| HAL | `esp-hal` (`unstable`) | 1.2.2 |
| Wi-Fi + sniffer | `esp-radio` (`wifi`, `sniffer`, `unstable`) | 1.0.0-beta.2 |
| Scheduler / Embassy glue | `esp-rtos` (`esp-radio`, `embassy`) | 0.4.0 |
| Executor | `embassy-executor` | 0.10.0 |
| Network stack | `embassy-net` | 0.9.1 |
| Heap (the radio needs one) | `esp-alloc` | 0.11.0 |
| Bootloader header | `esp-bootloader-esp-idf` | 0.6.0 |
| On-device tests | `embedded-test` (`embassy-010`) | 0.7.2 |
| 802.11 parsing | none: core reads three fixed offsets (`ieee80211` 0.4 only if decoding grows) | — |
| HTTP client | `reqwless`, or a hand-written POST over `embassy-net` TCP | 0.14.0 |

This is the set `esp-generate` 1.4.0 emits (it pins `esp-radio`
1.0.0-beta.1; beta.2 needs the same esp-hal).

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
  filter: our BSSID only        ──►       every 60 s: hand a report over
  take: addrs, len, rssi, flags         send: POST report to the collector
  try_send into a static Channel        wifi: connect, reconnect, keep PS off
  (full → drop, bump a counter)
```

### The esp-radio API (1.0.0-beta.2)

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
- Drop anything whose BSSID isn't ours. Do it here: that is the cheapest
  place, and it keeps the neighbours' metadata out of memory entirely.
- `try_send` the decoded `Copy` `Frame` into an `embassy_sync::channel::Channel`
  with a `CriticalSectionRawMutex`. Never block or allocate. When the channel
  is full, drop the frame and increment an `AtomicU32`, so the report says
  how much was lost.

### Who is the device in a frame

For frames in our BSS, the station is the address that isn't the BSSID:

| ToDS | FromDS | addr1 | addr2 | addr3 | Station | Direction |
|---|---|---|---|---|---|---|
| 1 | 0 | BSSID | station | destination | addr2 | upload (tx) |
| 0 | 1 | station | BSSID | source | addr1 | download (rx) |
| 0 | 0 | — | — | BSSID | addr1 or addr2 | management |
| 1 | 1 | — | — | — | — | mesh/WDS: ignore |

- **Retries** (retry bit set) are retransmissions of the same frame. Count
  them separately, or byte counts inflate on a noisy link.
- **Broadcast and multicast** downlink (`addr1` is `ff:…` or has the group
  bit) belongs to no single device.

### The report

Every 60 s, one JSON document:

```json
{"sensor":"c6-1","seq":42,"uptime_s":2520,"dropped":0,
 "devices":[{"mac":"aa:bb:…","tx_frames":10,"tx_bytes":1200,
             "rx_frames":8,"rx_bytes":9000,"retries":1,"rssi_max":-41}]}
```

- **No clock on the board.** `seq` and `uptime_s` let the collector spot
  restarts and gaps, and the collector stamps the receive time. That avoids
  SNTP for now.
- **When a POST fails,** keep a few reports in a small ring buffer and send
  them later. Drop the oldest first.

## Layout

Same split as `inkday/docs/project-structure.md`, adapted for firmware. Core is
its own crate so its tests run on the Mac (see `DECISIONS.md`):

| Path | What |
|---|---|
| `Cargo.toml` | workspace: `core`, `firmware` |
| `core/Cargo.toml` | `chatter-core`, `no_std`, no esp deps |
| `core/src/models/` | `Frame` (decoded header), `Counters`, `Report` |
| `core/src/handlers/` | the aggregation: frames in, a report per minute out |
| `firmware/src/main.rs` | init, spawn tasks |
| `firmware/src/config.rs` | SSID, PSK and collector URL from build-time `env!` |
| `firmware/src/error.rs` | one `Error` enum |
| `firmware/src/services/wifi.rs` | connect, reconnect, power save off |
| `firmware/src/services/sniffer.rs` | callback plumbing, BSSID filter, static channel |
| `firmware/src/services/report.rs` | HTTP POST, retry ring buffer |
| `firmware/tests/` | on-device adapter tests (`embedded-test`) |

The package is `chatter-core`, not `core`: a crate named `core` would clash
with Rust's own `core`.

Where the inkday template doesn't carry over:

- **`.cargo/config.toml`:** no `target-cpu=native` or lld. The firmware needs
  `[target.riscv32imac-unknown-none-elf]` with the probe-rs runner and the
  esp-hal linker script. It must not set a default `build.target` for the
  whole workspace, or `cargo test -p chatter-core` stops running on the Mac.
- **Logging:** there is no `println!` in `no_std`. `utils.rs`-style macros
  would wrap `defmt` or `esp-println` instead. To decide in the scaffold chunk.
- **`unsafe_code = "forbid"`:** may clash with the esp macros
  (`esp_app_desc!`, `#[esp_rtos::main]`). Fine for `chatter-core`; check
  for the firmware.
- **Errors:** core can't use the firmware's `Error`, because the dependency
  runs the other way. Core needs its own error type, or `error.rs` moves into
  core. To decide when core is written.

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
  it bytes. Something like
  `fn decode(header: &[u8], bssid: Mac) -> Option<Frame>` in
  `core/src/models/`, tested on the Mac against the address table. The exact
  shape is settled when core is written.

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
- **Credentials.** The SSID and PSK come in through `env!` at build time from
  a git-ignored `.env`. Never commit them, and keep them out of logs.
- **Unstable API.** Pin versions (see Stack).

## Still unsure

1. Does the sniffer deliver frames while the station is connected on
   esp-radio 1.0.0-beta.2? This is the first thing the spike settles.
2. How much of the router's 802.11ax traffic the C6 decodes, and how much
   arrives as MIMO length-only.
3. ~~Is `RxControlInfo` the same on the C6 as on the ESP32?~~ No. The C6 uses
   the second layout (`wifi_mac_version = "2"`, with 802.11ax fields): `rssi`,
   `rate`, `sig_len`, `channel`, `noise_floor`, `is_group` and more. This was
   read from the esp-radio 1.0.0-beta.2 source and still needs confirming on
   hardware.
4. Callback load on a busy channel: how often the channel fills and frames
   are dropped.

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
6. **Sniffer spike:** sniffer on next to the station, printing per-MAC frame
   counts every 10 s.
   Check: a known 2.4 GHz device downloads a large file, and its rx bytes
   jump by roughly that amount.
7. **Core:** `Frame`, `Counters` and the aggregation handler.
   Check: unit tests on the host for the address table above.
8. **Report** to a collector stub on the Mac.
   Check: reports arrive every 60 s, with `seq` going up and `dropped`
   reported.
9. **Collector on the home server.**
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
  ahead of 1.0.0-beta.2.
- esp-radio 1.0.0-beta.2 source (`src/wifi/sniffer.rs`, `src/wifi/mod.rs`),
  read from the crates.io tarball.
- `esp-generate` 1.4.0 template (`src/template_files.rs`): the release set and
  the probe-rs runner line.
- [ESP-IDF Wi-Fi sniffer mode (C6)](https://docs.espressif.com/projects/esp-idf/en/stable/esp32c6/api-guides/wifi-driver/wifi-modes.html)
- [Espressif forum: transmitting without an antenna](https://esp32.com/viewtopic.php?p=44218)
