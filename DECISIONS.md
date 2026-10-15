# Decisions

## 2026-10-15 - Embassy on esp-hal + esp-radio, not esp-idf-svc

**Decision:** The firmware is `no_std` on `esp-hal` 1.2, with `esp-radio` 1.0.0-beta.2 (`wifi`, `sniffer`, `unstable`), `esp-rtos` 0.4 for Embassy, and `embassy-net`. It builds on stable Rust for `riscv32imac-unknown-none-elf`. Versions are pinned, as one release set (see `docs/sensor.md`, Stack).
**Why:** Embassy is the preferred async runtime, and plain cargo on stable keeps the toolchain light: no `espup`, no ESP-IDF download. esp-hal's repo has a C6-capable `sniffer` example (on unreleased `main`, with path dependencies). Underneath it's the same Espressif Wi-Fi driver as ESP-IDF, so the sniffer behaves the same. The cost is that the sniffer API is `unstable` and changed between releases, hence the pins. We picked the 1.2 set over the older esp-hal 1.1 + esp-radio 0.18 set because `esp-generate` 1.4 emits it, and it saves a 0.18 → 1.0 migration later.
**Alternatives:** `esp-idf-svc` (std) calling `esp_wifi_set_promiscuous_rx_cb` through `esp-idf-sys` (a stable C API and ESP-IDF's HTTP/NVS, but a heavy first build and nightly `build-std`); C on ESP-IDF (no Rust); esp-hal 1.1 + esp-radio 0.18 (older, and a migration ahead).

## 2026-10-15 - Tools come from crates.io and rustup only

**Decision:** Everything installs with `rustup` (read from `rust-toolchain.toml`) or `cargo install --locked <crate>@<version>`. No cloned repos, no hand-built C libraries, no `espup`. Every step is logged in `docs/setup.md`, so a fresh machine can be rebuilt from it.
**Why:** On the last ESP project, extra C libraries piled up and couldn't be reinstalled cleanly. Crates that compile some C during `cargo install` (probe-rs builds `hidapi` with Xcode's clang) are fine, as long as nothing has to be fetched or built by hand. The Wi-Fi driver is Espressif's prebuilt blob, shipped inside `esp-wifi-sys-esp32c6`; no Rust Wi-Fi stack avoids that.
**Alternatives:** `espup` (needed only for Xtensa chips, not the RISC-V C6); ESP-IDF's installer (pulls a C toolchain and Python).

## 2026-10-15 - Core is its own crate, tested on the Mac

**Decision:** A Cargo workspace: `core/` is a `no_std` library with no esp dependencies, and `firmware/` is the binary (main, config, error, services), which depends on core.
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
