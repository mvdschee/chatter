//! ESP32-C3 test client. Single-stream, so the sensor can decode its traffic:
//! it joins the network, downloads `/test.bin` from the collector
//! (`make collector-test`) once a minute, and logs exactly how many bytes arrived. Compare that with the
//! sensor's rx bytes for this board's MAC.

#![no_std]
#![no_main]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

use core::net::SocketAddrV4;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_net::{Runner, StackResources, tcp::TcpSocket};
use embassy_time::{Duration, Instant, Timer};
use embedded_io_async::Write;
use esp_hal::{clock::CpuClock, rng::Rng, timer::timg::TimerGroup};
use esp_radio::wifi::{
	AuthenticationMethodConfig, Config, Interface, PowerSaveMode, WifiController,
	sta::StationConfig,
};
use panic_rtt_target as _;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const SSID: &str = env!("CHATTER_WIFI_SSID");
const PASSWORD: &str = env!("CHATTER_WIFI_PASSWORD");
const ADDR: &str = env!("CHATTER_COLLECTOR_ADDR");
const PATH: &str = "/test.bin";
const EVERY_SECS: u64 = 60;

static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static RX_BUF: StaticCell<[u8; 4096]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 512]> = StaticCell::new();
static READ_BUF: StaticCell<[u8; 1536]> = StaticCell::new();

#[allow(
	clippy::large_stack_frames,
	reason = "runs once at boot; the driver config structs are large"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
	rtt_target::rtt_init_defmt!();

	let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
	esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);

	let timg0 = TimerGroup::new(peripherals.TIMG0);
	esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

	let addr: SocketAddrV4 = ADDR.parse().expect("CHATTER_COLLECTOR_ADDR is not <ip>:<port>");

	let mut controller =
		WifiController::new(peripherals.WIFI, Default::default()).expect("wifi init failed");
	let station = StationConfig::default()
		.with_ssid(SSID.try_into().expect("SSID too long"))
		.with_authentication(AuthenticationMethodConfig::Wpa2Personal(
			PASSWORD.try_into().expect("password too long"),
		));
	controller.set_config(&Config::Station(station)).expect("wifi config failed");
	controller.set_power_saving(PowerSaveMode::None).expect("power save failed");

	let interface = Interface::station();
	info!("testclient: mac {=[u8]:02x}", interface.mac_address());

	let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
	let (stack, runner) = embassy_net::new(
		interface,
		embassy_net::Config::dhcpv4(Default::default()),
		RESOURCES.init(StackResources::new()),
		seed,
	);
	spawner.spawn(net(runner).expect("net task already running"));
	spawner.spawn(connection(controller).expect("connection task already running"));

	stack.wait_config_up().await;
	info!("testclient: got an address");

	let rx = RX_BUF.init([0; 4096]);
	let tx = TX_BUF.init([0; 512]);
	let buf = READ_BUF.init([0; 1536]);
	loop {
		let mut socket = TcpSocket::new(stack, rx, tx);
		socket.set_timeout(Some(Duration::from_secs(10)));
		let start = Instant::now();
		match download(&mut socket, addr, buf).await {
			Ok(bytes) => info!(
				"testclient: downloaded {} bytes (incl. HTTP headers) in {} ms",
				bytes,
				start.elapsed().as_millis()
			),
			Err(e) => warn!("testclient: download failed after {} bytes", e),
		}
		socket.close();
		drop(socket);
		Timer::after(Duration::from_secs(EVERY_SECS)).await;
	}
}

/// Plain HTTP/1.0 GET; the server closes the connection when it's done.
/// Returns every byte received, headers included. On error, how many bytes
/// had arrived.
async fn download(
	socket: &mut TcpSocket<'_>,
	addr: SocketAddrV4,
	buf: &mut [u8],
) -> Result<u32, u32> {
	socket.connect((*addr.ip(), addr.port())).await.map_err(|_| 0u32)?;
	socket.write_all(b"GET ").await.map_err(|_| 0u32)?;
	socket.write_all(PATH.as_bytes()).await.map_err(|_| 0u32)?;
	socket.write_all(b" HTTP/1.0\r\n\r\n").await.map_err(|_| 0u32)?;

	let mut total: u32 = 0;
	loop {
		match socket.read(buf).await {
			Ok(0) => return Ok(total),
			Ok(n) => total += n as u32,
			Err(_) => return Err(total),
		}
	}
}

#[embassy_executor::task]
async fn net(mut runner: Runner<'static, Interface>) {
	runner.run().await
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
	loop {
		match controller.connect_async().await {
			Ok(_) => {
				info!("testclient: wifi connected");
				let _ = controller.wait_for_disconnect_async().await;
				warn!("testclient: wifi disconnected");
			},
			Err(_) => warn!("testclient: wifi connect failed"),
		}
		Timer::after(Duration::from_secs(5)).await;
	}
}
