# Decisions

## 2026-10-15 - Embassy on esp-hal + esp-radio, not esp-idf-svc

**Decision:** The firmware is `no_std` on `esp-hal` 1.2, with `esp-radio` 1.0.0-beta.1 (`wifi`, `sniffer`, `unstable`), `esp-rtos` 0.4 for Embassy, and `embassy-net`. It builds on stable Rust for `riscv32imac-unknown-none-elf`. Versions are pinned, as one release set (see `docs/sensor.md`, Stack).
**Why:** Embassy is the preferred async runtime, and plain cargo on stable keeps the toolchain light: no `espup`, no ESP-IDF download. esp-hal's repo has a C6-capable `sniffer` example (on unreleased `main`, with path dependencies). Underneath it's the same Espressif Wi-Fi driver as ESP-IDF, so the sniffer behaves the same. The cost is that the sniffer API is `unstable` and changed between releases, hence the pins. We picked the 1.2 set over the older esp-hal 1.1 + esp-radio 0.18 set because `esp-generate` 1.4 emits it, and it saves a 0.18 → 1.0 migration later.
**Alternatives:** `esp-idf-svc` (std) calling `esp_wifi_set_promiscuous_rx_cb` through `esp-idf-sys` (a stable C API and ESP-IDF's HTTP/NVS, but a heavy first build and nightly `build-std`); C on ESP-IDF (no Rust); esp-hal 1.1 + esp-radio 0.18 (older, and a migration ahead).

## 2026-10-15 - Tools come from crates.io and rustup only

**Decision:** Everything installs with `rustup` (read from `rust-toolchain.toml`) or `cargo install --locked <crate>@<version>`. No cloned repos, no hand-built C libraries, no `espup`. Every step is logged in `docs/setup.md`, so a fresh machine can be rebuilt from it.
**Why:** On the last ESP project, extra C libraries piled up and couldn't be reinstalled cleanly. Crates that compile some C during `cargo install` (probe-rs builds `hidapi` with Xcode's clang) are fine, as long as nothing has to be fetched or built by hand. The Wi-Fi driver is Espressif's prebuilt blob, shipped inside `esp-wifi-sys-esp32c6`; no Rust Wi-Fi stack avoids that.
**Alternatives:** `espup` (needed only for Xtensa chips, not the RISC-V C6); ESP-IDF's installer (pulls a C toolchain and Python).

## 2026-10-15 - Core is its own crate, tested on the Mac

