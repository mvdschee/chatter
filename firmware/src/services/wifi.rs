//! Station connection: join the network from `.env`, keep power save off, and
//! reconnect when the link drops. SSIDs and the password are never logged.

use defmt::{info, warn};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{
	AuthenticationMethodConfig, Config, ConnectionError, PowerSaveMode, WifiController,
	sta::StationConfig,
};

use crate::{
	Result,
	config::{RECONNECT_DELAY_SECS, WIFI_PASSWORD, WIFI_SSID},
};

#[allow(
	clippy::large_stack_frames,
	reason = "runs once at boot; the driver config structs are large"
)]
pub fn init(wifi: WIFI<'static>) -> Result<WifiController<'static>> {
	let mut controller = WifiController::new(wifi, Default::default())?;
	let station = StationConfig::default()
		.with_ssid(WIFI_SSID.try_into()?)
		.with_authentication(AuthenticationMethodConfig::Wpa2Personal(WIFI_PASSWORD.try_into()?));
	controller.set_config(&Config::Station(station))?;
	// modem sleep turns the radio off between beacons; the sniffer would miss most frames
	controller.set_power_saving(PowerSaveMode::None)?;
	Ok(controller)
}

/// Keeps trying until the station is connected.
pub async fn connect(controller: &mut WifiController<'static>) {
	loop {
		match controller.connect_async().await {
			Ok(connected) => {
				let rssi = controller.rssi().ok();
				info!(
					"wifi: connected, bssid {=[u8]:02x}, channel {}, rssi {}",
					connected.bssid, connected.channel, rssi
				);
				return;
			},
			// log the reason only: the error also carries the SSID
			Err(ConnectionError::Failed(failed)) => {
				warn!("wifi: connect failed, reason {}", failed.reason)
			},
			Err(e) => warn!("wifi: connect failed: {}", e),
		}
		Timer::after(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
	}
}

/// Waits for the link to drop, then reconnects. Expects a connected station.
#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
	loop {
		match controller.wait_for_disconnect_async().await {
			Ok(gone) => warn!("wifi: disconnected, reason {}", gone.reason),
			Err(e) => warn!("wifi: disconnect wait failed: {}", e),
		}
		connect(&mut controller).await;
	}
}
