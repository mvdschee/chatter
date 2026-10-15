#![no_std]
#![no_main]
#![deny(
	clippy::mem_forget,
	reason = "mem::forget is not safe with esp_hal types that hold buffers during a transfer"
)]
#![deny(clippy::large_stack_frames)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use panic_rtt_target as _;

// app descriptor the esp-idf bootloader expects
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
	rtt_target::rtt_init_defmt!();

	let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

	let timg0 = TimerGroup::new(peripherals.TIMG0);
	esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

	info!("chatter booted");

	let mut beat: u32 = 0;
	loop {
		info!("heartbeat {}", beat);
		beat = beat.wrapping_add(1);
		Timer::after(Duration::from_secs(1)).await;
	}
}
