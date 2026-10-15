FIRMWARE = -p chatter-firmware --target riscv32imac-unknown-none-elf

run:
	cargo run $(FIRMWARE)

test:
	cargo test -p chatter-core

test-device:
	cargo test $(FIRMWARE)

check:
	cargo clippy -p chatter-core --all-targets -- -D warnings
	cargo clippy -p chatter-core --target riscv32imac-unknown-none-elf -- -D warnings
	cargo clippy $(FIRMWARE) --bins --tests -- -D warnings
	cargo fmt --check
