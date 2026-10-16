//! The IP stack on the station interface: DHCP and TCP, for posting reports.

use embassy_net::{Runner, Stack, StackResources};
use esp_hal::rng::Rng;
use esp_radio::wifi::Interface;
use static_cell::ConstStaticCell;

// report socket, plus room for DHCP and DNS
static RESOURCES: ConstStaticCell<StackResources<3>> = ConstStaticCell::new(StackResources::new());

pub fn init(interface: Interface) -> (Stack<'static>, Runner<'static, Interface>) {
	let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
	embassy_net::new(
		interface,
		embassy_net::Config::dhcpv4(Default::default()),
		RESOURCES.take(),
		seed,
	)
}

#[embassy_executor::task]
pub async fn run(mut runner: Runner<'static, Interface>) {
	runner.run().await
}
