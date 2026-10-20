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

## 2026-10-18 - Collector: axum and SQLite through sqlx

**Decision:** The collector becomes an axum service in the standard layout (`core`, `repository`, `services/web`), storing reports in one SQLite file through sqlx with its bundled SQLite. A row per report, per device minute and per broadcast minute; the device rows carry the report's time, so per-device queries over time need no join. A resent report (same sensor, `seq` and uptime) is answered `204` and not stored again.
**Why:** It may become a product: one file next to the binary is something a buyer can run and back up, with no database server. sqlx matches the other services' generic repository, and its bundled SQLite comes from crates.io, so no system library or hand-built C. Plain axum (hyper) still speaks the firmware's HTTP/1.0.
**Alternatives:** Postgres (a second container to run and update, for one writer at a row a minute); rusqlite (synchronous, a different repository style); keeping the hand-rolled std server (no routing, middleware or SSE for the live view).

## 2026-10-18 - Collector on the LAN without authentication

**Decision:** For now nothing is authenticated: anyone on the LAN can post a report. The port is bound to the server's LAN address only.
**Why:** The sensors have no way to get a secret yet besides a reflash. A per-sensor token belongs with setting sensors up without reflashing, which a product needs anyway.
**Alternatives:** A shared token in `.env`, built into the firmware (a reflash to rotate, and the same secret in every board).

## 2026-10-18 - Collector settings only from the environment, `.env` locally

**Decision:** The collector reads every setting from environment variables (`CHATTER_BIND`, `CHATTER_DB`, `CHATTER_UI_DIR`, `CHATTER_TEST_FILE_MIB`). Locally it loads the repo's `.env` itself (dotenvy); its container sets the same keys. The makefile only runs it.
**Why:** One way to configure it, the same for a local run and the image, and a makefile without per-setup targets. Reading `.env` in the collector rather than including it in the makefile keeps make from parsing values like the Wi-Fi password, where `#` and `$` mean something to make.
**Alternatives:** Per-setup make targets with the values inline (what it was; grew a target per case); `include .env` in the makefile (breaks on some passwords).

## 2026-10-18 - The collector runs the network scans itself

**Decision:** The collector runs nmap: a host sweep (`-sn`) every few minutes, and a daily port scan at `CHATTER_PORT_SCAN_AT` (18:00, plus up to an hour) with the infra repo's `lan_scan` options (every TCP port, the UDP ports IoT devices use, `-sV --version-light`, 30 min per host at most). A missed port scan runs once the collector is back, like a systemd timer's `Persistent=true`. Its container runs on the host network as root, with every capability dropped except `NET_RAW` and `NET_ADMIN`, and a read-only root filesystem. It sweeps the /24 of the network its machine is on, looked up before each sweep from the default route's local address (a UDP connect that sends nothing), unless `CHATTER_SCAN_TARGETS` names targets; those are checked to be IP addresses or CIDR ranges before nmap gets them.
**Why:** One app that does it all, which a buyer installs once. nmap only reports MACs on its own network segment (Docker's bridge network is a routed hop), and its ARP sweep needs raw sockets. Root with two capabilities was chosen over a non-root user with `setcap` on nmap, which would mean dropping `no-new-privileges`.
**Alternatives:** A scanner outside the app that posts its results (the earlier plan: two things to install); a separate privileged scanner container next to an unprivileged collector (more to deploy); non-root with file capabilities (needs `no-new-privileges` off).

## 2026-10-18 - Changes between port scans: lan_scan's rules, stored per scan

**Decision:** After each port scan, the collector compares every host that didn't time out with the port scan before that saw it, and stores the changes (`scan_changes`): a new device or new port is an alert; a closed port, a changed product or version, and a new high port (from 32768) on a device that has had a high port close before are information. The first port scan is the baseline. They're Timeline events; emails come later.
**Why:** The same rules the infra repo's `lan_scan` had worked out, so switching it off loses nothing but the emails. Storing the changes, rather than working them out on every page load, keeps "has rotated before" a simple lookup and gives the emails something to read.
**Alternatives:** Working out the changes in the overview query each time (no history of what was alerted); a per-device state blob like `lan_scan`'s `state.json` (doesn't fit the database).

