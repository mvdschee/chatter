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
- collector: `make collector` _(works)_ runs `crates/collector` (Rust, std plus
  `serde_json`) on `CHATTER_COLLECTOR_ADDR` from `.env`, and appends each
  report to `/tmp/chatter-reports.jsonl`. The firmware build needs that key
  too: reports go to `POST /report` there.
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
- Three terminals: `make collector-test` (the collector, plus a 10 MiB
  `GET /test.bin`), `make run` (sensor), `make test-client` (C3).

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
  station joins the IoT network on channel 6 with power save off. The first
  attempt or two after boot fail (`AuthenticationFailed`,
  `FourWayHandshakeTimeout`) before it connects; the retry loop covers it.
- 2026-10-15: router restart test. The board dropped (`SaQueryTimeout`),
  retried through `NoAccessPointFound` while the router booted, and came
  back on the same BSSID and channel by itself.
- 2026-10-15: sniffer spike (chunk 6). Promiscuous mode works next to the
  connected station. About 85 frames/s on channel 6, and no header-less
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
- 2026-10-16: the first hour-long run showed 453 MACs against 22 real devices.
  Probe requests and responses are now left out of device counts.
- 2026-10-16: second hour-long run, next to the router: 25 MACs (20 known),
  stronger signal. The non-joined network still only showed up at uptime 789 s,
  so BSSIDs are now also learned from probe responses, and reports carry
  `nets_known`.
- 2026-10-16: open issue. With the probe-response firmware, both networks were
  known within the first minute, but only report 0 arrived; after that every POST
  failed (`send`, then `connect`). Ruled out: the probe-rs/RTT attachment and
  the Mac (firewall off, address unchanged). The old firmware, which had sent
  53 reports in a row earlier that day, then couldn't connect to Wi-Fi at all,
  so the cause is most likely outside the code. Suspect: the antenna connector
  after moving the board. **The board currently runs the old firmware.** Next:
  check the antenna, reflash (`make run`), run 10 minutes with `make collector`.
