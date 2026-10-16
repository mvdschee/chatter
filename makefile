FIRMWARE = -p chatter-firmware --target riscv32imac-unknown-none-elf
# testclient/ is its own workspace, so cargo runs from inside it
TESTCLIENT = --manifest-path testclient/Cargo.toml --target riscv32imc-unknown-none-elf

# With several boards plugged in, probe-rs needs to know which one: the probe
# IDs live in .env (CHATTER_PROBE_C6, CHATTER_PROBE_C3), because they contain
# the boards' MAC addresses.
probe = $(shell grep '^CHATTER_PROBE_$(1)=' .env 2>/dev/null | cut -d= -f2-)
C6_PROBE = $(if $(call probe,C6),PROBE_RS_PROBE=$(call probe,C6))
C3_PROBE = $(if $(call probe,C3),PROBE_RS_PROBE=$(call probe,C3))

run:
	$(C6_PROBE) cargo run $(FIRMWARE)

test-client:
	$(C3_PROBE) cargo run $(TESTCLIENT)

test:
	cargo test -p chatter-core

test-device:
	$(C6_PROBE) cargo test $(FIRMWARE)

check:
	cargo clippy -p chatter-core --all-targets -- -D warnings
	cargo clippy -p chatter-core --target riscv32imac-unknown-none-elf -- -D warnings
	cargo clippy $(FIRMWARE) --bins --tests -- -D warnings
	cargo clippy $(TESTCLIENT) -- -D warnings
	cargo fmt --check
	cargo fmt --check --manifest-path testclient/Cargo.toml
