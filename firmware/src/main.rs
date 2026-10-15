#![no_std]
#![no_main]
#![deny(
	clippy::mem_forget,
	reason = "mem::forget is not safe with esp_hal types that hold buffers during a transfer"
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

mod config;
mod error;
mod services;

use defmt::info;
use embassy_executor::Spawner;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use panic_rtt_target as _;

pub use error::{Error, Result};
use services::wifi;

// app descriptor the esp-idf bootloader expects
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
	rtt_target::rtt_init_defmt!();

	let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
	// the radio driver allocates
	esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);

	let timg0 = TimerGroup::new(peripherals.TIMG0);
	esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

	info!("chatter booted");

	let mut controller = wifi::init(peripherals.WIFI).expect("wifi init failed");
	wifi::connect(&mut controller).await;
	spawner.spawn(wifi::connection(controller).expect("wifi task already running"));

	loop {
		core::future::pending::<()>().await;
	}
}
