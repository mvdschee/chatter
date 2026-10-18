# Setup

How to get from a clean Mac to flashing the C6, and how to take it all away
again. This is filled in as we go: a step counts as done only once it has
actually been run. Until then it's marked _not run yet_.

## Rules

- Everything comes from `rustup` or `cargo install --locked <crate>@<version>`.
  No cloned repos, no hand-built C libraries, no `espup`, no ESP-IDF.
- Crates come through a local panamax mirror, which `~/.cargo/config.toml`
  sets up with `replace-with` (outside this repo). Without it, cargo falls
  back to crates.io and nothing else changes.
- Toolchains come through the same mirror: `RUSTUP_DIST_SERVER` and
  `RUSTUP_UPDATE_ROOT` are set in `~/.zshenv`. The mirror only serves the
  versioned manifest (`dist/channel-rust-<version>.toml`) for versions
  pinned in panamax's config. When `rust-toolchain.toml` changes, add the
  new version there and re-sync first, or the install fails with a 404.
- Versions are pinned here and in `rust-toolchain.toml`. Upgrading is a
  deliberate step: change the version here, reinstall, and add a log entry.
- Some crates compile a little C while installing (probe-rs builds `hidapi`
  and `capstone`). That's fine. It uses Xcode's clang and needs nothing extra.
- One prebuilt binary is unavoidable: Espressif's Wi-Fi driver, inside
  `esp-wifi-sys-esp32c6`. It's a normal cargo dependency of the firmware, so
  there is nothing to install for it.

## Prerequisites

| What                     | Why                                             | Check              |
| ------------------------ | ----------------------------------------------- | ------------------ |
| `rustup`                 | installs the pinned toolchain and target        | `rustup --version` |
| Xcode Command Line Tools | the C compiler some crates use while installing | `xcode-select -p`  |

Both are already on this Mac (2026-10-15).

## 1. Toolchain _(done 2026-10-15)_

`rust-toolchain.toml` at the repo root:

```toml
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt", "rust-analyzer"]
targets = ["riscv32imac-unknown-none-elf"]
```

Then, inside the repo:

```sh
rustup toolchain install   # no arguments: reads rust-toolchain.toml
```

esp-hal 1.2 needs Rust ≥ 1.95.

Never run `rustup show` or `cargo` in the repo while `rust-toolchain.toml`
names a channel that isn't installed: rustup auto-installs whatever is
written there, typos included (it warns that this behaviour is deprecated).
Use `rustup toolchain install` on purpose instead.

Check: `rustup show` lists `riscv32imac-unknown-none-elf` under the active
toolchain.

Result: the download took seconds, and `rustup show` reports `1.99.0`,
"overridden by rust-toolchain.toml", with both `aarch64-apple-darwin` and
`riscv32imac-unknown-none-elf` installed.

## 2. Tools _(done 2026-10-15)_

```sh
cargo install --locked probe-rs-tools@0.32.0   # probe-rs, cargo-flash, cargo-embed
cargo install --locked esp-generate@1.4.0      # reference scaffold only
```

- **probe-rs** flashes, prints logs over RTT, debugs and runs on-device tests.
  It talks to the C6's built-in USB-JTAG through the board's USB-C port.
  On macOS that needs no driver.
- **esp-generate** generates a fresh project in a temp dir, to copy from. It
  never writes into this repo. It checks crates.io for a newer version on
  each run; pass `--skip-update-check` to stop that.

Check: `probe-rs list` shows the C6 with the board plugged in. This needs no
antenna, because Wi-Fi stays off.

Result: the builds took about 55 s (probe-rs) and 17 s (esp-generate).
`probe-rs list` shows `ESP JTAG -- 303a:1001 (EspJtag)`, and
`probe-rs info` reports `Detected chip: esp32c6`. The probe's serial number
is the board's MAC address, so keep it out of the docs and logs.

## 3. Build, flash, test _(not run yet)_

Filled in by Plan steps 2–4 in `docs/sensor.md`. `make test` runs the core
tests, and `make check` runs clippy (host and riscv32imac) plus a fmt check.

- core tests on the Mac: `cargo test -p chatter-core` _(works)_
- core still `no_std`: `cargo build -p chatter-core --target
  riscv32imac-unknown-none-elf` _(works)_. This fails as soon as
  anything in core pulls in `std`.
- firmware build, flash and log: `make run` _(works)_. It builds for
  riscv32imac, flashes over USB-JTAG, then streams `defmt` logs until Ctrl-C.
  The first build takes about 20 s.
- Not copied from the esp-generate template: `[unstable] build-std` (nightly
  only, and the target ships a prebuilt `core`), `rust-src`, and
  `force-frame-pointers` (only needed for `esp-backtrace`). It builds and runs
  on stable without them.
