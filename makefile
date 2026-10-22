FIRMWARE = -p chatter-firmware --target riscv32imac-unknown-none-elf
# crates/collector is its own workspace
COLLECTOR_CRATE = --manifest-path crates/collector/Cargo.toml

# Which board, where it reports and its name all come from .env
# (CHATTER_PROBE, CHATTER_COLLECTOR_ADDR, CHATTER_SENSOR_NAME). The probe ID
# lives there because it contains the board's MAC address; with one board
# plugged in it can stay empty.
PROBE = $(shell grep '^CHATTER_PROBE=' .env 2>/dev/null | cut -d= -f2-)
PROBE_ENV = $(if $(PROBE),PROBE_RS_PROBE=$(PROBE))

# plain `make` lists the targets rather than flashing a board
help:
	@echo "Server:"
	@echo "  make collector     run the collector here (page and reports on CHATTER_BIND); SUDO=1 to sweep"
	@echo "  make test          host tests: core and collector"
	@echo "  make check         clippy and fmt for every crate"
	@echo "Board (the one .env names):"
	@echo "  make run           flash it, reporting to CHATTER_COLLECTOR_ADDR"
	@echo "  make test-device   firmware tests on it"
	@echo "Data:"
	@echo "  make port-scan     one port scan now, into CHATTER_DB (about half an hour); SUDO=1"

run:
	$(PROBE_ENV) cargo run $(FIRMWARE)

# The collector takes its settings from .env (CHATTER_BIND, CHATTER_DB, ...,
# see crates/collector/src/config.rs); its container gets the same keys.
# Built first, so it's always the current code. SUDO=1 runs it as root, which
# the network sweep needs on a Mac.
collector:
	cargo build -q $(COLLECTOR_CRATE)
	$(if $(SUDO),sudo) target/debug/chatter-collector

# one port scan now (about half an hour), into CHATTER_DB; SUDO=1 on a Mac
port-scan:
	cargo build -q $(COLLECTOR_CRATE)
	$(if $(SUDO),sudo) target/debug/chatter-collector port-scan

test:
	cargo test -p chatter-core
	cargo test -q $(COLLECTOR_CRATE)

test-device:
	$(PROBE_ENV) cargo test $(FIRMWARE)

check:
	cargo clippy -p chatter-core --all-targets -- -D warnings
	cargo clippy -p chatter-core --target riscv32imac-unknown-none-elf -- -D warnings
	cargo clippy $(FIRMWARE) --bins --tests -- -D warnings
	cargo clippy $(COLLECTOR_CRATE) --all-targets -- -D warnings
	cargo fmt --check
	cargo fmt --check --manifest-path crates/collector/Cargo.toml