**Decision:** A Cargo workspace: `crates/core/` is a `no_std` library with no esp dependencies, and `crates/firmware/` is the binary (main, config, error, services), which depends on core.
**Why:** A test binary built for `riscv32imac` can't run on the Mac. Core is pure logic (header bytes in, counters out), so it should be tested with a plain `cargo test` in seconds. Cargo then enforces that core never imports esp code.
**Alternatives:** One crate with esp deps gated to `riscv32` and tests run with `--target aarch64-apple-darwin` (it works, but only convention keeps core clean); all tests on the device (a flash cycle per run, which doesn't help pure logic).

## 2026-10-15 - probe-rs to flash, debug and run on-device tests

**Decision:** `probe-rs` (`probe-rs-tools` on crates.io) is the cargo runner. It flashes, prints logs over RTT and gives breakpoints over the C6's built-in USB-JTAG. Adapter tests that need the chip run on the board with `embedded-test`. `espflash` is not installed by default.
**Why:** The board's USB-C goes straight to the C6, so no extra probe hardware is needed. One tool covers flashing, logging, debugging and device tests. esp-hal tests itself the same way.
**Alternatives:** `espflash` (flash and serial monitor only, no debugger; kept as a rescue tool if JTAG ever becomes unreachable).

## 2026-10-15 - Decode frames in the sniffer callback, with core's decode fn

**Decision:** The sniffer callback calls a decode function from `chatter-core` on the raw header and queues the small `Copy` `Frame` it returns, not the raw bytes. No `ieee80211` crate; the decode reads three fixed offsets.
**Why:** esp-radio doesn't parse 802.11 headers, and its `&[u8]` is gone when the callback returns, so something gets copied either way. A decoded `Frame` (about 12 bytes) is smaller than the raw header plus metadata (about 32), and the callback has to read the header anyway for the BSSID filter. Core still owns the decode and tests it on the Mac, the same way `TryFrom<PgRow>` lives in core.
**Alternatives:** Queue raw header bytes and decode in core's aggregation task (a cleaner split, but double the queue memory and a second read of the header); the `ieee80211` crate (zero-copy, but a dependency for three offsets).

## 2026-10-15 - Logging with defmt over RTT

**Decision:** The firmware logs with `defmt` through `rtt-target`, and panics go through `panic-rtt-target`. probe-rs decodes and prints them.
**Why:** With probe-rs as the runner, `esp-generate` only offers this pairing. defmt sends compact indices instead of formatted strings, which keeps flash and CPU cost low. That matters once the sniffer callback is busy.
**Alternatives:** `log` + `esp-println` over the USB serial port (readable without probe-rs, but it formats on the device and needs espflash as the monitor); `utils.rs`-style `println!` macros like the server projects (there's no `println!` in `no_std`).

## 2026-10-15 - Learn watched BSSIDs from beacons, not a scan

**Decision:** The sniffer maps each watched SSID to its BSSID from beacon frames. The joined network's BSSID comes from the connect result.
**Why:** `scan_async` in esp-radio 1.0.0-beta.1 returned 0 access points on the C6, both before and after connecting. Beacons carry both the SSID and the BSSID, need no extra API, and keep working if the router changes BSSIDs.
**Alternatives:** Debug esp-radio's scan (open-ended); BSSIDs in `.env` (simple, but breaks when the router changes).

## 2026-10-15 - Byte formatting writes into a caller buffer, with integer math

**Decision:** `format_bytes(bytes: u32, buf: &mut [u8; 16]) -> usize` in core writes `"<value> <unit>"` into the buffer (`512 B`, `1.5 KiB`; one truncated decimal above bytes), and `&buf[..len]` is the text. Units are binary (`KiB`/`MiB`/`GiB`, dividing by 1024), up to GiB, which is as far as a `u32` goes. Integer math only.
**Why:** Core is `no_std` without `alloc`, so there is no `String` to return, and the C6 (RV32IMAC) has no FPU. The fixed 16-byte array makes "fits any `u32`" a compile-time fact, so there is no truncation path. Binary units are labelled as such, because a `kB`/`MB` label on 1024-based values is off by 2.4–4.9%, as large as the Wi-Fi overhead the download test measures.
**Alternatives:** f64 scaling into a `String` (needs `alloc`, and soft-float on the C6); returning `(value, unit)` and formatting in the firmware (digit math in two places); decimal units dividing by 1000 (fine too, but file sizes and tools disagree on which they mean).

## 2026-10-16 - Validate counts with an ESP32-C3 test client

**Decision:** `crates/testclient/` (its own workspace) runs on an ESP32-C3 and downloads a known file from a LAN HTTP server. The sensor's counts for its MAC are compared with the exact byte count the C3 logs.
**Why:** The sensor can only decode single-stream frames. A multi-stream (MIMO) laptop was invisible, so it can't validate anything. The C3 is single-stream like most IoT gear, and it reports exactly what it received.
**Alternatives:** A real IoT device compared against router stats (the router has no per-client stats); a phone or laptop (MIMO, invisible).

## 2026-10-16 - Deduplicate retries by sequence number, not the retry bit

**Decision:** Every retry counts toward `retries`. Its frames and bytes only count when its sequence control differs from the last one seen for that device and direction, that is, when the sensor missed the original.
**Why:** Dropping every retry undercounted a known 10 MiB download by about 4%: when the sensor missed a first transmission, the retry was the only copy it heard. With deduplication the count is within 1% of the expected on-air bytes.
**Alternatives:** Ignore the retry bit (inflates bytes on noisy links); keep a window of recent sequence numbers per device (catches A-MPDU block-ack retries that arrive out of order, but costs more memory per device for well under 1%).

## 2026-10-16 - Reports: JSON in a fixed buffer, posted by a separate task

**Decision:** Core writes the report as JSON with `core::fmt` into a 24 KiB buffer (no allocation, and the full-table worst case fits, checked by a test). The aggregate task fills one shared pending buffer and signals a report task, which posts it with plain HTTP/1.0 (`POST /report`, `Content-Length`). If the previous report is still sending, the new one is skipped and the gap shows in `seq`.
**Why:** Posting inside the counting loop would stall the frame queue for as long as the collector takes, and the queue only holds 64 frames. With one pending buffer, a slow or missing collector costs reports, never counts. HTTP/1.0 with a known length needs no chunking or keep-alive code.
**Alternatives:** Stream the JSON into the socket (no 24 KiB buffer, but the Content-Length is unknown and serialization would hold the table during network I/O); `reqwless` (more dependencies for one POST); MQTT (needs a broker).
**Update 2026-10-16:** the socket's send buffer is also 24 KiB, the timeout 15 s, and a failed POST is retried once. The failing reports that led to this turned out to be caused mainly by the transmit power (see below); the bigger buffer and the retry stay as headroom. Unsent reports now wait in an outbox (`crates/core/src/models/outbox.rs`) instead of one pending slot.

## 2026-10-16 - All crates under crates/, all tooling in Rust

**Decision:** Every crate lives in `crates/`: `core` and `firmware` form the main workspace, while `testclient` (ESP32-C3) and `collector` (host, std) are their own workspaces. All of them share one `target/`, and the makefile is the single entry point. The development collector, which also serves the test client's download, is Rust: std plus `serde_json`.
**Why:** One place for code, and no Python next to the Rust. The extra workspaces aren't arbitrary: Cargo merges chip features within a workspace (`esp32c3` with `esp32c6`), and rust-analyzer checks the whole workspace for the C6 target, where std doesn't exist.
**Alternatives:** One workspace with `forced-target` per crate (nightly only); keeping the Python stub (works, but a second language to maintain).

## 2026-10-16 - Learn BSSIDs from probe responses too

**Decision:** `announcement()` in core takes the SSID→BSSID mapping from probe responses as well as beacons. The report lists which networks are known (`nets_known`).
**Why:** In two runs, both far from the router and right next to it, the network the sensor doesn't join was only learned at uptime 789–790 s. While connected, the C6's driver passes almost no beacons from other access points, but it does pass their probe responses, which carry the same SSID element. `nets_known` makes this visible at the collector, without USB logs.
**Alternatives:** BSSIDs in `.env` (breaks when the router changes them); guessing the second BSSID from the first (vendor-specific).

## 2026-10-16 - Transmit power 15 dBm instead of esp-radio's 5 dBm

**Decision:** `wifi::init` sets `set_max_tx_power(60)` (15 dBm, in 0.25 dBm steps).
**Why:** At esp-radio's default of 5 dBm, the router heard the board's small frames but lost full-size ones. Reports over ~1.5 KB failed, handshakes failed at boot, and some stretches the board couldn't join at all, depending on distance and how busy the air was. esp-radio's docs describe exactly this ("connection failures on boards with a weak antenna") and give [8, 84] as the range, with authentication failures reported above about 65. 60 is the highest value below that. At 15 dBm every ~3.1 KB report arrived on the first attempt.
**Alternatives:** Pausing promiscuous mode during each POST (tested, didn't help, so receive-buffer starvation wasn't the cause); more receive buffers (same reason); the maximum of 84 (above the reported authentication-failure threshold).

## 2026-10-17 - The collector ships as a container image from CI

**Decision:** `crates/collector/Dockerfile` plus a GitHub Actions workflow push `ghcr.io/<owner>/chatter-collector:latest`. The deployment (compose, ports, volume) lives in a separate infrastructure repo and only pulls the image.
**Why:** It matches how the owner's other services deploy: rerunning one playbook picks up a new version, and nothing about the server lives in this public repo. The image builds from the repo root with a deny-all `.dockerignore`, so only the collector's sources go in.
**Alternatives:** A cross-compiled binary plus a systemd unit (needs a Linux linker on the Mac); building on the server (a Rust toolchain there).