## 2026-10-19 - Vendors from IEEE's registry, over reqwest with rustls

**Decision:** The collector downloads IEEE's MA-L registry (`oui.csv`) into its data directory when it's missing or a month old, and a scan takes a host's vendor from it before nmap's. The download uses reqwest with rustls (no OpenSSL) and the User-Agent `chatter`; the image has `ca-certificates`. A failed download keeps the old copy.
**Why:** nmap's vendor list is as old as its package, so newer devices show up without one; the infra repo's `lan_scan` did the same for that reason. reqwest is what the other services use for HTTP; rustls keeps the build to crates.io.
**Alternatives:** nmap's list only (gaps for new devices); ureq (blocking, a second style); shipping the CSV in the image (stale between image builds).

## 2026-10-19 - Timeline buckets computed from the stored minutes, average and peak

**Decision:** `GET /v1/timeline` takes a range (1h–30d, ending at the newest report) or a from/to stretch, picks the smallest bucket that keeps it under 400 buckets (the classic traffic-graph tiers: minutes, 5 min, 30 min, 2 h), and works out each bucket's average and peak minute from `device_minutes` on every request. No rollup tables. Details in `docs/timeline.md`.
**Why:** A home's minutes are few enough for SQLite to group on the fly (30 days in about 0.1 s), and computing from the raw minutes means no rollup job and nothing to keep in sync. The peak is kept because an average can't be turned back into one: a 5-minute port scan would vanish at 30 days.
**Alternatives:** RRD-style rollup tables per tier (worth it once a year of minutes makes the query slow); a fixed 24-hour day in the overview (what it was).

## 2026-10-19 - Baselines per device and hour, computed hourly in the collector