- Expected at boot: `WARN RcFastDivClk calibration failed`. It comes from
  esp-hal's clock setup and doesn't stop the boot. The cause hasn't been
  looked into.
- collector: `make collector` _(works)_ runs `crates/collector` (axum and
  SQLite). Its settings are environment variables, which it reads from `.env`
  when it runs locally; its container sets the same keys (see its
  `src/config.rs`):
  - `CHATTER_BIND`: where it listens, e.g. `127.0.0.1:8080`. The page is on
    `/`.
  - `CHATTER_DB`: the SQLite file, e.g. `.local/chatter.db` (`.local/` is
    git-ignored).
  - `CHATTER_UI_DIR`: `crates/collector/ui` to read the page from disk on
    every request, so a refresh shows an edit. Unset, the binary's built-in
    copy is served.
  - `CHATTER_TEST_FILE_MIB`: set (to 10) to serve the test client's download.
  - `CHATTER_SCAN_TARGETS`: the networks to sweep with nmap. Unset, the
    collector sweeps the /24 of the network it's on now. On a Mac the sweep
    needs nmap (`brew install nmap`) and root for its ARP requests
    (`make collector SUDO=1`); without them it logs an error and the rest
    works.
  - `CHATTER_SWEEP_MINUTES`: minutes between sweeps, 10 if unset.
  - `CHATTER_PORT_SCAN_AT`: the daily port scan's local time, `18:00` if
    unset, or `off`. A missed one (the collector was down) runs 5 minutes
    after it starts. `make port-scan SUDO=1` runs one now.

  `CHATTER_COLLECTOR_ADDR` is where the sensor sends its reports, a different
  key: for the sensor to reach this collector, it's this machine's LAN address,
  and `CHATTER_BIND` must listen there too. SQLite is compiled from the C
  source that sqlx ships on crates.io, so nothing needs installing; `sqlite3`
  (part of macOS) is handy to look inside.
- import: `make import` loads `.local/reports.jsonl` (or `FILE=...`), a copy
  of the first collector's reports file, into `CHATTER_DB`
  (`chatter-collector import <file>`). Reports already in the database are
  skipped, so a newer copy can be imported again.
- on-device tests: `make test-device` _(works)_. `embedded-test` flashes
  each file in `crates/firmware/tests/` and reports pass or fail like `cargo test`.
  Checked both ways: a broken assertion shows `FAILED` and exits 1.
- `make check` lints the firmware with `--bins --tests`, not
  `--all-targets`: that would also build the binary as a test, which
  needs the `test` crate the C6 doesn't have.

## 4. Test client on an ESP32-C3 _(works)_

`crates/testclient` is a single-stream client that downloads a known file, so
the sensor's counts can be checked against an exact number.

- It's its own workspace (`exclude` in the root `Cargo.toml`), and so is
  `crates/collector`. Inside the main one, the C3's `esp32c3` features would
  merge with the firmware's `esp32c6` ones, and the collector's `std` can't
  build for the C6 target rust-analyzer checks everything for. All three
  workspaces share the repo's `target/` (`build.target-dir` in
  `.cargo/config.toml`).
- The target is `riscv32imc-unknown-none-elf` (the C3 has no atomics
  extension), listed in `rust-toolchain.toml`. The mirror serves it.
