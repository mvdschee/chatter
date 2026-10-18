FIRMWARE = -p chatter-firmware --target riscv32imac-unknown-none-elf
# crates/testclient and crates/collector are their own workspaces
TESTCLIENT = --manifest-path crates/testclient/Cargo.toml --target riscv32imc-unknown-none-elf
COLLECTOR_CRATE = --manifest-path crates/collector/Cargo.toml

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

# development collector on this machine, at CHATTER_COLLECTOR_ADDR from .env
COLLECTOR = $(shell grep '^CHATTER_COLLECTOR_ADDR=' .env 2>/dev/null | cut -d= -f2-)

# the database goes in /tmp: development data only
COLLECTOR_ENV = CHATTER_BIND=$(COLLECTOR) CHATTER_DB=/tmp/chatter.db

collector:
	$(COLLECTOR_ENV) cargo run -q $(COLLECTOR_CRATE)

# also serves the 10 MiB GET /test.bin the test client downloads
collector-test:
	$(COLLECTOR_ENV) CHATTER_TEST_FILE_MIB=10 cargo run -q $(COLLECTOR_CRATE)

# the monitoring view's static mockup (made-up data), docs/monitoring.md step 1
mockup:
	open crates/collector/ui/mockup.html

test:
	cargo test -p chatter-core
	cargo test -q $(COLLECTOR_CRATE)

test-device:
	$(C6_PROBE) cargo test $(FIRMWARE)

check:
	cargo clippy -p chatter-core --all-targets -- -D warnings
	cargo clippy -p chatter-core --target riscv32imac-unknown-none-elf -- -D warnings
	cargo clippy $(FIRMWARE) --bins --tests -- -D warnings
	cargo clippy $(TESTCLIENT) -- -D warnings
	cargo clippy $(COLLECTOR_CRATE) --all-targets -- -D warnings
	cargo fmt --check
	cargo fmt --check --manifest-path crates/testclient/Cargo.toml
	cargo fmt --check --manifest-path crates/collector/Cargo.toml
