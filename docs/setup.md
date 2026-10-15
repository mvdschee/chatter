# Setup

How to get from a clean Mac to flashing the C6, and how to take it all away
again. This is filled in as we go: a step counts as done only once it has
actually been run. Until then it's marked _not run yet_.

## Rules

- Everything comes from `rustup` or `cargo install --locked <crate>@<version>`.
  No cloned repos, no hand-built C libraries, no `espup`, no ESP-IDF.
- Versions are pinned here and in `rust-toolchain.toml`. Upgrading is a
  deliberate step: change the version here, reinstall, and add a log entry.
- Some crates compile a little C while installing (probe-rs builds `hidapi`).
  That's fine. It uses Xcode's clang and needs nothing extra.
- One prebuilt binary is unavoidable: Espressif's Wi-Fi driver, inside
  `esp-wifi-sys-esp32c6`. It's a normal cargo dependency of the firmware, so
  there is nothing to install for it.

## Prerequisites

| What | Why | Check |
|---|---|---|
| `rustup` | installs the pinned toolchain and target | `rustup --version` |
| Xcode Command Line Tools | the C compiler some crates use while installing | `xcode-select -p` |

Both are already on this Mac (2026-10-15).

## 1. Toolchain _(not run yet)_

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

Check: `rustup show` lists `riscv32imac-unknown-none-elf` under the active
toolchain.

## 2. Tools _(not run yet)_

```sh
cargo install --locked probe-rs-tools@0.32.0   # probe-rs, cargo-flash, cargo-embed
cargo install --locked esp-generate@1.4.0      # reference scaffold only
```

- **probe-rs** flashes, prints logs over RTT, debugs and runs on-device tests.
  It talks to the C6's built-in USB-JTAG through the board's USB-C port.
  On macOS that needs no driver.
- **esp-generate** generates a fresh project in a temp dir, to copy from. It
  never writes into this repo.

Check: `probe-rs list` shows the C6 with the board plugged in. This needs no
antenna, because Wi-Fi stays off.

## 3. Build, flash, test _(not run yet)_

Filled in by Plan steps 2–4 in `docs/sensor.md`:

- core tests on the Mac: `cargo test -p chatter-core`
- firmware build, flash and log: _tbd_
- on-device tests: _tbd_

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