- `.env` gets `CHATTER_PROBE_C6` and `CHATTER_PROBE_C3` (the `VID:PID:serial`
  from `probe-rs list`; the serial is the board's MAC). The makefile passes
  them as `PROBE_RS_PROBE`. The client downloads from `CHATTER_COLLECTOR_ADDR`.
- Three terminals: `make collector` with `CHATTER_TEST_FILE_MIB=10` in
  `.env` (it then also serves a 10 MiB `GET /test.bin`), `make run` (sensor),
  `make test-client` (C3).

## 5. Collector image _(works)_

`crates/collector/Dockerfile` builds the collector into a Debian slim image
with nmap. It keeps its database in `/var/lib/chatter/chatter.db`, and runs as
root: the deployment drops every capability except `NET_RAW` and `NET_ADMIN`
and puts it on the host's network, for nmap (see DECISIONS.md).
The build context is the repo root; `.dockerignore` only lets the collector's
sources, migrations and page in. CI
(`.github/workflows/collector-image.yml`) pushes it to ghcr on every change to
the collector on `main`.

Local check (podman, from the repo root):

```sh
podman build -f crates/collector/Dockerfile -t chatter-collector:local .
podman run --rm -p 127.0.0.1:18080:8080 --read-only --tmpfs /tmp \
  -v chatter-data:/var/lib/chatter chatter-collector:local
```

The deployment itself lives in a separate infrastructure repo.

`CHATTER_COLLECTOR_ADDR` in `.env` is where the sensor sends its reports.
Pointed at the home server, the sensor doesn't report to a collector on the
Mac: for that, point it at the Mac and reflash.

## Start over

Everything lives in `~/.rustup` and `~/.cargo/bin`:

```sh
cargo uninstall probe-rs-tools esp-generate
rustup toolchain uninstall 1.99.0
```

Then run steps 1 and 2 again.

## Not covered yet

- **Linux** (for example building on a home server): probe-rs needs udev
  rules for USB access there. Not needed on the Mac.
- **Rescue:** if the firmware ever disables the USB-JTAG pins, `espflash`
  can flash over the ROM bootloader (hold BOOT while plugging in). It isn't
  installed. Add it here if we ever need it.

## Log

- 2026-10-15: picked the release set (esp-hal 1.2.2, esp-radio
  1.0.0-beta.2, esp-rtos 0.4.0) and the tools. Nothing installed yet.
- 2026-10-15: steps 1 and 2. Rust 1.99.0 with the riscv32imac target,
  probe-rs-tools 0.32.0 and esp-generate 1.4.0 installed. probe-rs sees the
  C6 over USB-C.
- 2026-10-15: moved the channel from 1.99.0 to 1.96.0 to match the other
  projects. The tools didn't need reinstalling.
- 2026-10-15: back to 1.99.0. Pinned 1.99.0 in panamax, and the mirror now
  serves `channel-rust-1.99.0.toml`.
- 2026-10-15: workspace with `chatter-core`. Host tests run (0 tests), and
  core builds for riscv32imac.
- 2026-10-15: firmware boots on the C6 and logs a heartbeat every second
  over probe-rs (`make run`).
- 2026-10-15: on-device tests with embedded-test 0.7.2. `smoke` passes on
  the board.
- 2026-10-15: esp-radio 1.0.0-beta.2 turned out to be yanked; pinned
  1.0.0-beta.1 instead (same source apart from an S31 build fix).
- 2026-10-15: Wi-Fi (chunk 5). `.env` is read by `firmware/build.rs`. The
  station joins its network with power save off. The first
  attempt or two after boot fail (`AuthenticationFailed`,
  `FourWayHandshakeTimeout`) before it connects; the retry loop covers it.
- 2026-10-15: router restart test. The board dropped (`SaQueryTimeout`),
  retried through `NoAccessPointFound` while the router booted, and came
  back on the same BSSID and channel by itself.
- 2026-10-15: sniffer spike (chunk 6). Promiscuous mode works next to the
  connected station. About 85 frames/s, and no header-less
  frames.
- 2026-10-15: core (chunk 7). Decode, counters and aggregation; `make test`
  runs 19 host tests.
- 2026-10-16: per-device counts (chunk 8) and the C3 test client. embassy-time
  moved to 0.5.1, because embassy-net 0.9.1 needs it. The C3 got 10,485,962 B;
  the sensor counted about 96% of the frames as new, plus retries.
- 2026-10-16: sequence-number deduplication. The C3 rerun is within 1% of the
  expected on-air bytes (11,338,668 B counted against about 11,253,600 B expected).
- 2026-10-16: reporting (chunk 9). The period is now 60 s; a JSON report goes
  to the collector stub over HTTP. New `.env` key: `CHATTER_COLLECTOR_ADDR`.
- 2026-10-16: everything moved under `crates/` (core, firmware, testclient,
  collector) with one shared `target/`. The Python collector stub and the
  Python file server were replaced by `crates/collector` (Rust). Checked end to
  end: the C3 downloaded `/test.bin` from it, and the sensor's reports arrived.
- 2026-10-16: the first hour-long run showed hundreds of MACs against a couple
  of dozen real devices.
  Probe requests and responses are now left out of device counts.
- 2026-10-16: second hour-long run, next to the router: only real devices,
  stronger signal. The non-joined network still only showed up at uptime 789 s,
  so BSSIDs are now also learned from probe responses, and reports carry
  `nets_known`.
- 2026-10-16: reports after the first failed (`send`, then `connect`). A/B
  flashes showed it wasn't the probe-response change: small reports (~0.4 KB)
  always arrived, ~3 KB reports didn't. Cause: a 1.5 KB send buffer with a 5 s
  timeout, so writing a report had to wait for ACKs midway, which failed once
  the link got slower. Fixed with a send buffer that fits a whole report, a
  15 s timeout and one retry; after that every ~3.2 KB report arrived on the
  first attempt. Also fixed: a report could be sent twice after a long DHCP wait.
- 2026-10-16: outbox (9b) flashed, but the board couldn't join Wi-Fi for over
  10 minutes: 72 failed attempts, mostly `AuthenticationExpired`, plus
  `FourWayHandshakeTimeout` and `AssociationFailed`. This also happened once in
  the afternoon. Hypothesis: the router temporarily refuses the board after many
  failed handshakes. Every boot starts with 1–3 failed attempts, there were
  dozens of boots that day, and the reconnect loop retries every 5 s. Open:
  whether the network is WPA2/WPA3 mixed (that would explain the failing first
  attempts), and a reconnect backoff so the board doesn't hammer the router.
- 2026-10-16: root cause of the failing reports and Wi-Fi trouble: esp-radio's
  default transmit power of 5 dBm. A test that paused sniffing during each POST
  still failed, which ruled out receive-buffer starvation. At 15 dBm, ~3.1 KB
  reports went through on the first attempt. The earlier send-buffer
  explanation was at most part of it, and the "router lockout" was probably the
  same weak signal. Outbox (9b) checked: with the collector stopped for 3
  periods, all reports arrived afterwards, in order, with no gap.
- 2026-10-17: collector image and CI workflow; checked with podman (204, data in
  the volume, uid 65532, read-only root).
- 2026-10-17: collector deployed on the home server; the sensor reports there.
  The collector now ignores connections that close without a request (the
  container healthcheck logged a broken pipe every 30 s).
- 2026-10-18: the first day-long run (about 20 h, sensor next to the router):
  no gaps, no backlog, both networks known throughout. Frames were only
  dropped during the scheduled LAN port scan, when every device talks at once.
  So the frame queue grew from 64 to 256 (3.5 KB), and the sensor no longer
  counts its own MAC as a device.
- 2026-10-18: flashed: the server's reports no longer list the sensor's own MAC.
- 2026-10-18: the collector moves to axum and SQLite (monitoring plan step 2).
  Checked locally and in the podman image: a firmware-style HTTP/1.0 report
  gets `204`, a resend is recognised and not stored twice, gaps are logged,
  bad reports get `400`, and the image runs read-only with its database in the
  volume. Replaying the first day-long run (about 1200 real reports) gave 204
  for all of them; with their original receive times, a per-device, per-hour
  query over the day takes about 15 ms and uses the (mac, time) index. A
  report sent before the first connect has channel 0, so 0 is accepted: a
  rejected report would block the sensor's outbox. The JSON Lines file of
  earlier reports stays in the volume; it isn't imported.
- 2026-10-18: `chatter-collector import`: the home server's reports file (about
  a day and a half) loads into a local database in about 2 s. 17 of its lines
  were resends the first collector had stored twice; the import skips them.
