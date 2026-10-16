//! On-device smoke test: the chip boots, the scheduler runs, and timers fire.
//! Run with `make test-device` (flashes the board over probe-rs).

#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(test)]
#[embedded_test::tests(executor = esp_rtos::embassy::Executor::new())]
mod tests {
	use embassy_time::{Duration, Instant, Timer};

	#[init]
	fn init() {
		let peripherals = esp_hal::init(esp_hal::Config::default());

		let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
		esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

		rtt_target::rtt_init_defmt!();
	}

	#[test]
	async fn timer_fires() {
		let start = Instant::now();
		Timer::after(Duration::from_millis(100)).await;
		defmt::assert!(start.elapsed() >= Duration::from_millis(100));
	}
}