**Decision:** Once an hour the collector works out each device's median and MAD of `ln(1 + bytes a minute)` per hour of the day over the 14 days up to the newest report, and replaces the `baselines` table. A minute the device wasn't heard in counts as 0, from its first minute in the window on; port scan minutes are left out; an hour with under 60 of its minutes stores the device's all-hours baseline, keeping its own `n`. The all-hours baseline is also stored as hour 24: timeline buckets longer than an hour span several hours and are scored against it, and its `n` says whether the device is still learning. The page gets the score's constants (the MAD floor and the white and red thresholds) from the overview. Hours are in the collector's time zone, the same clock as `CHATTER_PORT_SCAN_AT`. Details in `docs/outliers.md`.
**Why:** SQLite has no median; in Rust, two days of a home's minutes took about 0.3 s with the database opening included, so 14 days should stay within seconds. Storing the fallback means the page gets 24 final pairs per device and needs no rule of its own. One clock for the hours, sent with the data, keeps a browser in another time zone from scoring against the wrong hour.
**Alternatives:** Computing on every page load (repeats the same work every request); zeros from the window's start (a new device looks idle and goes red); letting the page fall back to the all-hours row for thin hours (the rule twice, in Rust and JS); the score's constants in JS (the floor's derivation in two places).

## 2026-10-19 - Floor plan: rooms as polygons, one JSON document per save

**Decision:** The floor plan is a list of rooms, each a polygon of corners in metres, plus openings on room edges; walls are the rooms' edges. It's stored as one JSON document in the `floorplans` table, a new row per save, and the newest row is the plan. Details in `docs/floorplan.md`.
**Why:** The app needs rooms more than walls (which room a device is in), and with polygons that's a point-in-polygon test instead of face detection that has to be exactly right. A home is a few dozen corners, always loaded and edited whole, so tables per corner and wall would only add joins; a row per save keeps older versions to go back to.
**Alternatives:** Walls first with rooms worked out as faces of the wall graph (react-planner: snapping and splitting must be exact, and doorways break faces); tables for rooms, corners and openings (more SQL, no query needs them apart).

## 2026-10-19 - The view fits the plan to its frame, walls split at corners

**Decision:** The page scales a plan in metres to fit 1000 by 600 plan units, the same scale both ways, and the view's frame takes the plan's shape with a 40-unit margin, so any home fills the view. Walls come from the rooms' edges, each split at every corner that lies on it and kept once per pair of corners; a doorway cuts every wall dot in its stretch, whichever room's edge names it.
**Why:** The look (wall height, dot spacing, packet pace, glow sizes) was tuned in plan units; a fixed scale per metre would need retuning them all and would put more dots in a bigger home. Splitting at corners makes rooms that share part of a wall (a long room against two short ones) give the same pieces, so nothing is drawn twice and a door named on the short edge cuts the long one too.
**Alternatives:** A fixed number of plan units per metre (walls and glows out of proportion between a flat and a house); matching shared edges only by their two corners (fails at T-junctions, which every row of rooms has).

## 2026-10-19 - The page as ES modules by domain, no build step

**Decision:** The page's script is split into ES modules by domain (`main.js`, `data.js`, `plan.js`, `geometry.js`, `view.js`, `draw.js`, `playback.js`, `timeline.js`, `panel.js`, `sensors.js`, `colour.js`, `format.js`, `random.js`), loaded by the browser as they are, with no bundler. State lives in the module that changes it, exported with `let`, which importers read live; the playhead and the frame clock, changed from several modules, are one exported `play` object. The collector serves the files from a list, and a test fails when a file in `ui/` is missing from it.
**Why:** One 1900-line file was getting hard to work in, and the plan editor would add a lot more. Browsers load modules natively, so a bundler would only add a toolchain; the geometry module has no page in it, so it can be checked in node.
**Alternatives:** Keep one file (the editor would double it); a bundler such as esbuild (a build step and a dependency for no gain on a LAN page); one shared state object for everything (every module reads and writes everything).

## 2026-10-19 - The plan editor draws in metres on its own canvas; geometry tested in node

**Decision:** Drawing rooms happens on a second canvas over the view (`edit.js`), straight from above, in metres with its own zoom and pan; the 3D view isn't drawn while it's open. Snapping and the crossing checks are plain functions in `geometry.js`, tested with `node --test` from `crates/collector/ui-tests/`, which needs no npm or package file. The page repeats the collector's crossing check so a bad room is refused while drawing, not on save.
**Why:** The view works in plan units that `toUnits` refits to the plan's bounds, so every new corner would rescale what's being drawn, and its tilt and drift make clicking corners hard. The tests sit outside `ui/` because the collector serves every file in there. Refusing while drawing saves redrawing a room the save would reject.
**Alternatives:** Editing in the 3D view with the tilt turned off (plan units shift under the pointer); tests in `ui/` (served to the browser, and the served-files test fails on them); only the collector checking (the problem shows up at save time, after the room is drawn).

## 2026-10-20 - The underlay: one image file beside the database, measured from one wall

**Decision:** The floor plan's underlay image is one file, `plan-underlay`, in the collector's data directory, stored through `PUT /v1/plan/underlay` (PNG, JPEG or WebP by their first bytes, up to 20 MB) and replaced by the next one. The plan JSON keeps where it lies: pixels per metre, the top left corner's offset and an angle. It's scaled from two clicks on a wall's ends and its typed length, and a wall within 10° of level or upright turns square. Details in `docs/floorplan.md`.
**Why:** In the plan JSON the image would blow the plan route's 512 KB limit and be copied into every saved row; a file next to the database is on the same volume in the container. Turning a slightly crooked scan square keeps it from fighting the square snapping, and it comes free with the two points the scale needs.
**Alternatives:** The image in SQLite (a blob per save, or a table of its own); a data URL in the plan; a separate step to rotate the image; reading PDFs in the page with pdf.js (a large dependency, when exporting a page as PNG is one step).