- 2026-10-18: the collector serves the monitoring page on `/` and its data on
  `/v1/overview` (about 120 KB for a day of a few dozen devices). `make collector-local`
  reads the page from disk on every request (`CHATTER_UI_DIR`); the image has
  it built in. Opened as a file, the page still shows its made-up data.
- 2026-10-18: the mockup became the collector's page (`crates/collector/ui/`:
  `index.html`, `style.css`, `app.js`), without its made-up data. The
  collector reads `.env` itself (dotenvy), so `make collector` is the same
  command for every setup and the makefile no longer passes settings.
- 2026-10-18: the collector sweeps the network with nmap (`-sn`) and shows
  hostnames, IPs and vendors. The image now has nmap and runs as root. Checked
  with podman: with only `NET_RAW` and `NET_ADMIN` it can't open a database in
  a volume the old image (uid 65532) created, so the deployment hands the
  volume to root once. nmap's sweep can't be checked in podman on the Mac: it's
  rootless there, and nmap can't open the network device even with every
  capability. The real check is the first sweep on the server.
- 2026-10-18: the first sweep (on the Mac, as root) found MACs and vendors but
  no hostnames: the Mac's DNS server had no reverse entries. Asking the default
  gateway instead (`--dns-servers`, the gateway from `nmap --iflist`) named
  nearly every host. The collector now does that, and it sweeps the network it
  finds itself on unless `CHATTER_SCAN_TARGETS` is set.
- 2026-10-18: the collector port-scans daily (`lan_scan`'s options and time).
  Ports go into `scan_ports`, show in the device panel, and each scan is an
  event in the Timeline panel and a band on the day graph. Locally `.env` has
  `CHATTER_PORT_SCAN_AT=off`. On the server it runs at 18:00, next to
  `lan_scan` until the collector sends the emails too. On the Mac (Wi-Fi) a
  full scan timed out on every host: about 2 million probes in 30 minutes is
  more than Wi-Fi carries with `-T4`; the wired server is the check.
- 2026-10-18: deployed on the home server. The first full port scan from the
  container (host network, root with `NET_RAW` and `NET_ADMIN`) finished with
  no host timed out, so the Mac's timeouts were its Wi-Fi. The next one was
  scheduled for the next day's 18:00 plus its spread.
